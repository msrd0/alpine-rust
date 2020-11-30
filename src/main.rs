#[macro_use]
extern crate log;

use askama::Template;
use bollard::Docker;
use serde::Deserialize;
use std::{fs::File, io::Read};

mod package;

#[derive(Deserialize, Template)]
#[template(path = "APKBUILD.tpl", escape = "none")]
struct APKBUILD {
	rustver: String,
	pkgver: String,
	pkgrel: u32,
	bootver: String,
	bootsys: bool,
	aportsha: String,
	sha512sums: String
}

#[derive(Deserialize)]
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

#[tokio::main]
async fn main() {
	pretty_env_logger::init_timed();

	info!("Reading versions.toml");
	let mut config_file = File::open("versions.toml").expect("Unable to find versions.toml");
	let mut config_buf = Vec::<u8>::new();
	config_file
		.read_to_end(&mut config_buf)
		.expect("Unable to read versions.toml");
	let config: Config = toml::from_slice(&config_buf).expect("Invalid syntax in versions.toml");

	let docker = Docker::connect_with_unix_defaults().expect("Cannot connect to docker daemon");

	for ver in &config.versions {
		package::build(&docker, &config, ver).await;
	}
}
