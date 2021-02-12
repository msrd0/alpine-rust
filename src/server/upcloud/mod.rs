use super::Server;
use crate::{
	docker::{gen_docker_keys, DockerKeys, IPv6CIDR},
	repo, Config
};
use bollard::{Docker, API_DEFAULT_VERSION};
use futures_util::StreamExt;
use rand::{distributions::Alphanumeric, thread_rng, Rng};
use serde_json::json;
use std::{collections::HashMap, env, path::Path};
use tokio::fs;

mod api;
use api::*;

mod ssh;
use ssh::*;

// using 8 cores for now - 10 cores took 1h 13min, very marginal improvements above 10 cores
pub const UPCLOUD_CORES: u16 = 8;
// compiling rust doesn't take much memory so we'll stick with 1G per core
pub const UPCLOUD_MEMORY: u16 = 8 * 1024;
// the OS comes in at about 3G and compiling rust doesn't take much so 20G should be plenty
pub const UPCLOUD_STORAGE: u16 = 20;

// the IPv6 CIDR that will be used by the docker server
#[allow(non_upper_case_globals)] // IPv6 is correct
pub const UPCLOUD_IPv6CIDR: IPv6CIDR<&str> = IPv6CIDR::new("fd00:dead:beef::", 48);

fn ip_last_parts(ip: &str) -> String {
	let mut parts = [""; 4];
	for part in ip.split(":") {
		parts[0] = parts[1];
		parts[1] = parts[2];
		parts[2] = parts[3];
		parts[3] = part;
	}
	parts.join("-")
}

#[allow(dead_code)]
pub struct UpcloudServer {
	ip: String,
	domain: String,
	password: String,
	uuid: String,
	keys: DockerKeys,
	repo_dir: String,
	repo_index: HashMap<String, String>
}

impl UpcloudServer {
	pub async fn create(config: &Config) -> anyhow::Result<Self> {
		let rng = thread_rng();
		let hostname = rng.sample_iter(Alphanumeric).take(10).map(char::from).collect::<String>();
		let title = format!("alpine-rust-{}", hostname);

		info!("Creating Server {}", title);
		let username = "msrd0";
		let password = env::var("UPCLOUD_PASSWORD")?;
		let req = CreateServerRequest::new(&title, "alpinerust");
		let res = req.send(username, &password).await?;

		let ip = res.ip_addr().ok_or(anyhow::Error::msg("Server does not have an IP"))?;
		let password = res.password();
		let uuid = res.uuid();

		// rustls doesn't support ip's, so we need to guess a dns name
		let domain = format!("{}.v6.de-fra1.upcloud.host", ip_last_parts(ip));

		// generate some keys for docker to use with TLS
		let keys = gen_docker_keys(ip, &domain).await?;

		let repo_dir = format!("/var/lib/alpine-rust/{}/alpine-rust/x86_64", config.alpine.version);
		Ok(UpcloudServer {
			ip: ip.to_owned(),
			domain,
			password: password.to_owned(),
			uuid: uuid.to_owned(),
			keys,
			repo_dir,
			repo_index: HashMap::new()
		})
	}
}

