use anyhow::bail;
use bollard::{container::LogsOptions, image::BuildImageOptions, Docker};
use futures_util::StreamExt;
use serde::Serialize;
use std::{hash::Hash, time::Duration};
use tokio::time::delay_for;

mod caddy;
pub use caddy::*;
mod cidr_v6;
pub use cidr_v6::*;
mod keys;
pub use keys::*;

pub fn tar_header(path: &str, len: usize) -> tar::Header {
	let mut header = tar::Header::new_old();
	header.set_path(path).unwrap();
	header.set_mode(0o644);
	header.set_uid(0);
	header.set_gid(0);
	header.set_size(len as u64);
	header.set_cksum();
	header
}

pub async fn build_image<T>(docker: &Docker, options: BuildImageOptions<T>, tar: Vec<u8>) -> anyhow::Result<()>
where
	T: Eq + Hash + Into<String> + Serialize
{
	let mut img_stream = docker.build_image(options, None, Some(tar.into()));
	while let Some(status) = img_stream.next().await {
		let status = status.expect("Failed to build image");
		if let Some(log) = status.stream {
			print!("{}", log);
		}
		if let Some(err) = status.error {
			print!("{}", err);
			bail!("Failed to build docker image");
		}
	}

	Ok(())
}

pub async fn run_container_to_completion(docker: &Docker, container_id: &str) -> anyhow::Result<()> {
	// start the container
	docker.start_container::<String>(container_id, None).await?;
	info!("Started container {}", container_id);

	// attach to the container logs
	let mut logs = docker.logs::<String>(
		container_id,
		Some(LogsOptions {
			follow: true,
			stdout: true,
			stderr: true,
			timestamps: true,
			..Default::default()
		})
	);
	while let Some(log) = logs.next().await {
		let log = log?;
		print!("{}", log);
	}
	info!("Log stream finished");

	// ensure that the container has stopped
	loop {
		delay_for(Duration::new(2, 0)).await;
		let state = docker.inspect_container(container_id, None).await?.state;
		let state = match state {
			Some(state) => state,
			None => {
				warn!("Container {} has unknown state", container_id);
				continue;
			}
		};
		if state.running == Some(true) {
			info!("Container {} is still running", container_id);
			continue;
		}
		let exit_code = match state.exit_code {
			Some(exit_code) => exit_code,
			None => {
				warn!("Unable to get exit code for container {}, assuming 0", container_id);
				break;
			}
		};
		if exit_code != 0 {
			return Err(anyhow::Error::msg(format!(
				"Container {} finished with exit code {}",
				container_id, exit_code
			)));
		}
		break;
	}
	info!("Container {} has stopped", container_id);
	Ok(())
}
