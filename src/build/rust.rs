use crate::{
	docker::{tar_header, IPv6CIDR},
	Config, Version, GITHUB_TOKEN
};
use anyhow::Context;
use askama::Template;
use bollard::{
	auth::DockerCredentials,
	container::{self, LogsOptions},
	image::BuildImageOptions,
	models::{HostConfig, Mount, MountTypeEnum},
	Docker
};
use futures_util::StreamExt;
use std::{collections::HashMap, env, io::Cursor, path::Path, process::exit};
use tokio::{
	fs::{self, File},
	io::{self, AsyncReadExt},
	time::{delay_for, Duration}
};

pub async fn up_to_date(repodir: &Path, config: &Config, ver: &Version) -> bool {
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
			error!("Unable to check if package was up to date: {}", err);
			// exiting is fine because no upcloud server was provisioned yet
			exit(1);
		}
	}
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

	if let Some(privkey) = privkey {
		// copy the public key
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

async fn docker_build_abuild(docker: &Docker, tag: &str, config: &Config, ver: &Version, jobs: u16) -> anyhow::Result<()> {
	info!("Building Docker image {}", tag);

	// create the context tar for docker build
	let apkbuild: String = ver.apkbuild().render()?;
	let dockerfile = config.rust_dockerfile_abuild(ver, jobs).render()?;
	let tar = build_tar(Some(&apkbuild), &dockerfile, &config.pubkey, Some(&config.privkey)).await?;

	// build the docker image
	let mut img_stream = docker.build_image(
		BuildImageOptions {
			t: tag.to_owned(),
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
			return Err(anyhow::Error::msg(format!("Failed to build docker image {}", tag)));
		}
	}
	info!("Built Docker image {}", tag);
	Ok(())
}

async fn run_container_to_completion(docker: &Docker, container_id: &str) -> anyhow::Result<()> {
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

async fn docker_run_test(docker: &Docker, img: &str, cmd: &str) -> anyhow::Result<()> {
	info!("Creating container for {}", img);

	// create the container
	let container = docker
		.create_container::<String, &str>(None, container::Config {
			cmd: Some(vec!["/bin/ash", "-exo", "pipefail", "-c", cmd]),
			attach_stdout: Some(true),
			attach_stderr: Some(true),
			image: Some(img),
			..Default::default()
		})
		.await?;
	info!("Created container {}", container.id);

	run_container_to_completion(docker, &container.id).await
}

async fn docker_build_dockerfile(docker: &Docker, tag: &str, dockerfile: &str, config: &Config) -> anyhow::Result<()> {
	info!("Building Docker image {}", tag);

	// create the context tar for docker build
	let tar = build_tar(None, dockerfile, &config.pubkey, None).await?;

	// build the docker image
	let mut img_stream = docker.build_image(
		BuildImageOptions {
			t: tag,
			pull: true,
			nocache: true,
			..Default::default()
		},
		None,
		Some(tar.into())
	);
	while let Some(status) = img_stream.next().await {
		let status = status?;
		if let Some(log) = status.stream {
			print!("{}", log);
		}
		if let Some(err) = status.error {
			print!("{}", err);
			return Err(anyhow::Error::msg(format!("Failed to build docker image {}", tag)));
		}
	}
	info!("Built Docker image {}", tag);
	Ok(())
}

async fn docker_push(docker: &Docker, tag: &str) -> anyhow::Result<()> {
	if env::var("CI").is_err() {
		info!("Running outside CI - not pushing {}", tag);
		return Ok(());
	}

	info!("Pushing Docker image {}", tag);
	let mut push_stream = docker.push_image::<String>(
		&tag,
		None,
		Some(DockerCredentials {
			username: Some("drone-msrd0-eu".to_owned()),
			password: Some(GITHUB_TOKEN.clone()),
			..Default::default()
		})
	);

	while let Some(info) = push_stream.next().await {
		let info = info?;
		if let Some(err) = info.error {
			println!("{}", err);
			return Err(anyhow::Error::msg(format!("Failed to push docker image {}", tag)));
		}
	}
	info!("Pushed Docker image {}", tag);
	Ok(())
}

pub async fn build_package(
	repomount: &str,
	docker: &Docker,
	config: &Config,
	ver: &Version,
	jobs: u16
) -> anyhow::Result<()> {
	info!("Building Rust 1.{}.{}", ver.rustminor, ver.rustpatch);

	let img = format!("alpine-rust-builder-1.{}", ver.rustminor);
	docker_build_abuild(docker, &img, config, ver, jobs).await?;
	docker_run_abuild(docker, &img, repomount).await?;

	Ok(())
}

pub async fn test_package(
	docker: &Docker,
	cidr_v6: &IPv6CIDR<String>,
	config: &Config,
	ver: &Version
) -> anyhow::Result<()> {
	let tag = format!("alpine-rust-test-1.{}", ver.rustminor);

	let dockerfile = config.rust_dockerfile_test(cidr_v6).render()?;
	docker_build_dockerfile(docker, &tag, &dockerfile, config).await?;

	// TODO is this the best way to get all packages?
	let packages = [
		"cargo-{}",
		"cargo-{}-bash-completions",
		"cargo-{}-doc",
		"cargo-{}-zsh-completion",
		"clippy-{}",
		"rust-{}",
		"rust-{}-analysis",
		"rust-{}-dbg",
		"rust-{}-doc",
		"rust-{}-gdb",
		"rust-{}-lldb",
		"rust-{}-src",
		"rust-{}-stdlib",
		"rustfmt-{}"
	]
	.iter()
	.map(|tpl| tpl.replace("{}", &format!("1.{}", ver.rustminor)))
	.collect::<Vec<_>>();

	// first of all, let's test that every package can be installed on its own
	for pkg in &packages {
		info!("Testing installation of {}", pkg);
		let cmd = format!("apk add {}", pkg);
		docker_run_test(docker, &tag, &cmd)
			.await
			.context(format!("Failed to install {}", pkg))?;
	}

	// next, let's test they can all be installed alongside each other
	info!("Testing installation of all packages for 1.{}", ver.rustminor);
	let cmd = format!("apk add {}", packages.join(" "));
	docker_run_test(docker, &tag, &cmd)
		.await
		.context(format!("Failed to install all packages for 1.{}", ver.rustminor))?;

	// and finally, test a small rust program that uses derive macros
	info!("Testing a small rust program");
	let cargo = indoc!(
		r#"
		[package]
		name = "alpine-rust-test"
		version = "0.0.0"
		authors = ["Tux", "The Rust Crab"]
		edition = "2018"
		publish = false
		
		[dependencies]
		serde = { version = "1.0", features = ["derive"] }
		serde_json = "1.0"
	"#
	);
	let main = indoc!(
		r#"
		use serde::Serialize;
		use serde_json::{json, to_value};
		
		#[derive(Serialize)]
		struct Foo {
			foo: u8
		}
		
		fn main() {
			let expected = json!({ "foo": 42 });
			let actual = to_value(&Foo { foo: 42 }).unwrap();
			assert_eq!(actual, expected);
		}
	"#
	);
	let cmd = vec![
		format!(
			"apk add cargo-1.{rustminor} clang lld rust-1.{rustminor}",
			rustminor = ver.rustminor
		),
		"mkdir -p /tmp/alpine-rust-test/src".to_owned(),
		"cd /tmp/alpine-rust-test".to_owned(),
		format!("echo {} | base64 -d >Cargo.toml", base64::encode(cargo)),
		format!("echo {} | base64 -d >src/main.rs", base64::encode(main)),
		"RUSTFLAGS=\"-C linker=clang -C link-arg=-fuse-ld=lld\" cargo run".to_owned(),
	]
	.join(" && ");
	docker_run_test(docker, &tag, &cmd)
		.await
		.context("Failed to run simple rust program")?;

	Ok(())
}

pub async fn build_and_upload_docker(
	docker: &Docker,
	config: &Config,
	ver: &Version,
	upload_docker: bool
) -> anyhow::Result<()> {
	let img = format!("ghcr.io/msrd0/alpine-rust:1.{}-minimal", ver.rustminor);
	let dockerfile = config.rust_dockerfile_minimal(Some(ver)).render()?;
	docker_build_dockerfile(docker, &img, &dockerfile, config).await?;
	if upload_docker {
		docker_push(docker, &img).await?;
	}

	let img = format!("ghcr.io/msrd0/alpine-rust:1.{}", ver.rustminor);
	let dockerfile = config.rust_dockerfile_default(Some(ver)).render()?;
	docker_build_dockerfile(docker, &img, &dockerfile, config).await?;
	if upload_docker {
		docker_push(docker, &img).await?;
	}

	Ok(())
}
