use super::Server;
use crate::{
	docker::{local_ipv6_cidr, IPv6CIDR},
	repo, Config
};
use anyhow::anyhow;
use bollard::Docker;
use inotify::{Inotify, WatchMask};
use std::path::{Path, PathBuf};

pub struct LocalServer {
	inotify: Inotify
}

impl LocalServer {
	pub fn new(config: &Config, repodir: &Path) -> Self {
		let mut inotify = Inotify::init().expect("Failed to init inotify");
		let dir = repodir.join(format!("{}/alpine-rust/x86_64", config.alpine.version));
		inotify
			.add_watch(&dir, WatchMask::CREATE | WatchMask::MODIFY)
			.expect("Failed to watch repodir");

		Self { inotify }
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
		let repomount = std::fs::canonicalize(repodir).unwrap();
		repomount.to_str().unwrap().to_owned()
	}

	fn cores(&self) -> u16 {
		num_cpus::get() as u16
	}

	fn cidr_v6(&self) -> IPv6CIDR<String> {
		local_ipv6_cidr().expect("Failed to parse /etc/docker/daemon.json - Is your docker daemon IPv6-enabled?")
	}

	async fn upload_repo_changes(&mut self, config: &Config, _repodir: &Path) -> anyhow::Result<()> {
		let mut res: anyhow::Result<()> = Ok(());
		let mut buf = [0u8; 4096];
		loop {
			let mut events = self.inotify.read_events(&mut buf)?.peekable();
			if events.peek().is_none() {
				break;
			}

			for event in events {
				let name = match event.name {
					Some(name) => name,
					None => {
						warn!("Skipping inotify event with no attached file name");
						continue;
					}
				};

				let event_path: PathBuf = name.to_owned().into();
				let file_name = event_path
					.file_name()
					.ok_or(anyhow!("{} does not have a filename", event_path.display()))?;
				let key = format!("{}/alpine-rust/x86_64/{}", config.alpine.version, file_name.to_string_lossy());
				if let Err(err) = repo::upload(&event_path, &key).await {
					error!("Error uploading {}: {}", event_path.display(), err);
					res = Err(err);
				}
			}
		}

		res
	}

	async fn destroy(self) -> anyhow::Result<()> {
		Ok(())
	}
}
