use super::Config;
use crate::{
	docker::{gen_keys, DockerKeys},
	run_git, APKBUILD
};
use futures_util::StreamExt;
use rand::{distributions::Alphanumeric, thread_rng, Rng};
use serde::{Deserialize, Serialize};
use ssh2::Session;
use std::{
	collections::HashMap,
	env,
	io::{BufRead, BufReader, Read, Write},
	net::{TcpStream, ToSocketAddrs},
	path::Path,
	time::Duration
};
use surf::{http::mime::JSON, Body};
use tokio::{
	fs::{self, File},
	io::{AsyncReadExt, AsyncWriteExt},
	time::delay_for
};

// using 8 cores for now - 10 cores took 1h 13min, very marginal improvements above 10 cores
pub const UPCLOUD_CORES: u16 = 8;
// compiling rust doesn't take much memory so we'll stick with 1G per core
pub const UPCLOUD_MEMORY: u16 = 8 * 1024;
// the OS comes in at about 3G and compiling rust doesn't take much so 15G should be plenty
pub const UPCLOUD_STORAGE: u16 = 15;

#[derive(Serialize)]
struct CreateServerRequest {
	server: CreateServer
}

#[derive(Serialize)]
struct CreateServer {
	title: String,
	hostname: String,
	plan: String,
	core_number: String,
	memory_amount: String,
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
	size: u16,
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
				core_number: UPCLOUD_CORES.to_string(),
				memory_amount: UPCLOUD_MEMORY.to_string(),
				zone: "de-fra1".to_owned(),
				storage_devices: StorageDevices {
					storage_device: vec![StorageDevice {
						action: "clone".to_owned(),
						storage: "01000000-0000-4000-8000-000020050100".to_owned(),
						title: storage_title,
						size: UPCLOUD_STORAGE,
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

#[derive(Serialize)]
struct StopServerRequest {
	stop_server: StopServer
}

#[derive(Serialize)]
struct StopServer {
	stop_type: &'static str,
	timeout: &'static str
}

impl StopServerRequest {
	fn new() -> Self {
		Self {
			stop_server: StopServer {
				stop_type: "soft",
				timeout: "30"
			}
		}
	}

	async fn send(&self, username: &str, password: &str, server_uuid: &str) -> surf::Result<serde_json::Value> {
		let auth = format!("{}:{}", username, password);
		let value: serde_json::Value = surf::post(format!("https://api.upcloud.com/1.3/server/{}/stop", server_uuid))
			.header("authorization", format!("Basic {}", base64::encode(auth.as_bytes())))
			.content_type(JSON)
			.body(Body::from_json(self)?)
			.recv_json()
			.await?;
		info!("Response: {:?}", value);
		Ok(value)
	}
}

#[derive(Serialize)]
struct DeleteServerRequest {
	storages: u8
}

impl DeleteServerRequest {
	fn new() -> Self {
		Self { storages: 1 }
	}

	async fn send(&self, username: &str, password: &str, server_uuid: &str) -> surf::Result<()> {
		let auth = format!("{}:{}", username, password);
		let bytes = surf::delete(format!("https://api.upcloud.com/1.3/server/{}", server_uuid))
			.query(self)?
			.header("authorization", format!("Basic {}", base64::encode(auth.as_bytes())))
			.recv_bytes()
			.await?;
		info!("Response: {:?}", bytes);
		Ok(())
	}
}

fn try_connect(domain: &str, password: &str) -> anyhow::Result<Session> {
	let addr = (domain, 22).to_socket_addrs()?.next().unwrap();
	let tcp = TcpStream::connect(addr)?;
	let mut sess = Session::new()?;
	sess.set_tcp_stream(tcp);
	sess.handshake()?;
	sess.userauth_password("root", password)?;
	Ok(sess)
}

async fn connect(domain: &str, password: &str) -> anyhow::Result<Session> {
	let mut error = None;
	for i in 0..10 {
		info!("Connecting to {}:22", domain);
		let err = match try_connect(domain, password) {
			Ok(sess) => return Ok(sess),
			Err(err) => err
		};
		error!("{}", err);
		error = Some(err);

		let wait = 2 * (i + 1);
		info!("Retrying SSH connection in {}s", wait);
		delay_for(Duration::new(wait, 0)).await;
	}
	Err(error.unwrap())
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

fn index(sess: &mut Session, path: &str) -> anyhow::Result<HashMap<String, String>> {
	info!("SSH: Indexing {}", path);
	let mut channel = sess.channel_session()?;
	let cmd = format!("cd '{}' && sha256sum *", path);
	channel.exec(&cmd)?;

	let mut index: HashMap<String, String> = HashMap::new();
	let reader = BufReader::new(&mut channel);
	for line in reader.lines() {
		let line = line?;
		let whitespace = match line.find(|c: char| c.is_whitespace()) {
			Some(index) => index,
			None => {
				warn!("SSH: Skipping unknown line {:?}", line);
				continue;
			}
		};
		let hash = line[0..whitespace].trim();
		let name = line[whitespace..].trim();
		index.insert(name.to_owned(), hash.to_owned());
	}

	channel.wait_close()?;
	let exit_code = channel.exit_status()?;
	info!("SSH: Command completed with exit code {}", exit_code);
	if exit_code == 0 {
		Ok(index)
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

async fn upload(sess: &mut Session, path: &str, host: &Path) -> anyhow::Result<()> {
	info!("SSH: Uploading {}", path);
	let mut host = File::open(host).await?;
	let mut file = sess.scp_send(path.as_ref(), 0o600, host.metadata().await?.len(), None)?;

	let mut buf = [0u8; 8192];
	loop {
		let count = host.read(&mut buf).await?;
		if count == 0 {
			break;
		}
		file.write(&buf[0..count])?;
	}

	file.send_eof()?;
	file.wait_eof()?;
	file.close()?;
	file.wait_close()?;

	Ok(())
}

async fn download(sess: &mut Session, path: &str, host: &Path) -> anyhow::Result<()> {
	info!("SSH: Downloading {}", path);
	let mut host = File::create(host).await?;
	let (mut file, _) = sess.scp_recv(path.as_ref())?;

	let mut buf = [0u8; 8192];
	loop {
		let count = file.read(&mut buf)?;
		if count == 0 {
			break;
		}
		host.write(&buf[0..count]).await?;
	}

	file.send_eof()?;
	file.wait_eof()?;
	file.close()?;
	file.wait_close()?;

	Ok(())
}

#[allow(dead_code)]
pub(super) struct UpcloudServer {
	pub(super) ip: String,
	pub(super) domain: String,
	password: String,
	uuid: String,
	pub(super) keys: DockerKeys,
	repo_dir: String,
	repo_index: HashMap<String, String>
}

impl UpcloudServer {
	pub(super) async fn destroy(&self) -> surf::Result<()> {
		info!("Removing Server {}", self.uuid);

		let username = "msrd0";
		let password = env::var("UPCLOUD_PASSWORD")?;

		StopServerRequest::new().send(username, &password, &self.uuid).await?;
		delay_for(Duration::new(30, 0)).await;
		DeleteServerRequest::new().send(username, &password, &self.uuid).await?;

		Ok(())
	}
}

pub(super) async fn launch_server(config: &Config, repodir: &Path) -> surf::Result<UpcloudServer> {
	let rng = thread_rng();
	let hostname = rng.sample_iter(Alphanumeric).take(10).collect::<String>();
	let title = format!("alpine-rust-{}", hostname);

	info!("Creating Server {}", title);
	let username = "msrd0";
	let password = env::var("UPCLOUD_PASSWORD")?;
	let req = CreateServerRequest::new(title, "alpinerust".to_owned());
	let server = req.send(username, &password).await?;
	let ip = server.ip_addr().ok_or(anyhow::Error::msg("Server does not have an IP"))?;
	let password = server.password();
	let uuid = server.uuid();

	// rustls doesn't support ip's, so we need to guess a dns name
	let domain = format!("{}.de-fra1.upcloud.host", ip.split('.').collect::<Vec<_>>().join("-"));

	// wait for the server and domain to be set up
	delay_for(Duration::new(10, 0)).await;

	// open an SSH connection
	let mut sess = connect(&domain, password).await?;

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
	let keys = gen_keys(ip, &domain).await?;
	run(&mut sess, "mkdir -p /etc/docker-certs")?;
	send(&mut sess, "/etc/docker-certs/ca.pem", &keys.ca_pem)?;
	send(&mut sess, "/etc/docker-certs/cert.pem", &keys.server_cert_pem)?;
	send(&mut sess, "/etc/docker-certs/key.pem", &keys.server_key_pem)?;

	// install the new docker systemd unit
	send(
		&mut sess,
		"/etc/systemd/system/docker-tlsverify.service",
		DOCKER_SYSTEMD_UNIT.as_bytes()
	)?;
	run(&mut sess, "systemctl daemon-reload")?;
	run(&mut sess, "systemctl enable --now docker-tlsverify")?;

	// upload the repository content
	let dir = format!("/var/lib/alpine-rust/{}/alpine-rust/x86_64", config.alpine);
	run(&mut sess, &format!("mkdir -p {}", dir))?;
	let mut entries = fs::read_dir(repodir.join(format!("{}/alpine-rust/x86_64", config.alpine))).await?;
	while let Some(entry) = entries.next().await {
		let entry = entry?;
		upload(
			&mut sess,
			&format!("{}/{}", dir, entry.file_name().to_string_lossy()),
			&entry.path()
		)
		.await?;
	}
	run(&mut sess, "chmod 777 $(find /var/lib/alpine-rust -type d)")?;
	run(&mut sess, "chmod 666 $(find /var/lib/alpine-rust -type f)")?;

	// index the repository
	let repo_index = index(&mut sess, &dir)?;
	info!("Index: {:?}", repo_index);

	Ok(UpcloudServer {
		ip: ip.to_owned(),
		domain,
		password: password.to_owned(),
		uuid: uuid.to_owned(),
		keys,
		repo_dir: dir,
		repo_index
	})
}

pub(super) async fn commit_changes(
	config: &Config,
	ver: &APKBUILD,
	repodir: &Path,
	server: &mut UpcloudServer
) -> anyhow::Result<()> {
	// establish a new ssh session
	let mut sess = connect(&server.domain, &server.password).await?;

	// pull the current index
	let dir = format!("/var/lib/alpine-rust/{}/alpine-rust/x86_64", config.alpine);
	let new_index = index(&mut sess, &dir)?;

	// get all updated files - the build will never delete files
	let updated = new_index
		.into_iter()
		.filter(|(file, hash)| server.repo_index.get(file.as_str()) != Some(&hash))
		.map(|(file, _)| file)
		.collect::<Vec<_>>();
	if updated.is_empty() {
		info!("No changes to commit");
		return Ok(());
	}

	// download those files and add them to git
	let mut err: Option<&'static str> = None;
	for file in &updated {
		download(
			&mut sess,
			&format!("{}/{}", dir, file),
			&repodir.join(format!("{}/alpine-rust/x86_64/{}", config.alpine, file))
		)
		.await?;
		if !run_git(repodir, &["add", &format!("{}/alpine-rust/x86_64/{}", config.alpine, file)]) {
			error!("Unable to add {} to git", file);
			err = Some("Unable to add files to git");
		}
	}

	// create the commit
	info!("Commiting changes for rust-1.{}", ver.rustminor);
	let msg = format!("Update rust-1.{} package for alpine {}", ver.rustminor, config.alpine);
	if !run_git(repodir, &["commit", "-m", &msg]) {
		error!("Failed to create commit");
		err = Some("Failed to create commit");
	}
	if !run_git(repodir, &["push"]) {
		error!("Failed to push commit");
		err = Some("Failed to push commit");
	}

	if let Some(err) = err {
		return Err(anyhow::Error::msg(err));
	}
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
ExecStart=/usr/bin/dockerd --tlsverify --tlscacert=/etc/docker-certs/ca.pem --tlscert=/etc/docker-certs/cert.pem --tlskey=/etc/docker-certs/key.pem -H=0.0.0.0:8443 --containerd=/run/containerd/containerd.sock
ExecReload=/bin/kill -s HUP $MAINPID
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
