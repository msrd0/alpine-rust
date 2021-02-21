use super::docker_run_abuild;
use crate::{
	config::{Config, PackageCrate, PackageLLVM},
	docker::{build_image, docker_push, tar_header}
};
use askama::Template;
use bollard::{
	image::{BuildImageOptions, TagImageOptions},
	Docker
};
use std::{fmt::Debug, io::Cursor, path::Path};
use tokio::{fs::File, io::AsyncReadExt};

pub trait Package: Debug + Send + Sync {
	fn pkgname(&self) -> String;
	fn pkgver(&self) -> &str;
	fn pkgrel(&self) -> u32;

	fn render_apkbuild(&self, config: &Config) -> Result<String, askama::Error>;
	fn render_dockerfile(&self, config: &Config) -> Option<Result<String, askama::Error>>;
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
	fn render_dockerfile(&self, _config: &Config) -> Option<Result<String, askama::Error>> {
		None
	}
}

impl Package for PackageCrate {
	fn pkgname(&self) -> String {
		self.crate_name.replace("_", "-").to_lowercase()
	}
	fn pkgver(&self) -> &str {
		&self.version
	}
	fn pkgrel(&self) -> u32 {
		self.pkgrel
	}

	fn render_apkbuild(&self, config: &Config) -> Result<String, askama::Error> {
		config.package_crate_apkbuild(&self).render()
	}
	fn render_dockerfile(&self, config: &Config) -> Option<Result<String, askama::Error>> {
		Some(config.package_crate_dockerfile(&self).render())
	}
}

pub async fn up_to_date(repodir: &Path, config: &Config, pkg: &dyn Package) -> bool {
	super::up_to_date(repodir, config, &pkg.pkgname(), pkg.pkgver(), pkg.pkgrel()).await
}

async fn build_tar(
	apkbuild: Option<&str>,
	dockerfile: &str,
	pubkey: &str,
	privkey: Option<&str>
) -> anyhow::Result<Vec<u8>> {
	let mut tar_buf: Vec<u8> = Vec::new();
	let mut tar = tar::Builder::new(&mut tar_buf);

	// write the APKBUILD file
	if let Some(apkbuild) = apkbuild {
		let bytes = apkbuild.as_bytes();
		let header = tar_header("APKBUILD", bytes.len());
		tar.append(&header, Cursor::new(bytes))?;
	}

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
	if let Some(privkey) = privkey {
		let mut file = File::open(privkey).await?;
		let mut bytes = Vec::<u8>::new();
		file.read_to_end(&mut bytes).await?;
		let header = tar_header(privkey, bytes.len());
		tar.append(&header, Cursor::new(bytes))?;
	}

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
	let tar = build_tar(
		Some(&apkbuild),
		&dockerfile,
		&config.alpine.pubkey,
		Some(&config.alpine.privkey)
	)
	.await?;

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

pub async fn build_and_upload_docker(
	docker: &Docker,
	config: &Config,
	pkg: &dyn Package,
	upload_docker: bool
) -> anyhow::Result<()> {
	let dockerfile = match pkg.render_dockerfile(config) {
		Some(dockerfile) => dockerfile?,
		None => {
			debug!("No Dockerfile specified for package {:?}", pkg);
			return Ok(());
		}
	};

	let pkgname = pkg.pkgname();
	let pkgver = pkg.pkgver();
	let image = format!("ghcr.io/msrd0/alpine-{}", pkgname);
	let tag = format!("{}:{}", image, pkgver);
	info!("Building Docker image {}", tag);
	let tar = build_tar(None, &dockerfile, &config.alpine.pubkey, None).await?;
	build_image(
		docker,
		BuildImageOptions {
			t: tag.clone(),
			pull: true,
			nocache: true,
			..Default::default()
		},
		tar
	)
	.await?;
	docker
		.tag_image(
			&tag,
			Some(TagImageOptions {
				repo: image.as_str(),
				tag: "latest"
			})
		)
		.await?;
	if upload_docker {
		docker_push(docker, &tag).await?;
		docker_push(docker, &image).await?;
	}

	Ok(())
}
