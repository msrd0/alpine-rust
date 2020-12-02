#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate log;

use askama::Template;
use bollard::{Docker, API_DEFAULT_VERSION};
use futures_util::{stream, FutureExt, StreamExt};
use serde::Deserialize;
use std::{
	env,
	path::Path,
	process::{exit, Command}
};
use tempfile::{tempdir, TempDir};
use tokio::{fs::File, io::AsyncReadExt};

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
	bootver: String,
	bootsys: bool,
	aportsha: String,
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
	jobs: u16
}

impl Config {
	fn dockerfile(&self) -> Dockerfile<'_> {
		Dockerfile {
			alpine: &self.alpine,
			pubkey: &self.pubkey,
			privkey: &self.privkey,
			jobs: upcloud::UPCLOUD_CORES
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
	static ref GITHUB_TOKEN: String = env::var("GITHUB_TOKEN").expect("GITHUB_TOKEN must be set");
}

fn run_git(dir: &Path, args: &[&str]) -> bool {
	let status = Command::new("git")
		.args(args)
		.current_dir(dir)
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
	if !run_git(repodir.path(), &["config", "user.name", "drone.msrd0.eu [bot]"]) {
		error!("Failed to set git user.name config");
		exit(1);
	}
	if !run_git(repodir.path(), &["config", "user.email", "noreply@drone.msrd0.eu"]) {
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
	let repodir = git_clone();

	// update the metadata
	metadata::update(&config, repodir.path()).await;

	// search for versions that need to be updated
	let pkg_updates = stream::iter(config.versions.iter())
		.filter(|ver| package::up_to_date(repodir.path(), &config, ver).map(|up_to_date| !up_to_date))
		.collect::<Vec<_>>()
		.await;

	// if everything is up to date, simply exit
	if pkg_updates.is_empty() {
		info!("Everything is up to date");
		return;
	}

	// launch an upcloud server
	let mut server = upcloud::launch_server(&config, repodir.path())
		.await
		.expect("Failed to launch UpCloud server");

	// connect to docker
	let docker_addr = format!("tcp://{}:8443/", server.domain);
	info!("Connecting to {}", docker_addr);
	let docker = Docker::connect_with_ssl(
		&docker_addr,
		&server.keys.client_key_path(),
		&server.keys.client_cert_path(),
		&server.keys.ca_path(),
		120,
		API_DEFAULT_VERSION
	)
	.expect("Failed to connect to docker");
	info!("Connected to {}", docker_addr);

	// update packages
	for ver in pkg_updates {
		package::build(&docker, &config, ver).await;
		upcloud::commit_changes(&config, ver, repodir.path(), &mut server)
			.await
			.expect("Failed to commit changes");
	}

	// remove the server
	server.destroy().await.expect("Failed to destroy the server");
}
