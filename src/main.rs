#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate log;

use bollard::{Docker, API_DEFAULT_VERSION};
use env::current_dir;
use futures_util::{stream, FutureExt, StreamExt};

use log::LevelFilter;
use server::upcloud::UPCLOUD_CORES;
use std::{borrow::Cow, env, path::PathBuf, process::exit};
use structopt::StructOpt;
use tempfile::tempdir;
use tokio::{
	fs::{self, File},
	io::AsyncReadExt
};

mod build;
mod config;
mod docker;
mod metadata;
mod repo;
mod server;
mod templates;

use config::*;

lazy_static! {
	static ref GITHUB_TOKEN: String = env::var("GITHUB_TOKEN").expect("GITHUB_TOKEN must be set");
}

/// Utility to compile Rust packages for Alpine Linux.
#[derive(StructOpt)]
struct Args {
	/// Verbose mode (-v, -vv, -vvv)
	#[structopt(short, long, parse(from_occurrences))]
	verbose: u8,

	/// Configuration file
	#[structopt(short, long, default_value = "config.toml")]
	config: PathBuf,

	/// Use custom dir to download the repository
	#[structopt(short, long)]
	repodir: Option<PathBuf>,

	/// Skip updating metadata
	#[structopt(long)]
	skip_metadata: bool,

	/// Skip building rust packages
	#[structopt(long)]
	skip_rust_packages: bool,

	/// Skip building rust docker images
	#[structopt(long)]
	skip_rust_docker: bool,

	/// Upload the metadata
	#[structopt(short = "m", long)]
	upload_metadata: bool,

	/// Upload the built packages to the repository
	#[structopt(short = "p", long)]
	upload_packages: bool,

	/// Upload the built docker images to the registry
	#[structopt(short = "d", long)]
	upload_docker: bool,

	/// Use the local docker daemon
	#[structopt(short = "l", long)]
	docker_local: bool,

	/// Deploy a docker daemon on an upcloud server
	#[structopt(short = "u", long)]
	docker_upcloud: bool,

	/// Rust versions to build, e.g. 1.42 (optional)
	#[structopt(name = "VERSION")]
	versions: Vec<String>
}

#[tokio::main]
async fn main() {
	let args = Args::from_args();
	pretty_env_logger::formatted_timed_builder()
		.filter_level(match args.verbose {
			0 => LevelFilter::Info,
			1 => LevelFilter::Debug,
			_ => LevelFilter::Trace
		})
		.init();

	info!("Reading config.toml");
	let mut config_file = File::open(&args.config).await.expect("Unable to find config file");
	let mut config_buf = Vec::<u8>::new();
	config_file
		.read_to_end(&mut config_buf)
		.await
		.expect("Unable to read config file");
	drop(config_file);
	let config: Config = toml::from_slice(&config_buf).expect("Invalid syntax in config file");

	// download the repository
	let (_repotmp, repodir) = match &args.repodir {
		Some(repodir) => (None, repodir.to_owned()),
		None => {
			let repotmp = tempdir().expect("Failed to create tempdir");
			let repodir = repotmp.path().to_owned();
			(Some(repotmp), repodir)
		}
	};
	repo::download(&repodir).await.expect("Failed to download repo");

	// create the repo dir if it does not exist yet
	let x86_64 = repodir.join(format!("{}/alpine-rust/x86_64", config.alpine));
	if let Err(err) = fs::create_dir_all(&x86_64).await {
		warn!("Unable to create {}: {}", x86_64.display(), err);
	}

	// update the metadata
	if args.skip_metadata {
		info!("Skipping metadata update");
	} else {
		metadata::update(&config, &repodir, args.upload_metadata).await;
	}

	// search for versions that need to be updated
	let pkg_updates;
	if args.versions.is_empty() {
		pkg_updates = stream::iter(config.versions.iter())
			.filter(|ver| build::rust::up_to_date(&repodir, &config, ver).map(|up_to_date| !up_to_date))
			.collect::<Vec<_>>()
			.await;
	} else {
		pkg_updates = config
			.versions
			.iter()
			.filter(|ver| args.versions.contains(&format!("1.{}", ver.rustminor)))
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

	// connect to docker
	let (mut server, docker) = if args.docker_local {
		info!("Connecting to local docker daemon");
		let docker = Docker::connect_with_local_defaults().expect("Failed to connect to docker");
		(None, docker)
	} else if args.docker_upcloud {
		// create an upcloud server
		let server = server::upcloud::create_server()
			.await
			.expect("Failed to create UpCloud server");

		let server = match server::upcloud::install_server(&config, &server, &repodir).await {
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
		error!("Unable to connect to docker daemon: No docker daemon specified");
		exit(1);
	};

	// determine the docker environment
	let (repomount, jobs) = match &server {
		Some(_) => (Cow::Borrowed("/var/lib/alpine-rust"), UPCLOUD_CORES),
		None => (
			current_dir().unwrap().join("repo").to_string_lossy().to_string().into(),
			num_cpus::get() as u16
		)
	};

	// start our local caddy server
	if let Err(err) = docker::build_caddy(&docker, &config, &repomount).await {
		error!("Unable to build caddy image: {}", err);
		exit(1);
	}
	let caddy = match docker::start_caddy(&docker, &repomount).await {
		Ok(caddy) => caddy,
		Err(err) => {
			error!("Unable to start caddy container: {}", err);
			exit(1);
		}
	};

	// update packages
	for ver in pkg_updates {
		// build the package
		if args.skip_rust_packages {
			info!("Skipping rust packages for 1.{}", ver.rustminor);
		} else {
			if let Err(err) = build::rust::build_package(&repomount, &docker, &config, ver, jobs).await {
				error!("Failed to build package: {}", err);
				if let Some(server) = server {
					server.destroy().await.expect("Failed to destroy the server");
				}
				exit(1);
			}

			// upload the changes
			if args.upload_packages {
				if let Some(mut server) = server.as_mut() {
					if let Err(err) = server::upcloud::commit_changes(&config, &repodir, &mut server).await {
						error!("Failed to commit changes: {}", err);
						server.destroy().await.expect("Failed to destroy the server");
						exit(1);
					}
				} else {
					error!("Uploading packages without upcloud is not supported");
					exit(1);
				}
			}
		}

		// build the docker images
		if args.skip_rust_docker {
			info!("Skipping rust docker images for 1.{}", ver.rustminor);
		} else {
			if let Err(err) = build::rust::build_and_upload_docker(&docker, &config, ver, args.upload_docker).await {
				error!("Failed to build docker images: {}", err);
				if let Some(server) = server {
					server.destroy().await.expect("Failed to destroy the server");
				}
				exit(1);
			}
		}
	}

	// stop the caddy container
	if let Err(err) = caddy.stop(&docker).await {
		error!("Unable to stop caddy: {}", err);
	}

	// remove the server
	if let Some(server) = server {
		server.destroy().await.expect("Failed to destroy the server");
	}
}
