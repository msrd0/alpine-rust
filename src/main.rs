#[macro_use]
extern crate log;

use askama::Template;
use serde::Deserialize;
use std::{
	fs::File,
	io::{self, Read, Write}
};
use tempfile::tempdir;

#[derive(Deserialize, Template)]
#[template(path = "APKBUILD.tpl", escape = "none")]
struct APKBUILD {
	rustver: String,
	pkgver: String,
	bootver: String,
	bootsys: bool,
	aportsha: String
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
	privkey: &'a str
}

impl Config {
	fn dockerfile(&self) -> Dockerfile<'_> {
		Dockerfile {
			alpine: &self.alpine,
			pubkey: &self.pubkey,
			privkey: &self.privkey
		}
	}
}

fn main() {
	pretty_env_logger::init_timed();

	info!("Reading versions.toml");
	let mut config_file = File::open("versions.toml").expect("Unable to find versions.toml");
	let mut config_buf = Vec::<u8>::new();
	config_file
		.read_to_end(&mut config_buf)
		.expect("Unable to read versions.toml");
	let config: Config = toml::from_slice(&config_buf).expect("Invalid syntax in versions.toml");

	for ver in &config.versions {
		info!("Building Rust {}", ver.pkgver);

		let tmpdir = tempdir().expect("Failed to create tempdir");
		let dir = tmpdir.path();
		info!("Using tmpdir {}", dir.to_string_lossy());

		// write the APKBUILD file
		{
			let mut apkbuild = File::create(dir.join("APKBUILD")).expect("Failed to create APKBUILD");
			// rendering to io::Write isn't possible: https://github.com/djc/askama/issues/163
			ver.render()
				.map_err(|err| io::Error::new(io::ErrorKind::Other, err))
				.and_then(|buf| apkbuild.write(buf.as_bytes()))
				.expect("Failed to write APKBUILD");
		}

		// write the Dockerfile file
		{
			let mut dockerfile = File::create(dir.join("Dockerfile")).expect("Failed to create Dockerfile");
			// rendering to io::Write isn't possible: https://github.com/djc/askama/issues/163
			config
				.dockerfile()
				.render()
				.map_err(|err| io::Error::new(io::ErrorKind::Other, err))
				.and_then(|buf| dockerfile.write(buf.as_bytes()))
				.expect("Failed to write Dockerfile");
		}

		// do not delete the tmpdir for now
		Box::leak(Box::new(tmpdir));
	}
}
