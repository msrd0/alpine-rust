use futures_util::future;
use rand::{distributions::Alphanumeric, thread_rng, Rng};
use serde::{Deserialize, Serialize};
use ssh2::Session;
use std::{
	env,
	io::{BufRead, BufReader, Write},
	net::{SocketAddr, TcpStream},
	sync::Arc
};
use surf::{http::mime::JSON, Body};

#[derive(Serialize)]
struct CreateServerRequest {
	server: CreateServer
}

#[derive(Serialize)]
struct CreateServer {
	title: String,
	hostname: String,
	plan: String,
	zone: String,
	storage_devices: StorageDevices
}

#[derive(Serialize)]
struct StorageDevices {
	storage_device: Vec<StorageDevice>
}

#[derive(Serialize)]
struct StorageDevice {
	action: String,
	storage: String,
	title: String,
	size: u32,
	tier: String
}

#[derive(Deserialize)]
struct ServerResponse {
	server: Server
}

#[derive(Deserialize)]
struct Server {
	ip_addresses: IpAddresses,
	password: String,
	uuid: String
}

#[derive(Deserialize)]
struct IpAddresses {
	ip_address: Vec<IpAddress>
}

#[derive(Deserialize)]
struct IpAddress {
	access: String,
	address: String,
	family: String
}

impl ServerResponse {
	fn ip_addr(&self) -> Option<&str> {
		self.server
			.ip_addresses
			.ip_address
			.iter()
			.filter(|addr| addr.access == "public" && addr.family == "IPv4")
			.map(|addr| addr.address.as_ref())
			.next()
	}

	fn password(&self) -> &str {
		&self.server.password
	}

	fn uuid(&self) -> &str {
		&self.server.uuid
	}
}

impl CreateServerRequest {
	fn new(title: String, hostname: String) -> Self {
		let storage_title = format!("{} (Debian Buster)", title);
		Self {
			server: CreateServer {
				title,
				hostname,
				plan: "custom".to_owned(),
				zone: "de-fra1".to_owned(),
				storage_devices: StorageDevices {
					storage_device: vec![StorageDevice {
						action: "clone".to_owned(),
						storage: "01000000-0000-4000-8000-000020050100".to_owned(),
						title: storage_title,
						size: 10,
						tier: "maxiops".to_owned()
					}]
				}
			}
		}
	}

	async fn send(&self, username: &str, password: &str) -> surf::Result<ServerResponse> {
		let auth = format!("{}:{}", username, password);
		let value: serde_json::Value = surf::post("https://api.upcloud.com/1.3/server")
			.header("authorization", format!("Basic {}", base64::encode(auth.as_bytes())))
			.content_type(JSON)
			.body(Body::from_json(self)?)
			.recv_json()
			.await?;
		info!("Response: {:?}", value);
		Ok(serde_json::from_value(value)?)
	}
}

fn run(sess: &mut Session, cmd: &str) -> anyhow::Result<()> {
	info!("SSH: Running `{}`", cmd);
	let mut channel = sess.channel_session()?;
	channel.exec(cmd)?;

	let reader = BufReader::new(&mut channel);
	for line in reader.lines() {
		println!("[ssh] {}", line?);
	}

	channel.wait_close()?;
	let exit_code = channel.exit_status()?;
	info!("SSH: Command completed with exit code {}", exit_code);
	if exit_code == 0 {
		Ok(())
	} else {
		Err(anyhow::Error::msg(format!(
			"Command `{}` returned exit-code {}",
			cmd, exit_code
		)))
	}
}

fn send(sess: &mut Session, path: &str, data: &[u8]) -> anyhow::Result<()> {
	info!("SSH: Uploading {}", path);
	let mut file = sess.scp_send(path.as_ref(), 0o600, data.len() as u64, None)?;
	file.write(data)?;

	file.send_eof()?;
	file.wait_eof()?;
	file.close()?;
	file.wait_close()?;

	Ok(())
}

