use super::Server;
use crate::{
	docker::{local_ipv6_cidr, IPv6CIDR},
	Config
};
use bollard::Docker;
use std::path::Path;

pub struct LocalServer;

impl LocalServer {
	pub fn new() -> Self {
		Self
	}
}

#[async_trait]
impl Server for LocalServer {
	async fn install(&self, _config: &Config, _repodir: &Path) -> anyhow::Result<()> {
		Ok(())
	}

	fn connect_to_docker(&self) -> Result<Docker, bollard::errors::Error> {
		Docker::connect_with_local_defaults()
	}

	fn repomount(&self, repodir: &Path) -> String {
		repodir.to_str().unwrap().to_owned()
	}

	fn cores(&self) -> u16 {
		num_cpus::get() as u16
	}

	fn cidr_v6(&self) -> IPv6CIDR<String> {
		local_ipv6_cidr().expect("Failed to parse /etc/docker/daemon.json - Is your docker daemon IPv6-enabled?")
	}

	async fn upload_repo_changes(&self, _config: &Config, _repodir: &Path) -> anyhow::Result<()> {
		anyhow::bail!("Uploading changes is not supported without UpCloud")
	}

	async fn destroy(self) -> anyhow::Result<()> {
		Ok(())
	}
}
