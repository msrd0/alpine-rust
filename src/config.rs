use crate::{CLIENT, GITHUB_TOKEN};
use anyhow::{bail, Context};
use chrono::NaiveDate;
use flate2::read::GzDecoder;
use futures_util::StreamExt;
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

// no, serde does not allow values to be used as default values
fn bool_true() -> bool {
	true
}

#[derive(Deserialize)]
pub struct Config {
	pub alpine: Alpine,
	#[serde(default)]
	pub packages: Packages,
	#[serde(default)]
	pub rust: HashMap<String, Rust>
}

#[derive(Default, Deserialize)]
pub struct Packages {
	#[serde(default)]
	pub llvm: Vec<PackageLLVM>,
	#[serde(default, rename = "crate")]
	pub crates: Vec<PackageCrate>
}

#[derive(Deserialize)]
pub struct PackageLLVM {
	pub pkgver: String,
	pub pkgrel: u32,
	#[serde(default)]
	pub paxmark: bool,
	pub sha512sum: String
}

#[derive(Deserialize)]
pub struct PackageCrate {
	pub crate_name: String,
	pub version: String,
	pub pkgrel: u32,
	pub description: String,
	pub license: String,
	#[serde(default = "bool_true")]
	pub check: bool,
	pub dependencies: Vec<String>,
	pub sha512sum: String
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

async fn check_patches_exist(version: &str) -> reqwest::Result<bool> {
	#[derive(Deserialize)]
	struct GitHubBranch {
		name: String
	}

	let branches: Vec<GitHubBranch> = get("https://api.github.com/repos/msrd0/alpine-rust/branches")
		.await?
		.json()
		.await?;
	let branch_name = format!("patches/{}", version);
	Ok(branches.iter().any(|branch| branch.name == branch_name))
}

fn copy_patches(version: &str, from: &str) -> anyhow::Result<()> {
	let dir = tempdir()?;
	let path = dir.path();

	let script = format!(
		r#"
			export GIT_AUTHOR_NAME GIT_AUTHOR_EMAIL GIT_COMMITTER_NAME GIT_COMMITTER_EMAIL
			git clone --branch patches/{from} https://drone-msrd0-eu:{GITHUB_TOKEN}@github.com/msrd0/alpine-rust.git {path}
			cd {path}
			git checkout -b patches/{version}
			git mv patches-{from} patches-{version}
			git commit -m "mv patches-{from} to patches-{version}"
			git reset --soft HEAD~$(($(git rev-list --count HEAD)-1))
			git commit --amend -m "copy patches for Rust {version} from Rust {from} at $(git rev-parse HEAD)"
			git push https://drone-msrd0-eu:{GITHUB_TOKEN}@github.com/msrd0/alpine-rust.git patches/{version}
			rm -rf {path}
		"#,
		GITHUB_TOKEN = GITHUB_TOKEN.as_str(),
		path = path.display(),
		version = version,
		from = from
	)
	.trim()
	.replace("\n", " && ");
	let status = Command::new("/bin/busybox")
		.args(&["ash", "-uo", "pipefail", "-c", &script])
		.env("GIT_AUTHOR_NAME", "drone.msrd0.eu [bot]")
		.env("GIT_AUTHOR_EMAIL", "noreply@drone.msrd0.eu")
		.env("GIT_COMMITTER_NAME", "drone.msrd0.eu [bot]")
		.env("GIT_COMMITTER_EMAIL", "noreply@drone.msrd0.eu")
		.status()?;
	if !status.success() {
		bail!("Copying patches returned non-zero exit code {:?}", status.code());
	}

	Ok(())
}

async fn get_hash_extract(cache_dir: &PathBuf, url: &str, location: &Path) -> anyhow::Result<impl LowerHex> {
	let mut hash = Sha512::new();
	let tempfile = NamedTempFile::new()?;
	let mut file = File::create(tempfile.path()).await?;

	if let Ok(mut cache) = File::open(cache_dir.join(base64::encode(url))).await {
		info!("Using cache for {}", url);
		let mut buf = [0u8; 8192];
		loop {
			let read = cache.read(&mut buf).await?;
			if read == 0 {
				break;
			}
			hash.update(&buf[..read]);
			file.write_all(&buf[..read]).await?;
		}
	} else {
		let res = get(url).await?;
		if res.status().as_u16() != 200 {
			bail!("{} returned non-200 status {}", url, res.status().as_u16());
		}

		let mut cache = File::create(cache_dir.join(base64::encode(url))).await?;
		let mut bytes = res.bytes_stream();
		while let Some(buf) = bytes.next().await {
			let buf = buf?;
			hash.update(&buf);
			file.write_all(&buf).await?;
			cache.write_all(&buf).await?;
		}
	}
	drop(file);

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

pub async fn update_config(config_path: &PathBuf, cache_dir: Option<&PathBuf>) {
	let default_cache_dir = dirs_next::cache_dir().unwrap().join("alpine-rust");
	let cache_dir = cache_dir.unwrap_or(&default_cache_dir);
	fs::create_dir_all(cache_dir).await.expect("Failed to create cache dir");

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

		if !check_patches_exist(&format!("{}.{}", major, minor))
			.await
			.expect("Failed to check patches")
		{
			copy_patches(&format!("{}.{}", major, minor), &format!("{}.{}", major, minor - 1))
				.expect("Failed to copy patches");
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
		let rust_src = get_hash_extract(cache_dir, &rust_src_url, src_path)
			.await
			.expect("Failed to download rust src");
		sha512sums += &format!("{:x}  rustc-{}-src.tar.gz\n", rust_src, rustc_src_ver);
		let patches_url = format!(
			"https://github.com/msrd0/alpine-rust/archive/patches/{}.{}.tar.gz",
			major, minor
		);
		let patches = get_hash_extract(cache_dir, &patches_url, src_path)
			.await
			.expect("Failed to download rust patches");
		sha512sums += &format!("{:x}  rustc-patches-1.{}.tar.gz\n", patches, minor);

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
			tbl["llvmver"] = value(11);
			tbl["bootver"] = value(bootver.as_str());
			tbl["bootsys"] = value(false);
			tbl["sha512sums"] = value(sha512sums.as_str());
			config["rust"][channel] = tbl;
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
