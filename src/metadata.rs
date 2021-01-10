use super::{repo, Config};
use askama::Template;
use std::path::Path;
use tokio::{
	fs::{self, File},
	io::AsyncWriteExt
};

pub(super) async fn update(config: &Config, repodir: &Path, upload_metadata: bool) {
	info!("Updating repository metadata");

	let path = repodir.join(&config.alpine.pubkey);
	fs::copy(&config.alpine.pubkey, &path).await.expect("Unable to copy pubkey");
	if upload_metadata {
		repo::upload(&path, &config.alpine.pubkey)
			.await
			.expect("Failed to upload pubkey");
	}

	let path = repodir.join("index.html");
	let mut index_html = File::create(&path).await.expect("Unable to create index.html");
	index_html
		.write_all(config.index_html().render().expect("Unable to render index.html").as_bytes())
		.await
		.expect("Unable to write index.html");
	drop(index_html);
	if upload_metadata {
		repo::upload(&path, "index.html").await.expect("Failed to upload index.html");
	}
}
