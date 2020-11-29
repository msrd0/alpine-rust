#[macro_use]
extern crate log;

use askama::Template;
use bollard::{image::BuildImageOptions, Docker};
use serde::Deserialize;
use std::{
	fs::File,
	io::{Cursor, Read},
	process::exit
};
use tokio::stream::StreamExt;

#[derive(Deserialize, Template)]
#[template(path = "APKBUILD.tpl", escape = "none")]
struct APKBUILD {
	rustver: String,
	pkgver: String,
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
		info!("Building Rust {}", ver.pkgver);

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
			let mut file = File::open(key).expect("Failed to open abuild key");
			tar.append_file(key, &mut file).expect("Failed to write abuild key");
		}

		// finish the tar archive
		tar.finish().expect("Failed to finish tar archive");
		drop(tar);

		// build the docker image
		let img = format!("alpine-rust-builder-{}", ver.rustver);
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
	}
}
