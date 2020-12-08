#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate log;

use bollard::{Docker, API_DEFAULT_VERSION};
use env::current_dir;
use futures_util::{stream, FutureExt, StreamExt};
use std::{borrow::Cow, collections::HashSet, env, process::exit};
use tempfile::tempdir;
use tokio::{
	fs::{self, File},
	io::AsyncReadExt
};
use upcloud::UPCLOUD_CORES;

mod config;
mod docker;
mod metadata;
mod package;
mod repo;
mod templates;
mod upcloud;

use config::*;

lazy_static! {
	static ref CI: bool = env::var("CI").is_ok();
	static ref GITHUB_TOKEN: String = env::var("GITHUB_TOKEN").expect("GITHUB_TOKEN must be set");
}

#[tokio::main]
async fn main() {
	pretty_env_logger::init_timed();

	info!("Reading config.toml");
	let mut config_file = File::open("config.toml").await.expect("Unable to find config.toml");
	let mut config_buf = Vec::<u8>::new();
	config_file
		.read_to_end(&mut config_buf)
		.await
		.expect("Unable to read config.toml");
	drop(config_file);
	let config: Config = toml::from_slice(&config_buf).expect("Invalid syntax in versions.toml");

	// download the repository
	let repotmp = tempdir().expect("Failed to create tempdir");
	let repodir = repotmp.path();
	repo::download(&repodir).await.expect("Failed to download repo");

	// create the repo dir if it does not exist yet
	if let Err(err) = fs::create_dir_all(repodir.join(format!("{}/alpine-rust/x86_64", config.alpine))).await {
		warn!("Unable to create {}/alpine-rust/x86_64: {}", config.alpine, err);
	}

	// update the metadata
	metadata::update(&config, &repodir).await;

	// search for versions that need to be updated
	let args = env::args().collect::<HashSet<_>>();
	let pkg_updates;
	if args.is_empty() {
		pkg_updates = stream::iter(config.versions.iter())
			.filter(|ver| package::up_to_date(&repodir, &config, ver).map(|up_to_date| !up_to_date))
			.collect::<Vec<_>>()
			.await;
	} else {
		pkg_updates = config
			.versions
			.iter()
			.filter(|ver| args.contains(&format!("1.{}", ver.rustminor)))
			.collect::<Vec<_>>()
	}

	// if everything is up to date, simply exit
	if pkg_updates.is_empty() {
		info!("Everything is up to date");
		return;
	}
	let pkg_updates_str = pkg_updates
		.iter()
		.map(|ver| format!("1.{}", ver.rustminor))
		.collect::<Vec<_>>()
		.join(", ");
	info!("The following rust versions will be updated: {}", pkg_updates_str);

	// upcloud for CI, local for non-CI
	let (mut server, docker) = if *CI {
		// create an upcloud server
		let server = upcloud::create_server().await.expect("Failed to create UpCloud server");

		let server = match upcloud::install_server(&config, &server, &repodir).await {
			Ok(server) => server,
			Err(err) => {
				error!("Failed to install server: {}", err);
				server.destroy().await.expect("Failed to destroy the server");
				exit(1);
			}
		};

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
			if let Err(err) = package::build_package(&repodir, &docker, &config, ver, jobs).await {
				error!("Failed to build package: {}", err);
				if let Some(server) = server {
					server.destroy().await.expect("Failed to destroy the server");
				}
				exit(1);
			}
		}

		// upload the changes
		if let Some(mut server) = server.as_mut() {
			if let Err(err) = upcloud::commit_changes(&config, &repodir, &mut server).await {
				error!("Failed to commit changes: {}", err);
				server.destroy().await.expect("Failed to destroy the server");
				exit(1);
			}
		} else {
			warn!("Not running in CI - No changes commited");
		}

		// build the docker images
		if let Err(err) = package::build_docker(&docker, &config, ver).await {
			error!("Failed to build docker images: {}", err);
			if let Some(server) = server {
				server.destroy().await.expect("Failed to destroy the server");
			}
			exit(1);
		}
	}

	// remove the server
	if let Some(server) = server {
		server.destroy().await.expect("Failed to destroy the server");
	}
}
