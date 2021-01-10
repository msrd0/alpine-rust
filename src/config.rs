use crate::{CLIENT, GITHUB_TOKEN};
use anyhow::{bail, Context};
use chrono::NaiveDate;
use flate2::read::GzDecoder;
use futures_util::{StreamExt, TryStreamExt};
use regex::Regex;
use serde::Deserialize;
use sha2::{Digest, Sha512};
use std::{
	collections::HashMap,
	fmt::LowerHex,
	future::Future,
	path::{Path, PathBuf},
	process::Command
};
use tempfile::{tempdir, NamedTempFile};
use tokio::{
	fs::{self, File},
	io::{AsyncReadExt, AsyncWriteExt}
};
use toml_edit::{table, value, Document};

#[derive(Deserialize)]
pub struct Config {
	pub alpine: Alpine,
	#[serde(default)]
	pub rust: HashMap<String, Rust>
}

#[derive(Deserialize)]
pub struct Alpine {
	pub version: String,
	pub pubkey: String,
	pub privkey: String
}

#[derive(Deserialize)]
pub struct Rust {
	pub pkgver: String,
	pub pkgrel: u32,
	pub date: Option<NaiveDate>,
	pub llvmver: u32,
	pub bootver: String,
	pub bootsys: bool,
	pub sysver: Option<String>,
	pub python: Option<String>,
	pub sha512sums: String
}

lazy_static! {
	static ref VERSION_REGEX: Regex = Regex::new(
		r#"(?P<major>\d+)\.(?P<minor>\d+).(?P<patch>\d+)(-(beta|nightly)\.\d+)?\s+\([0-9a-f]+\s+(?P<y>\d{4})-(?P<m>\d{2})-(?P<d>\d{2})\)"#
	)
	.unwrap();
}

fn get(url: &str) -> impl Future<Output = reqwest::Result<reqwest::Response>> {
	info!("Downloading {}", url);
	CLIENT
		.get(url)
		.header(
			"User-Agent",
			"alpine-rust bot; https://github.com/msrd0/alpine-rust; CONTACT: https://matrix.to/#/@msrd0:msrd0.de"
		)
		.send()
}

async fn get_hash_extract(url: &str, location: &Path) -> anyhow::Result<impl LowerHex> {
	let res = get(url).await?;
	if res.status().as_u16() != 200 {
		bail!("{} returned non-200 status {}", url, res.status().as_u16());
	}

	// I don't care about the name but I need the path, not a file
	let tempfile = NamedTempFile::new()?;

	let (hash, _) = res
		.bytes_stream()
		.map_err(anyhow::Error::from)
		.try_fold(
			(Sha512::new(), File::create(tempfile.path()).await?),
			|(mut hash, mut tempfile), bytes| async move {
				hash.update(&bytes);
				tempfile.write_all(&bytes).await?;
				Ok((hash, tempfile))
			}
		)
		.await?;

	let mut file = std::fs::File::open(tempfile.path())?;
	let mut archive = tar::Archive::new(GzDecoder::new(&mut file));
	for entry in archive.entries().context("Unable to get archive entries")? {
		entry
			.context("Unable to get archive entry")?
			.unpack_in(location)
			.context("Unable to extract archive entry")?;
	}

	Ok(hash.finalize())
}

