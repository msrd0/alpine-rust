use super::docker_run_abuild;
use crate::{
	docker::{build_image, run_container_to_completion, tar_header, IPv6CIDR},
	Config, GITHUB_TOKEN
};
use anyhow::anyhow;
use askama::Template;
use bollard::{auth::DockerCredentials, container, image::BuildImageOptions, Docker};
use futures_util::StreamExt;
use std::{io::Cursor, path::Path, process::exit, sync::Arc};
use tokio::{
	fs::{self, File},
	io::{self, AsyncReadExt},
	task::{spawn, JoinHandle}
};

const DOCKER_IMAGE: &str = "ghcr.io/msrd0/alpine-rust";

pub async fn up_to_date(repodir: &Path, config: &Config, channel: &str) -> bool {
	let rust = &config.rust[channel];
	let pkgname = format!("rust-{}", channel);
	let pkgver = match rust.date.as_ref() {
		Some(date) => format!("{}.{}", rust.pkgver, date.format("%Y%m%d")),
		None => format!("{}", rust.pkgver)
	};
	let path = format!(
		"{}/alpine-rust/x86_64/{}-{}-r{}.apk",
		config.alpine.version, pkgname, pkgver, rust.pkgrel
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

async fn build_tar(
	apkbuild: Option<&str>,
	dockerfile: &str,
	include_compiler_test: bool,
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

	// include the compiler test if desired
	if include_compiler_test {
		const BYTES: &[u8] = include_bytes!(env!("SIMPLE_COMPILER_TEST"));
		let header = tar_header("simple_compiler_test.tar", BYTES.len());
		tar.append(&header, Cursor::new(BYTES))?;
	}

	// copy the public key
	let mut file = File::open(pubkey).await?;
	let mut bytes = Vec::<u8>::new();
	file.read_to_end(&mut bytes).await?;
	let header = tar_header(pubkey, bytes.len());
	tar.append(&header, Cursor::new(bytes))?;

	if let Some(privkey) = privkey {
		// copy the private key
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

async fn docker_build_abuild(docker: &Docker, tag: &str, config: &Config, channel: &str, jobs: u16) -> anyhow::Result<()> {
	info!("Building Docker image {}", tag);

	// create the context tar for docker build
	let apkbuild: String = config.rust_apkbuild(channel).render()?;
	let dockerfile = config.rust_dockerfile_abuild(channel, jobs).render()?;
	let tar = build_tar(
		Some(&apkbuild),
		&dockerfile,
		false,
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

async fn docker_run_test(docker: Arc<Docker>, img: String, cmd: String) -> anyhow::Result<()> {
	info!("Creating container for {}", img);
	let cmd = vec!["/bin/ash", "-exo", "pipefail", "-c", &cmd];
	debug!("Running test command {:?}", cmd);

	// create the container
	let container = docker
		.create_container::<String, _>(None, container::Config {
			cmd: Some(cmd),
			attach_stdout: Some(true),
			attach_stderr: Some(true),
			image: Some(&img),
			..Default::default()
		})
		.await?;
	info!("Created container {}", container.id);

	run_container_to_completion(&docker, &container.id).await
}

async fn docker_build_dockerfile(
	docker: &Docker,
	tag: &str,
	include_compiler_test: bool,
	dockerfile: &str,
	config: &Config
) -> anyhow::Result<()> {
	info!("Building Docker image {}", tag);

	// create the context tar for docker build
	let tar = build_tar(None, dockerfile, include_compiler_test, &config.alpine.pubkey, None).await?;

	// build the docker image
	build_image(
		docker,
		BuildImageOptions {
			t: tag,
			pull: true,
			nocache: true,
			..Default::default()
		},
		tar
	)
	.await?;
	info!("Built Docker image {}", tag);
	Ok(())
}

async fn docker_push(docker: &Docker, tag: &str) -> anyhow::Result<()> {
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
	channel: &str,
	jobs: u16
) -> anyhow::Result<()> {
	info!("Building Rust {}", channel);

	let img = format!("alpine-rust-builder-{}", channel);
	docker_build_abuild(docker, &img, config, channel, jobs).await?;
	docker_run_abuild(docker, &img, repomount).await?;

	Ok(())
}

pub async fn test_package(
	docker: Arc<Docker>,
	cidr_v6: &IPv6CIDR<String>,
	config: &Config,
	channel: &str
) -> anyhow::Result<()> {
	info!("Testing build packages ...");

	let tag = format!("alpine-rust-test-{}", channel);

	let dockerfile = config.rust_dockerfile_test(cidr_v6).render()?;
	docker_build_dockerfile(&docker, &tag, true, &dockerfile, config).await?;

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
	.map(|tpl| tpl.replace("{}", &channel))
	.collect::<Vec<_>>();

	let mut tests: Vec<(JoinHandle<anyhow::Result<()>>, String)> = Vec::new();

	// first of all, let's test that every package can be installed on its own
	for pkg in &packages {
		let cmd = format!("apk add {}", pkg);
		let task = spawn(docker_run_test(docker.clone(), tag.clone(), cmd));
		let err = format!("Failed to install {}", pkg);
		tests.push((task, err));
	}

	// next, let's test they can all be installed alongside each other
	let cmd = format!("apk add {}", packages.join(" "));
	let task = spawn(docker_run_test(docker.clone(), tag.clone(), cmd));
	let err = format!("Failed to install all packages for {}", channel);
	tests.push((task, err));

	// and finally, test a small rust program that uses derive macros
	let cmd = [
		format!("apk add cargo-{channel} rust-{channel}", channel = channel),
		"mkdir -p /tmp/alpine-rust-test/src".to_owned(),
		"cd /tmp/alpine-rust-test".to_owned(),
		"tar xf /opt/simple_compiler_test.tar".to_owned(),
		"cargo test --offline --lib".to_owned()
	]
	.join(" && ");
	let task = spawn(docker_run_test(docker.clone(), tag.clone(), cmd));
	let err = format!("Failed to run simple rust program with {}", channel);
	tests.push((task, err));

	let mut tests_total: u32 = 0;
	let mut tests_failed: u32 = 0;
	for (test, err_msg) in tests {
		tests_total += 1;
		if let Err(err) = test.await? {
			tests_failed += 1;
			error!("{}: {}", err_msg, err);
		}
	}

	if tests_failed == 0 {
		Ok(())
	} else {
		Err(anyhow!("{} out of {} tests failed", tests_failed, tests_total))
	}
}

pub async fn build_and_upload_docker(
	docker: &Docker,
	config: &Config,
	channel: &str,
	upload_docker: bool
) -> anyhow::Result<()> {
	let (tag, minimal_tag) = match channel {
		"stable" => ("latest", "minimal".to_owned()),
		channel => (channel, format!("{}-minimal", channel))
	};

	let img = format!("{}:{}", DOCKER_IMAGE, minimal_tag);
	let dockerfile = config.rust_dockerfile_minimal(channel).render()?;
	docker_build_dockerfile(docker, &img, false, &dockerfile, config).await?;
	if upload_docker {
		docker_push(docker, &img).await?;
	}

	let img = format!("{}:{}", DOCKER_IMAGE, tag);
	let dockerfile = config.rust_dockerfile_default(channel).render()?;
	docker_build_dockerfile(docker, &img, false, &dockerfile, config).await?;
	if upload_docker {
		docker_push(docker, &img).await?;
	}

	Ok(())
}
