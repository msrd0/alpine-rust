#[macro_use]
extern crate async_trait;
#[macro_use]
extern crate indoc;
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate log;

use either::Either;
use futures_util::{stream, FutureExt, StreamExt};
use log::LevelFilter;
use std::{env, path::PathBuf, process::exit, sync::Arc};
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
use server::{local::LocalServer, upcloud::UpcloudServer, Server};

lazy_static! {
	static ref CLIENT: reqwest::Client = reqwest::Client::new();
	static ref GITHUB_TOKEN: String = env::var("GITHUB_TOKEN").expect("GITHUB_TOKEN must be set");
}

/// Utility to compile Rust packages for Alpine Linux.
#[derive(StructOpt)]
struct Args {
	/// Verbose mode (-v, -vv, -vvv)
	#[structopt(short, long, parse(from_occurrences))]
	verbose: u8,

	/// Configuration file
	#[structopt(long, default_value = "config.toml")]
	config: PathBuf,

	/// Update the configuration file if a newer rust version was found
	#[structopt(short = "c", long)]
	update_config: bool,

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

	/// Specify the amount of parallel jobs. Defaults to the number of CPUs on the system.
	#[structopt(short = "j", long)]
	jobs: Option<u16>,

	/// Rust versions/channels to build, e.g. 1.42 or stable (optional)
	#[structopt(name = "CHANNEL")]
	channels: Vec<String>
}

#[tokio::main]
async fn main() {
	let args = Args::from_args();
	pretty_env_logger::formatted_timed_builder()
		.filter_module("alpine_rust", match args.verbose {
			0 => LevelFilter::Info,
			1 => LevelFilter::Debug,
			_ => LevelFilter::Trace
		})
		.init();

	if args.update_config {
		config::update_config(&args.config).await;
	}

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
	let x86_64 = repodir.join(format!("{}/alpine-rust/x86_64", config.alpine.version));
	debug!("Creating directory {}", x86_64.display());
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
	debug!("Determining packages that needs updates");
	let pkg_updates;
	let config_ver_iter = config.rust.keys().map(|channel| channel.as_str());
	if args.channels.is_empty() {
		pkg_updates = stream::iter(config_ver_iter)
			.filter(|ver| build::rust::up_to_date(&repodir, &config, ver).map(|up_to_date| !up_to_date))
			.collect::<Vec<_>>()
			.await;
	} else {
		pkg_updates = config_ver_iter
			.filter(|channel| args.channels.iter().any(|ch| ch == channel))
			.collect::<Vec<_>>()
	}

	// if everything is up to date, simply exit
	if pkg_updates.is_empty() {
		info!("Everything is up to date");
		return;
	}
	let pkg_updates_str = pkg_updates.join(", ");
	info!("The following rust versions will be updated: {}", pkg_updates_str);

	// connect to docker - create a server
	let server = if args.docker_local {
		Either::Left(LocalServer::new())
	} else if args.docker_upcloud {
		Either::Right(UpcloudServer::create(&config).await.expect("Failed to create UpCloud server"))
	} else {
		error!("Unable to connect to docker daemon: No docker daemon specified");
		exit(1);
	};

	// connect to docker - install the server
	if let Err(err) = server.install(&config, &repodir).await {
		error!("Failed to install server: {}", err);
		server.destroy().await.expect("Failed to destroy the server");
		exit(1);
	}

	// connect to docker
	let docker = match server.connect_to_docker() {
		Ok(docker) => docker,
		Err(err) => {
			error!("Failed to connect to docker: {}", err);
			server.destroy().await.expect("Failed to destroy the server");
			exit(1);
		}
	};
	let docker = Arc::new(docker);
	info!("Connected to docker daemon");

	// determine the docker environment
	debug!("Inspecting docker environment");
	let repomount = server.repomount(&repodir);
	let jobs = args.jobs.unwrap_or_else(|| server.cores());
	let cidr_v6 = server.cidr_v6();

	// start our local caddy server
	if let Err(err) = docker::build_caddy(&docker, &config).await {
		error!("Unable to build caddy image: {}", err);
		exit(1);
	}
	let caddy = match docker::start_caddy(&docker, &repomount).await {
		Ok(caddy) => caddy,
		Err(err) => {
			error!("Unable to start caddy container: {}", err);
			if let Some(cause) = err.source() {
				error!("Cause: {}", cause);
			}
			exit(1);
		}
	};

	// update packages
	for channel in pkg_updates {
		// build the package
		if args.skip_rust_packages {
			info!("Skipping rust packages for {}", channel)
		} else {
			if let Err(err) = build::rust::build_package(&repomount, &docker, &config, channel, jobs).await {
				error!("Failed to build package: {}", err);
				server.destroy().await.expect("Failed to destroy the server");

				exit(1);
			}
		}

		// test the package
		if let Err(err) = build::rust::test_package(docker.clone(), &cidr_v6, &config, channel).await {
			error!("Testing package failed: {}", err);
			// TODO maybe upload the package somewhere for manual inspection
			server.destroy().await.expect("Failed to destroy the server");

			exit(1);
		}

		// upload the changes
		if args.upload_packages {
			if let Err(err) = server.upload_repo_changes(&config, &repodir).await {
				error!("Failed to commit changes: {}", err);
				server.destroy().await.expect("Failed to destroy the server");
				exit(1);
			}
		}

		// build the docker images
		if args.skip_rust_docker {
			info!("Skipping rust docker images for {}", channel);
		} else {
			if let Err(err) = build::rust::build_and_upload_docker(&docker, &config, channel, args.upload_docker).await {
				error!("Failed to build docker images: {}", err);
				server.destroy().await.expect("Failed to destroy the server");
				exit(1);
			}
		}
	}

	// stop the caddy container
	if let Err(err) = caddy.stop(&docker).await {
		error!("Unable to stop caddy: {}", err);
	}

	// remove the server
	server.destroy().await.expect("Failed to destroy the server");
}
