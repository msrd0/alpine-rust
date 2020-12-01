use super::{run_git, Config};
use askama::Template;
use std::{path::Path, process::exit};
use tokio::{
	fs::{self, File},
	io::AsyncWriteExt
};

pub(super) async fn update(config: &Config, repodir: &Path) {
	info!("Updating repository metadata");
	fs::copy(&config.pubkey, repodir.join(&config.pubkey))
		.await
		.expect("Unable to copy pubkey");
	let mut index_html = File::create(repodir.join("index.html"))
		.await
		.expect("Unable to create index.html");
	index_html
		.write_all(config.render().expect("Unable to render index.html").as_bytes())
		.await
		.expect("Unable to write index.html");
	drop(index_html);
	if !run_git(repodir, &["diff", "--exit-code"]) {
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
	} else {
		info!("Metadata up to date");
	}
}