#[async_trait]
impl Server for UpcloudServer {
	async fn install(&self, config: &Config, repodir: &Path) -> anyhow::Result<()> {
		// open an SSH connection
		let mut sess = connect(&self.domain, &self.password).await?;

		// install docker and stop it after the stupid autostart
		run(&mut sess, "apt-get update -y")?;
		run(&mut sess, "apt-get install -y --no-install-recommends apt-transport-https ca-certificates curl gnupg2 software-properties-common")?;
		run(
			&mut sess,
			"curl -fsSL https://download.docker.com/linux/debian/gpg | apt-key add -"
		)?;
		send(
			&mut sess,
			"/etc/apt/sources.list.d/docker.list",
			b"deb [arch=amd64] https://download.docker.com/linux/debian buster stable"
		)?;
		run(&mut sess, "apt-get update -y")?;
		run(&mut sess, "apt-get install -y --no-install-recommends docker-ce")?;
		run(&mut sess, "systemctl disable --now docker")?;

		// upload the certificates
		run(&mut sess, "mkdir -p /etc/docker-certs")?;
		send(&mut sess, "/etc/docker-certs/ca.pem", &self.keys.ca_pem)?;
		send(&mut sess, "/etc/docker-certs/cert.pem", &self.keys.server_cert_pem)?;
		send(&mut sess, "/etc/docker-certs/key.pem", &self.keys.server_key_pem)?;

		// make sure that our docker is IPv6-enabled
		// we will use this when testing to communicate with the host over IPv6, so we can skip
		// iptables rules, meaning no NAT-ing for now
		let json = json!({ "ipv6": true, "fixed-cidr-v6": UPCLOUD_IPv6CIDR });
		send(&mut sess, "/etc/docker/daemon.json", &serde_json::to_vec(&json)?)?;

		// install the new docker systemd unit
		send(
			&mut sess,
			"/etc/systemd/system/docker-tlsverify.service",
			DOCKER_SYSTEMD_UNIT.as_bytes()
		)?;
		run(&mut sess, "systemctl daemon-reload")?;
		run(&mut sess, "systemctl enable --now docker-tlsverify")?;

		// upload the repository content
		let dir = format!("/var/lib/alpine-rust/{}/alpine-rust/x86_64", config.alpine.version);
		run(&mut sess, &format!("mkdir -p {}", dir))?;
		let mut entries = fs::read_dir(repodir.join(format!("{}/alpine-rust/x86_64", config.alpine.version))).await?;
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
		run(&mut sess, &format!("test ! -e /var/lib/alpine-rust/{}/alpine-rust/x86_64/APKINDEX.tar.gz || chmod 666 $(find /var/lib/alpine-rust -type f)", config.alpine.version))?;

		// index the repository
		let repo_index = index(&mut sess, &dir)?;
		debug!("Index: {:?}", repo_index);

		Ok(())
	}

	fn connect_to_docker(&self) -> Result<Docker, bollard::errors::Error> {
		let docker_addr = format!("tcp://{}:8443/", self.domain);
		info!("Connecting to {}", docker_addr);
		Docker::connect_with_ssl(
			&docker_addr,
			&self.keys.client_key_path(),
			&self.keys.client_cert_path(),
			&self.keys.ca_path(),
			120,
			API_DEFAULT_VERSION
		)
	}

	fn repomount(&self, _repodir: &Path) -> String {
		"/var/lib/alpine-rust".to_owned()
	}

	fn cores(&self) -> u16 {
		UPCLOUD_CORES
	}

	fn cidr_v6(&self) -> IPv6CIDR<String> {
		UPCLOUD_IPv6CIDR.to_owned()
	}

	async fn upload_repo_changes(&mut self, config: &Config, repodir: &Path) -> anyhow::Result<()> {
		// establish a new ssh session
		let mut sess = connect(&self.domain, &self.password).await?;

		// pull the current index
		let dir = format!("/var/lib/alpine-rust/{}/alpine-rust/x86_64", config.alpine.version);
		let new_index = index(&mut sess, &dir)?;

		// get all updated files - the build will never delete files
		let updated = new_index
			.into_iter()
			.filter(|(file, hash)| self.repo_index.get(file.as_str()) != Some(&hash))
			.map(|(file, _)| file)
			.collect::<Vec<_>>();
		if updated.is_empty() {
			info!("No changes to commit");
			return Ok(());
		}

		// download those files and upload to the repo
		let mut res: anyhow::Result<()> = Ok(());
		for file in &updated {
			let path = format!("{}/alpine-rust/x86_64/{}", config.alpine.version, file);
			let dest = repodir.join(&path);
			download(&mut sess, &format!("{}/{}", dir, file), &dest).await?;
			if let Err(err) = repo::upload(&dest, &path).await {
				error!("Error uploading {}: {}", path, err);
				res = Err(err);
			}
		}
		res
	}

	async fn destroy(self) -> anyhow::Result<()> {
		destroy_server(&self.uuid).await
	}
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
