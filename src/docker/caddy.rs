use super::build_tar;
use crate::Config;
use anyhow::Context;
use askama::Template;
use bollard::{
	container,
	container::StopContainerOptions,
	image::BuildImageOptions,
	models::{HostConfig, Mount, MountTypeEnum, PortBinding},
	Docker
};
use futures_util::StreamExt;
use std::collections::HashMap;

const CADDY_IMG: &str = "alpine-rust-caddy";

pub async fn build_caddy(docker: &Docker, config: &Config) -> anyhow::Result<()> {
	info!("Building Docker image {}", CADDY_IMG);

	// create the context tar for docker build
	let caddyfile: String = config.caddyfile().render()?;
	let dockerfile = config.caddy_dockerfile().render()?;
	let tar = build_tar(&caddyfile, &dockerfile).await?;

	// build the docker image
	let mut img_stream = docker.build_image(
		BuildImageOptions {
			t: CADDY_IMG,
			pull: true,
			..Default::default()
		},
		None,
		Some(tar.into())
	);
	while let Some(status) = img_stream.next().await {
		let status = status.expect("Failed to build image");
		if let Some(log) = status.stream {
			print!("{}", log);
		}
		if let Some(err) = status.error {
			print!("{}", err);
			return Err(anyhow::Error::msg(format!("Failed to build docker image {}", CADDY_IMG)));
		}
	}
	info!("Built Docker image {}", CADDY_IMG);
	Ok(())
}

pub struct CaddyContainer {
	container_id: String
}

pub async fn start_caddy(docker: &Docker, repomount: &str) -> anyhow::Result<CaddyContainer> {
	info!("Creating caddy container");

	// port config
	let mut ports: HashMap<&str, HashMap<(), ()>> = HashMap::new();
	ports.insert("2015/tcp", Default::default());
	let mut port_bindings: HashMap<String, Option<Vec<PortBinding>>> = HashMap::new();
	port_bindings.insert(
		"2015/tcp".to_owned(),
		Some(vec![PortBinding {
			host_ip: None,
			host_port: Some("2015".to_owned())
		}])
	);

	// volume config
	let mut volumes: HashMap<&str, HashMap<(), ()>> = HashMap::new();
	volumes.insert("/repo", Default::default());
	let mut mounts: Vec<Mount> = Vec::new();
	mounts.push(Mount {
		target: Some("/repo".to_owned()),
		source: Some(repomount.to_owned()),
		typ: Some(MountTypeEnum::BIND),
		read_only: Some(true),
		..Default::default()
	});

	// create the container
	let container = docker
		.create_container::<String, &str>(None, container::Config {
			attach_stdout: Some(false),
			attach_stderr: Some(false),
			image: Some(CADDY_IMG),
			volumes: Some(volumes),
			exposed_ports: Some(ports),
			host_config: Some(HostConfig {
				mounts: Some(mounts),
				port_bindings: Some(port_bindings),
				..Default::default()
			}),
			..Default::default()
		})
		.await
		.context("Unable to create caddy container")?;
	info!("Created container {}", container.id);

	// start the container
	docker.start_container::<String>(&container.id, None).await?;
	info!("Started container {}", container.id);

	Ok(CaddyContainer {
		container_id: container.id
	})
}

impl CaddyContainer {
	pub async fn stop(self, docker: &Docker) -> anyhow::Result<()> {
		info!("Stopping caddy container {}", self.container_id);

		docker
			.stop_container(&self.container_id, Some(StopContainerOptions { t: 5 }))
			.await?;

		Ok(())
	}
}