async fn test_patches(src_path: &Path, rustc_src_ver: &str, major: i64, minor: i64) -> anyhow::Result<()> {
	let rust_src = src_path.join(format!("rustc-{}-src", rustc_src_ver));
	let patches = src_path.join(format!(
		"alpine-rust-patches-{major}.{minor}/patches-{major}.{minor}",
		major = major,
		minor = minor
	));

	let patch_files = fs::read_dir(patches).await?.collect::<Vec<_>>().await;
	let mut patch_files = patch_files.into_iter().collect::<Result<Vec<_>, _>>()?;
	if patch_files.is_empty() {
		bail!("Missing patches for Rust {}.{}", major, minor);
	}

	patch_files.sort_by_key(|file| file.path());
	for patch in patch_files {
		let path = patch.path();
		info!("Testing patch {} against rustc src ...", path.display());
		let status = Command::new("patch")
			.args(&["-N", "-p", "1", "-i", &path.to_string_lossy()])
			.current_dir(&rust_src)
			.status()?;
		if !status.success() {
			bail!("patching {} was unsuccessfull: exit code {:?}", path.display(), status.code())
		}
	}

	Ok(())
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

	for channel in &["stable", "beta"] {
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
		let major: i64 = version_match["major"].parse().unwrap();
		let minor: i64 = version_match["minor"].parse().unwrap();
		let patch: i64 = version_match["patch"].parse().unwrap();
		let date = channel_metadata["date"].as_str().unwrap();
		let rustver = format!("{}.{}", major, minor);
		let pkgver = format!("{}.{}.{}", major, minor, patch);
		let bootver = match *channel {
			"beta" => "stable".to_owned(),
			"nightly" => "beta".to_owned(),
			_ => format!("{}.{}", major, minor - 1)
		};
		info!(
			"Channel {} is at version {} (raw: {} from {})",
			channel, pkgver, version_raw, date
		);

		let channel_old_version = config["rust"][channel]["pkgver"].as_str().map(|it| it.to_string());
		let channel_needs_update =
			channel_old_version.as_deref() != Some(&pkgver) || config["rust"][channel]["date"].as_str() != Some(&date);
		let version_old_version = config["rust"][&rustver]["pkgver"].as_str().map(|it| it.to_string());
		let version_needs_update = *channel == "stable" && version_old_version.as_deref() != Some(&pkgver);

		if !channel_needs_update && !version_needs_update {
			continue;
		}

		let src_dir = tempdir().expect("Failed to create tempdir");
		let src_path = src_dir.path();

		let mut sha512sums = "\n".to_owned();
		let rustc_src_ver = match *channel {
			"beta" => "beta",
			"nightly" => "nightly",
			_ => &pkgver
		};
		let rust_src_url = format!(
			"https://static.rust-lang.org/dist/{}/rustc-{}-src.tar.gz",
			date, rustc_src_ver
		);
		let rust_src = get_hash_extract(&rust_src_url, src_path)
			.await
			.expect("Failed to download rust src");
		sha512sums += &format!("{:x}  rustc-{}-src.tar.gz\n", rust_src, rustc_src_ver);
		let patches_url = format!(
			"https://github.com/msrd0/alpine-rust/archive/patches/{}.{}.tar.gz",
			major, minor
		);
		let patches = get_hash_extract(&patches_url, src_path)
			.await
			.expect("Failed to download rust patches");
		sha512sums += &format!("{:x}  1.{}.tar.gz\n", patches, minor);

		test_patches(src_path, rustc_src_ver, major, minor)
			.await
			.expect("Failed to apply patches");

		if channel_needs_update {
			info!(
				"Updating channel {} from {} to {} ({})",
				channel,
				channel_old_version.as_deref().unwrap_or("None"),
				pkgver,
				date
			);

			let mut tbl = table();
			tbl.as_table_mut().unwrap().set_implicit(true);
			tbl["pkgver"] = value(pkgver.as_str());
			tbl["pkgrel"] = value(0);
			tbl["date"] = value(date);
			tbl["llvmver"] = value(10);
			tbl["bootver"] = value(bootver.as_str());
			tbl["bootsys"] = value(false);
			tbl["sha512sums"] = value(sha512sums.as_str());
			config["channel"][channel] = tbl;
			updated = true;
		}

		if version_needs_update {
			info!(
				"Updating version {} from {} to {}",
				rustver,
				version_old_version.as_deref().unwrap_or("None"),
				pkgver
			);

			let mut tbl = table();
			tbl.as_table_mut().unwrap().set_implicit(true);
			tbl["pkgver"] = value(pkgver.as_str());
			tbl["pkgrel"] = value(0);
			tbl["llvmver"] = value(10);
			tbl["bootver"] = value(bootver.as_str());
			tbl["bootsys"] = value(false);
			tbl["sha512sums"] = value(sha512sums.as_str());
			config["rust"][rustver] = tbl;
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
