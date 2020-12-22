use crate::{CLIENT, GITHUB_TOKEN};
use futures_util::TryStreamExt;
use regex::Regex;
use serde::Deserialize;
use sha2::{Digest, Sha512};
use std::{collections::HashMap, future::Future, path::PathBuf, process::Command};
use tokio::{
	fs::File,
	io::{AsyncReadExt, AsyncWriteExt}
};
use toml_edit::{table, value, Document};

#[derive(Deserialize)]
pub struct Config {
	pub alpine: String,
	pub pubkey: String,
	pub privkey: String,
	#[serde(default)]
	pub versions: Vec<Version>,
	#[serde(default)]
	pub channel: HashMap<String, Version>
}

#[derive(Deserialize)]
pub struct Version {
	pub channel: Option<String>,
	pub rustminor: u32,
	pub rustpatch: u32,
	pub pkgrel: u32,
	pub date: Option<String>,
	pub llvmver: u32,
	pub bootver: String,
	pub bootsys: bool,
	pub sysver: Option<String>,
	pub python: Option<String>,
	pub sha512sums: String
}

lazy_static! {
	static ref VERSION_REGEX: Regex =
		Regex::new(r#"1\.(?P<minor>\d+).(?P<patch>\d+)\s+\([0-9a-f]+\s+(?P<y>\d{4})-(?P<m>\d{2})-(?P<d>\d{2})\)"#).unwrap();
}

fn get(url: &str) -> impl Future<Output = reqwest::Result<reqwest::Response>> {
	CLIENT
		.get(url)
		.header(
			"User-Agent",
			"alpine-rust bot; https://github.com/msrd0/alpine-rust; CONTACT: https://matrix.to/#/@msrd0:msrd0.de"
		)
		.send()
}

pub async fn update_config(config_path: &PathBuf) {
	let config_path = config_path.canonicalize().unwrap();
	info!("Reading {}", config_path.display());
	let mut config_file = File::open(&config_path).await.expect("Unable to find config file");
	let mut config_buf = String::new();
	config_file
		.read_to_string(&mut config_buf)
		.await
		.expect("Unable to read config file");
	drop(config_file);
	let mut config = config_buf.parse::<Document>().expect("Failed to parse config file");
	let mut updated = false;

	if config["channel"].as_table_like().is_none() {
		let mut tbl = table();
		tbl.as_table_mut().unwrap().set_implicit(true);
		config["channel"] = tbl;
	}

	for channel in &["stable"] {
		let channel_metadata_buf = get(&format!("https://static.rust-lang.org/dist/channel-rust-{}.toml", channel))
			.await
			.expect("Failed to query channel")
			.bytes()
			.await
			.expect("Failed to read channel response");
		let channel_metadata: toml::Value =
			toml::from_slice(&channel_metadata_buf).expect("Failed to parse channel response");

		let renames = &channel_metadata["renames"];
		let to = "to";
		let rust_name = renames.get("rust").and_then(|name| name[to].as_str()).unwrap_or("rust");
		let rustfmt_name = renames.get("rustfmt").and_then(|name| name[to].as_str()).unwrap_or("rustfmt");
		let clippy_name = renames.get("clippy").and_then(|name| name[to].as_str()).unwrap_or("clippy");

		let pkg = &channel_metadata["pkg"];
		let target = "target";
		let x86_64 = "x86_64-unknown-linux-gnu";
		let available = "available";
		let rust_available = pkg[rust_name][target][x86_64][available].as_bool();
		let rustfmt_available = pkg[rustfmt_name][target][x86_64][available].as_bool();
		let clippy_available = pkg[clippy_name][target][x86_64][available].as_bool();

		if rust_available != Some(true) || rustfmt_available != Some(true) || clippy_available != Some(true) {
			info!("Skipping channel {} due to missing packages", channel);
			continue;
		}

		let version_raw = channel_metadata["pkg"][rust_name]["version"].as_str().unwrap();
		let version_match = VERSION_REGEX.captures_iter(version_raw).next().unwrap();
		let minor: i64 = version_match["minor"].parse().unwrap();
		let patch: i64 = version_match["patch"].parse().unwrap();
		let date_raw = channel_metadata["date"].as_str().unwrap();
		let date_condensed = date_raw.replace("-", "");
		let version = format!("1.{}.{}.{}", minor, patch, date_condensed);
		info!(
			"Channel {} is at version {} (raw: {} from {})",
			channel, version, version_raw, date_raw
		);

		if config["channel"][channel]["_version"].as_str() != Some(&version) {
			info!("Updating channel {} to {}", channel, version);

			let mut sha512sums = "\n".to_owned();
			let rust_src = get(&format!(
				"https://static.rust-lang.org/dist/{}/rustc-1.{}.{}-src.tar.gz",
				date_raw, minor, patch
			))
			.await
			.expect("Failed to query rust src")
			.bytes_stream()
			.try_fold(Sha512::new(), |mut hash, bytes| async move {
				hash.update(bytes);
				Ok(hash)
			})
			.await
			.expect("Failed to get sha512 sum of rust rust")
			.finalize();
			sha512sums += &format!("{:x}  rustc-1.{}.{}-src.tar.gz\n", rust_src, minor, patch);
			let patches = get(&format!(
				"https://github.com/msrd0/alpine-rust/archive/patches/1.{}.tar.gz",
				minor
			))
			.await
			.expect("Failed to query rust src")
			.bytes_stream()
			.try_fold(Sha512::new(), |mut hash, bytes| async move {
				hash.update(bytes);
				Ok(hash)
			})
			.await
			.expect("Failed to get sha512 sum of rust rust")
			.finalize();
			sha512sums += &format!("{:x}  1.{}.tar.gz\n", patches, minor);

			let mut tbl = table();
			tbl.as_table_mut().unwrap().set_implicit(true);
			tbl["_version"] = value(version.as_ref());
			tbl["channel"] = value(*channel);
			tbl["rustminor"] = value(minor);
			tbl["rustpatch"] = value(patch);
			tbl["pkgrel"] = value(1);
			tbl["date"] = value(date_raw);
			tbl["llvmver"] = value(10);
			tbl["bootver"] = value(format!("1.{}", minor - 1));
			tbl["bootsys"] = value(false);
			tbl["sha512sums"] = value(sha512sums);
			config["channel"][channel] = tbl;
			updated = true;
		}
	}

	if updated {
		info!("Writing updated config file");
		let mut config_file = File::create(&config_path).await.expect("Unable to create config file");
		config_file
			.write(config.to_string().as_bytes())
			.await
			.expect("Failed to write config file");

		info!("Commiting updated config file");
		let dir = config_path.parent().unwrap();
		println!("DIR: {}", dir.display());
		Command::new("git")
			.args(&["commit", "-n", "-m", COMMIT_MESSAGE, &config_path.to_string_lossy()])
			.current_dir(&dir)
			.env("GIT_AUTHOR_NAME", "drone.msrd0.eu [bot]")
			.env("GIT_AUTHOR_EMAIL", "noreply@drone.msrd0.eu")
			.env("GIT_COMMITTER_NAME", "drone.msrd0.eu [bot]")
			.env("GIT_COMMITTER_EMAIL", "noreply@drone.msrd0.eu")
			.status()
			.expect("Failed to run git commit");
		Command::new("git")
			.args(&[
				"push",
				&format!(
					"https://drone-msrd0-eu:{}@github.com/msrd0/alpine-rust.git",
					GITHUB_TOKEN.as_str()
				)
			])
			.current_dir(&dir)
			.status()
			.expect("Failed to run git push");
	}
}

const COMMIT_MESSAGE: &str = r#"Update config.toml

This commit was automatically created because an update for one of the rust channels was found.

[skip ci] to prevent unwanted recursion
"#;
