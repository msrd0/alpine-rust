use super::{tar_header, Config, APKBUILD};
use askama::Template;
use bollard::{
	container::{self, LogsOptions},
	image::BuildImageOptions,
	models::{HostConfig, Mount, MountTypeEnum},
	Docker
};
use futures_util::StreamExt;
use std::{collections::HashMap, fs::File, io::Cursor, path::Path, process::exit};
use tokio::{
	fs, io,
	time::{delay_for, Duration}
};

pub(super) async fn up_to_date(repodir: &Path, config: &Config, ver: &APKBUILD) -> bool {
	let path = format!(
		"{alpine}/alpine-rust/x86_64/rust-1.{minor}-1.{minor}.{patch}-r{pkgrel}.apk",
		alpine = config.alpine,
		minor = ver.rustminor,
		patch = ver.rustpatch,
		pkgrel = ver.pkgrel
	);
	match fs::metadata(repodir.join(path)).await {
		Ok(_) => true,                                              // file exists
		Err(err) if err.kind() == io::ErrorKind::NotFound => false, // not found
		Err(err) => {
			// other i/o error
			error!("Unable to check if package was up to date: {}", err);
			exit(1);
		}
	}
}

pub(super) async fn build(docker: &Docker, config: &Config, ver: &APKBUILD) {
	info!("Building Rust 1.{}.{}", ver.rustminor, ver.rustpatch);

	let mut tar_buf: Vec<u8> = Vec::new();
	let mut tar = tar::Builder::new(&mut tar_buf);

	// write the APKBUILD file
	{
		let apkbuild = ver.render().expect("Failed to render APKBUILD");
		let bytes = apkbuild.as_bytes();
		let header = tar_header("APKBUILD", bytes.len());
		tar.append(&header, Cursor::new(bytes)).expect("Failed to write APKBUILD");
	}

	// write the Dockerfile file
	{
		let dockerfile = config.dockerfile().render().expect("Failed to render Dockerfile");
		let bytes = dockerfile.as_bytes();
		let header = tar_header("Dockerfile", bytes.len());
		tar.append(&header, Cursor::new(bytes)).expect("Failed to write Dockerfile");
	}

	// copy the public and private keys
	for key in &[&config.privkey, &config.pubkey] {
		// TODO sync i/o in async context
		let mut file = File::open(key).expect("Failed to open abuild key");
		tar.append_file(key, &mut file).expect("Failed to write abuild key");
	}

	// finish the tar archive
	tar.finish().expect("Failed to finish tar archive");
	drop(tar);

	// build the docker image
	let img = format!("alpine-rust-builder-1.{}", ver.rustminor);
	info!("Building docker image {}", img);
	let mut img_stream = docker.build_image(
		BuildImageOptions {
			t: img.clone(),
			pull: true,
			..Default::default()
		},
		None,
		Some(tar_buf.into())
	);
	while let Some(status) = img_stream.next().await {
		let status = status.expect("Failed to build image");
		if let Some(log) = status.stream {
			print!("{}", log);
		}
		if let Some(err) = status.error {
			print!("{}", err);
			error!("Failed to build docker image {}", img);
			exit(1);
		}
	}
	info!("Built docker image {}", img);

	// create the container
	let mut volumes: HashMap<String, HashMap<(), ()>> = HashMap::new();
	volumes.insert("/repo".to_owned(), Default::default());
	let mut mounts: Vec<Mount> = Vec::new();
	mounts.push(Mount {
		target: Some("/repo".to_string()),
		source: Some("/var/lib/alpine-rust".to_string()),
		typ: Some(MountTypeEnum::BIND),
		read_only: Some(false),
		..Default::default()
	});
	let container = docker
		.create_container::<String, String>(None, container::Config {
			attach_stdout: Some(true),
			attach_stderr: Some(true),
			image: Some(img.clone()),
			volumes: Some(volumes),
			host_config: Some(HostConfig {
				mounts: Some(mounts),
				..Default::default()
			}),
			..Default::default()
		})
		.await
		.expect("Failed to create container");
	info!("Created container {}", container.id);

	// start the container
	docker
		.start_container::<String>(&container.id, None)
		.await
		.expect("Failed to start container");
	info!("Started container {}", container.id);

	// attach to the container logs
	let mut logs = docker.logs::<String>(
		&container.id,
		Some(LogsOptions {
			follow: true,
			stdout: true,
			stderr: true,
			timestamps: true,
			..Default::default()
		})
	);
	while let Some(log) = logs.next().await {
		let log = log.expect("Failed to attach to container");
		print!("{}", log);
	}
	info!("Log stream finished");

	// ensure that the container has stopped
	loop {
		delay_for(Duration::new(2, 0)).await;
		let state = docker
			.inspect_container(&container.id, None)
			.await
			.expect("Failed to inspect container")
			.state;
		let state = match state {
			Some(state) => state,
			None => {
				warn!("Container {} has unknown state", container.id);
				continue;
			}
		};
		if state.running == Some(true) {
			info!("Container {} is still running", container.id);
			continue;
		}
		let exit_code = match state.exit_code {
			Some(exit_code) => exit_code,
			None => {
				warn!("Unable to get exit code for container {}, assuming 0", container.id);
				break;
			}
		};
		if exit_code != 0 {
			error!("Container {} finished with exit code {}", container.id, exit_code);
			exit(1);
		}
		break;
	}
	info!("Container {} has stopped", container.id);
}