pub(super) async fn launch_server(ca_pem: &[u8], cert_pem: &[u8], key_pem: &[u8]) -> surf::Result<()> {
	let rng = thread_rng();
	let hostname = rng.sample_iter(Alphanumeric).take(10).collect::<String>();
	let title = format!("alpine-rust-{}", hostname);

	info!("Creating Server {}", title);
	let username = "msrd0";
	let password = env::var("UPCLOUD_PASSWORD")?;
	let req = CreateServerRequest::new(title, "alpinerust".to_owned());
	//let server = req.send(username, &password).await?;
	//let ip = server.ip_addr().ok_or(anyhow::Error::msg("Server does not have an IP"))?;
	let ip = "94.237.102.87";
	let password = "REDACTED";

	let tcp = TcpStream::connect(format!("{}:22", ip).parse::<SocketAddr>()?)?;
	let mut sess = Session::new()?;
	sess.set_tcp_stream(tcp);
	sess.handshake()?;
	sess.userauth_password("root", password)?;

	// install docker and stop it after the stupid autostart
	run(&mut sess, "apt-get update -y")?;
	run(&mut sess, "apt-get install -y --no-install-recommends apt-transport-https ca-certificates curl gnupg2 software-properties-common")?;
	run(
		&mut sess,
		"curl -fsSL https://download.docker.com/linux/debian/gpg | apt-key add -"
	)?;
	run(&mut sess, "echo 'deb [arch=amd64] https://download.docker.com/linux/debian buster stable' >/etc/apt/sources.list.d/docker.list")?;
	run(&mut sess, "apt-get update -y")?;
	run(&mut sess, "apt-get install -y --no-install-recommends docker-ce")?;
	run(&mut sess, "systemctl disable --now docker")?;

	// upload the certificates
	run(&mut sess, "mkdir -p /etc/docker-certs")?;
	send(&mut sess, "/etc/docker-certs/ca.pem", ca_pem)?;
	send(&mut sess, "/etc/docker-certs/cert.pem", cert_pem)?;
	send(&mut sess, "/etc/docker-certs/key.pem", key_pem)?;

	// install the new docker systemd unit
	send(
		&mut sess,
		"/etc/systemd/system/docker-tlsverify.service",
		DOCKER_SYSTEMD_UNIT.as_bytes()
	)?;
	run(&mut sess, "systemctl daemon-reload")?;
	run(&mut sess, "systemctl enable --now docker-tlsverify")?;

	Ok(())
}

const DOCKER_SYSTEMD_UNIT: &str = r#"
# Adopted from /lib/systemd/system/docker.service
[Unit]
Description=Docker Application Container Engine
Documentation=https://docs.docker.com
BindsTo=containerd.service
After=network-online.target firewalld.service containerd.service
Wants=network-online.target

[Service]
Type=notify
# the default is not to use systemd for cgroups because the delegate issues still
# exists and systemd currently does not support the cgroup feature set required
# for containers run by docker
ExecStart=/usr/bin/dockerd --tlsverify --tlscacert=/etc/docker-certs/ca.pem --tlscert=/etc/docker-certs/cert.pem --tlskey=/etc/docker-certs/key.pem -H=0.0.0.0:2376 --containerd=/run/containerd/containerd.sock
ExecReload=/bin/kill -s HUP \$MAINPID
TimeoutSec=0
RestartSec=2
Restart=always

# Note that StartLimit* options were moved from "Service" to "Unit" in systemd 229.
# Both the old, and new location are accepted by systemd 229 and up, so using the old location
# to make them work for either version of systemd.
StartLimitBurst=3

# Note that StartLimitInterval was renamed to StartLimitIntervalSec in systemd 230.
# Both the old, and new name are accepted by systemd 230 and up, so using the old name to make
# this option work for either version of systemd.
StartLimitInterval=60s

# Having non-zero Limit*s causes performance problems due to accounting overhead
# in the kernel. We recommend using cgroups to do container-local accounting.
LimitNOFILE=infinity
LimitNPROC=infinity
LimitCORE=infinity

# Comment TasksMax if your systemd version does not support it.
# Only systemd 226 and above support this option.
TasksMax=infinity

# set delegate yes so that systemd does not reset the cgroups of docker containers
Delegate=yes

# kill only the docker process, not all processes in the cgroup
KillMode=process

[Install]
WantedBy=multi-user.target
"#;
