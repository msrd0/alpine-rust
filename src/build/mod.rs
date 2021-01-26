use crate::{docker::run_container_to_completion, Config};
use bollard::{
	container,
	models::{HostConfig, Mount, MountTypeEnum},
	Docker
};
use std::{collections::HashMap, io, path::Path, process::exit};
use tokio::fs;

pub mod packages;
pub mod rust;

async fn up_to_date(repodir: &Path, config: &Config, pkgname: &str, pkgver: &str, pkgrel: u32) -> bool {
	let path = format!(
		"{}/alpine-rust/x86_64/{}-{}-r{}.apk",
		config.alpine.version, pkgname, pkgver, pkgrel
	);
	info!("Checking if {} is up to date ...", path);
	match fs::metadata(repodir.join(path)).await {
		Ok(_) => true,                                              // file exists
		Err(err) if err.kind() == io::ErrorKind::NotFound => false, // not found
		Err(err) => {
			error!("Unable to check if package was up to date: {}", err);
			// exiting is fine because no upcloud server was provisioned yet
			exit(1);
		}
	}
}

async fn docker_run_abuild(docker: &Docker, img: &str, repomount: &str) -> anyhow::Result<()> {
	info!("Creating container for {}", img);

	// create the container
	let mut volumes: HashMap<&str, HashMap<(), ()>> = HashMap::new();
	volumes.insert("/repo", Default::default());
	let mut mounts: Vec<Mount> = Vec::new();
	mounts.push(Mount {
		target: Some("/repo".to_string()),
		source: Some(repomount.to_string()),
		typ: Some(MountTypeEnum::BIND),
		read_only: Some(false),
		..Default::default()
	});
	let container = docker
		.create_container::<String, &str>(None, container::Config {
			attach_stdout: Some(true),
			attach_stderr: Some(true),
			image: Some(img),
			volumes: Some(volumes),
			host_config: Some(HostConfig {
				mounts: Some(mounts),
				..Default::default()
			}),
			..Default::default()
		})
		.await?;
	info!("Created container {}", container.id);

	run_container_to_completion(docker, &container.id).await
}
