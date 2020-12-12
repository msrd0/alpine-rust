use super::Server;
use crate::{
	docker::{local_ipv6_cidr, IPv6CIDR},
	repo, Config
};
use anyhow::anyhow;
use bollard::Docker;
use std::{ffi::OsString, os::unix::ffi::OsStrExt, path::Path};
use tokio::{
	fs::{self, File},
	io::{self, AsyncReadExt},
	stream::StreamExt
};

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
		let repomount = std::fs::canonicalize(repodir).unwrap();
		repomount.to_str().unwrap().to_owned()
	}

	fn cores(&self) -> u16 {
		num_cpus::get() as u16
	}

	fn cidr_v6(&self) -> IPv6CIDR<String> {
		local_ipv6_cidr().expect("Failed to parse /etc/docker/daemon.json - Is your docker daemon IPv6-enabled?")
	}

	async fn upload_repo_changes(&self, config: &Config, repodir: &Path) -> anyhow::Result<()> {
		let dir = repodir.join(format!("{}/alpine-rust/x86_64", config.alpine));

		let mut res: anyhow::Result<()> = Ok(());
		let mut entries = fs::read_dir(&dir).await?;
		while let Some(file) = entries.next().await {
			let path = file?.path();
			info!("Inspecting {}", path.display());
			let parent = path.parent().ok_or(anyhow!("{} does not have a parent", path.display()))?;
			let file_name = path
				.file_name()
				.ok_or(anyhow!("{} does not have a filename", path.display()))?;
			if file_name.as_bytes()[0] == '.' as u8 {
				continue;
			}

			let mut file = File::open(&path).await?;
			let mut hash = md5::Context::new();
			let mut buf = [0u8; 8192];
			loop {
				let len = file.read(&mut buf).await?;
				if len == 0 {
					break;
				}
				hash.consume(&buf[..len]);
			}
			let hash = format!("\"{:x}\"", hash.compute());

			let mut etag_name = OsString::from(".");
			etag_name.push(file_name);
			etag_name.push(".etag");
			let etag_path = parent.join(&etag_name);

			let etag = match File::open(&etag_path).await {
				Ok(mut etag_file) => {
					let mut etag = String::new();
					etag_file.read_to_string(&mut etag).await?;
					Some(etag)
				},
				Err(err) if err.kind() == io::ErrorKind::NotFound => None,
				Err(err) => return Err(err.into())
			};

			if etag != Some(hash) {
				let key = format!("{}/alpine-rust/x86_64/{}", config.alpine, file_name.to_string_lossy());
				if let Err(err) = repo::upload(&path, &key).await {
					error!("Error uploading {}: {}", path.display(), err);
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
