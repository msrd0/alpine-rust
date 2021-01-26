use super::docker_run_abuild;
use crate::{
	config::{Config, PackageLLVM},
	docker::{build_image, tar_header}
};
use askama::Template;
use bollard::{image::BuildImageOptions, Docker};
use std::{io::Cursor, path::Path};
use tokio::{fs::File, io::AsyncReadExt};

pub trait Package: Send + Sync {
	fn pkgname(&self) -> String;
	fn pkgver(&self) -> &str;
	fn pkgrel(&self) -> u32;

	fn render_apkbuild(&self, config: &Config) -> Result<String, askama::Error>;
}

impl Package for PackageLLVM {
	fn pkgname(&self) -> String {
		format!("llvm{}", self.pkgver.splitn(2, '.').next().unwrap())
	}
	fn pkgver(&self) -> &str {
		&self.pkgver
	}
	fn pkgrel(&self) -> u32 {
		self.pkgrel
	}

	fn render_apkbuild(&self, config: &Config) -> Result<String, askama::Error> {
		config.package_llvm_apkbuild(&self).render()
	}
}

pub async fn up_to_date(repodir: &Path, config: &Config, pkg: &dyn Package) -> bool {
	super::up_to_date(repodir, config, &pkg.pkgname(), pkg.pkgver(), pkg.pkgrel()).await
}

async fn build_tar(apkbuild: &str, dockerfile: &str, pubkey: &str, privkey: &str) -> anyhow::Result<Vec<u8>> {
	let mut tar_buf: Vec<u8> = Vec::new();
	let mut tar = tar::Builder::new(&mut tar_buf);

	// write the APKBUILD file
	let bytes = apkbuild.as_bytes();
	let header = tar_header("APKBUILD", bytes.len());
	tar.append(&header, Cursor::new(bytes))?;

	// write the Dockerfile file
	let bytes = dockerfile.as_bytes();
	let header = tar_header("Dockerfile", bytes.len());
	tar.append(&header, Cursor::new(bytes))?;

	// copy the public key
	let mut file = File::open(pubkey).await?;
	let mut bytes = Vec::<u8>::new();
	file.read_to_end(&mut bytes).await?;
	let header = tar_header(pubkey, bytes.len());
	tar.append(&header, Cursor::new(bytes))?;

	// copy the private key
	let mut file = File::open(privkey).await?;
	let mut bytes = Vec::<u8>::new();
	file.read_to_end(&mut bytes).await?;
	let header = tar_header(privkey, bytes.len());
	tar.append(&header, Cursor::new(bytes))?;

	// finish the tar archive
	tar.finish()?;
	drop(tar);
	Ok(tar_buf)
}

async fn docker_build_abuild(
	docker: &Docker,
	tag: &str,
	config: &Config,
	pkg: &dyn Package,
	jobs: u16
) -> anyhow::Result<()> {
	info!("Building Docker image {}", tag);

	// create the context tar for docker build
	let apkbuild: String = pkg.render_apkbuild(config)?;
	let dockerfile = config.packages_dockerfile_abuild(jobs).render()?;
	let tar = build_tar(&apkbuild, &dockerfile, &config.alpine.pubkey, &config.alpine.privkey).await?;

	// build the docker image
	build_image(
		docker,
		BuildImageOptions {
			t: tag.to_owned(),
			pull: true,
			..Default::default()
		},
		tar
	)
	.await?;
	info!("Built Docker image {}", tag);
	Ok(())
}

pub async fn build_package(
	repomount: &str,
	docker: &Docker,
	config: &Config,
	pkg: &dyn Package,
	jobs: u16
) -> anyhow::Result<()> {
	info!("Building Package {}", pkg.pkgname());

	let img = format!("alpine-rust-builder-{}", pkg.pkgname());
	docker_build_abuild(docker, &img, config, pkg, jobs).await?;
	docker_run_abuild(docker, &img, repomount).await?;

	Ok(())
}
