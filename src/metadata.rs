use super::{run_git, Config};
use askama::Template;
use std::{env, path::Path, process::exit};
use tokio::{
	fs::{self, File},
	io::{self, AsyncWriteExt}
};

async fn exists(path: &Path) -> io::Result<bool> {
	match fs::metadata(path).await {
		Ok(_) => Ok(true),
		Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
		Err(err) => Err(err)
	}
}

pub(super) async fn update(config: &Config, repodir: &Path) {
	info!("Updating repository metadata");

	let pubkey_existed = exists(&repodir.join(&config.pubkey)).await.expect("Unable to stat pubkey");
	fs::copy(&config.pubkey, repodir.join(&config.pubkey))
		.await
		.expect("Unable to copy pubkey");

	let index_existed = exists(&repodir.join("index.html")).await.expect("Unable to stat index.html");
	let mut index_html = File::create(repodir.join("index.html"))
		.await
		.expect("Unable to create index.html");
	index_html
		.write_all(config.render().expect("Unable to render index.html").as_bytes())
		.await
		.expect("Unable to write index.html");
	drop(index_html);

	if !pubkey_existed || !index_existed || !run_git(repodir, &["diff", "--exit-code"]) {
		if env::var("CI").is_err() {
			info!("Running outside CI - Not commiting metadata changes");
		} else {
			info!("Commiting metadata changes");
			if !run_git(repodir, &["add", "index.html", &config.pubkey]) {
				error!("Failed to add files to git");
				exit(1);
			}
			if !run_git(repodir, &["commit", "-m", "Update repository metadata"]) {
				error!("Failed to create commit");
				exit(1);
			}
			if !run_git(repodir, &["push"]) {
				error!("Failed to push commit");
				exit(1);
			}
		}
	} else {
		info!("Metadata up to date");
	}
}
