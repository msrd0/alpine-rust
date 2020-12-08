use super::{repo, Config, CI};
use askama::Template;
use std::path::Path;
use tokio::{
	fs::{self, File},
	io::AsyncWriteExt
};

pub(super) async fn update(config: &Config, repodir: &Path) {
	info!("Updating repository metadata");

	let path = repodir.join(&config.pubkey);
	fs::copy(&config.pubkey, &path).await.expect("Unable to copy pubkey");
	if *CI {
		repo::upload(&path, &config.pubkey).await.expect("Failed to upload pubkey");
	}

	let path = repodir.join("index.html");
	let mut index_html = File::create(&path).await.expect("Unable to create index.html");
	index_html
		.write_all(config.index_html().render().expect("Unable to render index.html").as_bytes())
		.await
		.expect("Unable to write index.html");
	drop(index_html);
	if *CI {
		repo::upload(&path, "index.html").await.expect("Failed to upload index.html");
	}
}
