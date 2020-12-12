use crate::{docker::IPv6CIDR, Config};
use bollard::Docker;
use either::Either;
use std::path::Path;

pub mod local;
pub mod upcloud;

#[async_trait]
pub trait Server {
	/// Install any missing dependencies/keys/... on the server.
	async fn install(&self, config: &Config, repodir: &Path) -> anyhow::Result<()>;

	/// Connect to the docker daemon running on the server.
	fn connect_to_docker(&self) -> Result<Docker, bollard::errors::Error>;

	/// Get the directory that contains the repository on the server.
	fn repomount(&self, repodir: &Path) -> String;

	/// Get the amount of (virtual) cores on the server.
	fn cores(&self) -> u16;

	/// Get the IPv6 CIDR of the docker daemon.
	fn cidr_v6(&self) -> IPv6CIDR<String>;

	/// Upload any changes made to the repodir.
	async fn upload_repo_changes(&self, config: &Config, repodir: &Path) -> anyhow::Result<()>;

	/// Destroy the server if it was created previously.
	async fn destroy(self) -> anyhow::Result<()>;
}

#[async_trait]
impl<A, B> Server for Either<A, B>
where
	A: Server + Send + Sync,
	B: Server + Send + Sync
{
	async fn install(&self, config: &Config, repodir: &Path) -> anyhow::Result<()> {
		self.as_ref()
			.either(|a| a.install(config, repodir), |b| b.install(config, repodir))
			.await
	}

	fn connect_to_docker(&self) -> Result<Docker, bollard::errors::Error> {
		self.as_ref().either(A::connect_to_docker, B::connect_to_docker)
	}

	fn repomount(&self, repodir: &Path) -> String {
		self.as_ref().either(|a| a.repomount(repodir), |b| b.repomount(repodir))
	}

	fn cores(&self) -> u16 {
		self.as_ref().either(A::cores, B::cores)
	}

	fn cidr_v6(&self) -> IPv6CIDR<String> {
		self.as_ref().either(A::cidr_v6, B::cidr_v6)
	}

	async fn upload_repo_changes(&self, config: &Config, repodir: &Path) -> anyhow::Result<()> {
		self.as_ref()
			.either(
				|a| a.upload_repo_changes(config, repodir),
				|b| b.upload_repo_changes(config, repodir)
			)
			.await
	}

	async fn destroy(self) -> anyhow::Result<()> {
		self.either(A::destroy, B::destroy).await
	}
}
