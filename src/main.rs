#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate log;

use askama::Template;
use bollard::{Docker, API_DEFAULT_VERSION};
use env::current_dir;
use futures_util::{stream, FutureExt, StreamExt};
use serde::Deserialize;
use std::{
	borrow::Cow,
	env,
	path::Path,
	process::{exit, Command}
};
use tempfile::{tempdir, TempDir};
use tokio::{fs::File, io::AsyncReadExt};
use upcloud::UPCLOUD_CORES;

mod docker;
mod metadata;
mod package;
mod upcloud;

#[derive(Deserialize, Template)]
#[template(path = "APKBUILD.tpl", escape = "none")]
struct APKBUILD {
	rustminor: u32,
	rustpatch: u32,
	pkgrel: u32,
	llvmver: u32,
	bootver: String,
	bootsys: bool,
	sysver: Option<String>,
	python: Option<String>,
	sha512sums: String
}

#[derive(Deserialize, Template)]
#[template(path = "index.html")]
struct Config {
	alpine: String,
	pubkey: String,
	privkey: String,
	versions: Vec<APKBUILD>
}

#[derive(Template)]
#[template(path = "Dockerfile.tpl", escape = "none")]
struct Dockerfile<'a> {
	alpine: &'a str,
	pubkey: &'a str,
	privkey: &'a str,
	sysver: Option<&'a str>,
	jobs: u16
}

impl Config {
	fn dockerfile<'a>(&'a self, ver: &'a APKBUILD, jobs: u16) -> Dockerfile<'a> {
		Dockerfile {
			alpine: &self.alpine,
			pubkey: &self.pubkey,
			privkey: &self.privkey,
			sysver: ver.sysver.as_deref(),
			jobs
		}
	}
}

fn tar_header(path: &str, len: usize) -> tar::Header {
	let mut header = tar::Header::new_old();
	header.set_path(path).unwrap();
	header.set_mode(0o644);
	header.set_uid(0);
	header.set_gid(0);
	header.set_size(len as u64);
	header.set_cksum();
	header
}

lazy_static! {
	static ref CI: bool = env::var("CI").is_ok();
	static ref GITHUB_TOKEN: String = env::var("GITHUB_TOKEN").expect("GITHUB_TOKEN must be set");
}

const GIT_NAME: &str = "drone.msrd0.eu [bot]";
const GIT_EMAIL: &str = "noreply@drone.msrd0.eu";

fn run_git(dir: &Path, args: &[&str]) -> bool {
	let status = Command::new("git")
		.args(args)
		.current_dir(dir)
		.env("GIT_AUTHOR_NAME", GIT_NAME)
		.env("GIT_AUTHOR_EMAIL", GIT_EMAIL)
		.env("GIT_COMMITTER_NAME", GIT_NAME)
		.env("GIT_COMMITTER_EMAIL", GIT_EMAIL)
		.status()
		.expect("Failed to run git");
	status.success()
}

fn git_clone() -> TempDir {
	info!("Cloning git repository");
	let repodir = tempdir().expect("Failed to create tempdir");
	let repourl = format!(
		"https://drone-msrd0-eu:{}@github.com/msrd0/alpine-rust",
		GITHUB_TOKEN.as_str()
	);
	if !run_git("/".as_ref(), &[
		"clone",
		"--branch=gh-pages",
		"--depth=1",
		&repourl,
		&repodir.path().to_string_lossy()
	]) {
		error!("Failed to clone git repo");
		exit(1);
	}
	if !run_git(repodir.path(), &["config", "user.name", GIT_NAME]) {
		error!("Failed to set git user.name config");
		exit(1);
	}
	if !run_git(repodir.path(), &["config", "user.email", GIT_EMAIL]) {
		error!("Failed to set git user.email config");
		exit(1);
	}

	repodir
}

#[tokio::main]
async fn main() {
	pretty_env_logger::init_timed();

	info!("Reading versions.toml");
	let mut config_file = File::open("versions.toml").await.expect("Unable to find versions.toml");
	let mut config_buf = Vec::<u8>::new();
	config_file
		.read_to_end(&mut config_buf)
		.await
		.expect("Unable to read versions.toml");
	drop(config_file);
	let config: Config = toml::from_slice(&config_buf).expect("Invalid syntax in versions.toml");

	// clone the git repo
	let (_repotmpdir, repodir) = if *CI {
		let dir = git_clone();
		let path = dir.path().to_owned();
		(Some(dir), path)
	} else {
		let dir = current_dir().unwrap().join("repo");
		(None, dir)
	};

	// update the metadata
	metadata::update(&config, &repodir).await;

	// search for versions that need to be updated
	let pkg_updates = stream::iter(config.versions.iter())
		.filter(|ver| package::up_to_date(&repodir, &config, ver).map(|up_to_date| !up_to_date))
		.collect::<Vec<_>>()
		.await;

	// if everything is up to date, simply exit
	if pkg_updates.is_empty() {
		info!("Everything is up to date");
		return;
	}

	// upcloud for CI, local for non-CI
	let (mut server, docker) = if *CI {
		// launch an upcloud server
		let server = upcloud::launch_server(&config, &repodir)
			.await
			.expect("Failed to launch UpCloud server");

		let docker_addr = format!("tcp://{}:8443/", server.domain);
		info!("Connecting to {}", docker_addr);
		let docker = Docker::connect_with_ssl(
			&docker_addr,
			&server.keys.client_key_path(),
			&server.keys.client_cert_path(),
			&server.keys.ca_path(),
			120,
			API_DEFAULT_VERSION
		);
		let docker = match docker {
			Ok(docker) => docker,
			Err(err) => {
				error!("Failed to connect to docker: {}", err);
				server.destroy().await.expect("Failed to destroy the server");
				exit(1);
			}
		};
		info!("Connected to {}", docker_addr);

		(Some(server), docker)
	} else {
		info!("Connecting to local docker daemon");
		let docker = Docker::connect_with_local_defaults().expect("Failed to connect to docker");
		(None, docker)
	};

	// update packages
	for ver in pkg_updates {
		// build the package
		{
			let (repodir, jobs) = match &server {
				Some(_) => (Cow::Borrowed("/var/lib/alpine-rust"), UPCLOUD_CORES),
				None => (
					current_dir().unwrap().join("repo").to_string_lossy().to_string().into(),
					num_cpus::get() as u16
				)
			};
			if let Err(err) = package::build(&repodir, &docker, &config, ver, jobs).await {
				error!("Failed to build package: {}", err);
				if let Some(server) = server {
					server.destroy().await.expect("Failed to destroy the server");
				}
				exit(1);
			}
		}

		// commit the changes
		if let Some(mut server) = server.as_mut() {
			if let Err(err) = upcloud::commit_changes(&config, ver, &repodir, &mut server).await {
				error!("Failed to commit changes: {}", err);
				server.destroy().await.expect("Failed to destroy the server");
				exit(1);
			}
		} else {
			warn!("Not running in CI - No changes commited");
		}
	}

	// remove the server
	if let Some(server) = server {
		server.destroy().await.expect("Failed to destroy the server");
	}
}
