#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate log;

use askama::Template;
use bollard::Docker;
use serde::Deserialize;
use std::{
	env,
	path::Path,
	process::{exit, Command}
};
use tempfile::tempdir;
use tokio::{
	fs::{self, File},
	io::{AsyncReadExt, AsyncWriteExt}
};

mod package;

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
	jobs: usize
}

impl Config {
	fn dockerfile(&self) -> Dockerfile<'_> {
		Dockerfile {
			alpine: &self.alpine,
			pubkey: &self.pubkey,
			privkey: &self.privkey,
			jobs: num_cpus::get()
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

	info!("Updating repository metadata");
	fs::copy(&config.pubkey, repodir.path().join(&config.pubkey))
		.await
		.expect("Unable to copy pubkey");
	let mut index_html = File::create(repodir.path().join("index.html"))
		.await
		.expect("Unable to create index.html");
	index_html
		.write_all(config.render().expect("Unable to render index.html").as_bytes())
		.await
		.expect("Unable to write index.html");
	drop(index_html);
	if !run_git(repodir.path(), &["diff", "--exit-code"]) {
		info!("Commiting metadata changes");
		if !run_git(repodir.path(), &["add", "index.html", &config.pubkey]) {
			error!("Failed to add files to git");
			exit(1);
		}
		if !run_git(repodir.path(), &["commit", "-m", "Update repository metadata"]) {
			error!("Failed to create commit");
			exit(1);
		}
		if !run_git(repodir.path(), &["push"]) {
			error!("Failed to push commit");
			exit(1);
		}
	} else {
		info!("Metadata up to date");
	}

	let docker = Docker::connect_with_unix_defaults().expect("Cannot connect to docker daemon");

	for ver in &config.versions {
		package::build(repodir.path(), &docker, &config, ver).await;
	}
}
