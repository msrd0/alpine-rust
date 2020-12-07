use anyhow::Context;
use s3::{creds::Credentials, Bucket, Region};
use std::{env, fs::File, path::Path};
use tokio::fs;

const MINIO_BUCKET_NAME: &str = "alpine-rust";

lazy_static! {
	static ref MINIO_ACCESS_KEY: String = env::var("MINIO_ACCESS_KEY").expect("MINIO_ACCESS_KEY must be set");
	static ref MINIO_SECRET_KEY: String = env::var("MINIO_SECRET_KEY").expect("MINIO_SECRET_KEY must be set");
	static ref REGION: Region = Region::Custom {
		region: "msrd0cdn.de".to_owned(),
		endpoint: "https://msrd0cdn.de".to_owned()
	};
}

pub(super) async fn download(dest: &Path) -> anyhow::Result<()> {
	let bucket = Bucket::new_public_with_path_style(MINIO_BUCKET_NAME, REGION.clone()).context("Failed to open bucket")?;

	let list = bucket.list("/".to_owned(), None).await.context("Failed to list bucket")?;
	let keys = list.into_iter().flat_map(|res| res.contents.into_iter().map(|obj| obj.key));

	for key in keys {
		let key_relative = if key.starts_with("/") { &key[1..] } else { &key };
		let path = dest.join(key_relative);
		info!("Downloading {} to {}", key, path.display());

		if let Some(parent) = path.parent() {
			fs::create_dir_all(parent)
				.await
				.context("Failed to create destination path")?;
		}

		let mut file = File::create(&path).context("Failed to create destination file")?;
		bucket
			.get_object_stream(&key, &mut file)
			.await
			.context("Failed to download from bucket")?;
	}

	Ok(())
}

pub(super) async fn upload(path: impl AsRef<Path>, key: &str) -> anyhow::Result<()> {
	let creds = Credentials::new(Some(&MINIO_ACCESS_KEY), Some(&MINIO_SECRET_KEY), None, None, None)
		.context("Failed to get MinIO creds")?;
	let bucket = Bucket::new_with_path_style(MINIO_BUCKET_NAME, REGION.clone(), creds).context("Failed to open bucket")?;

	let path = path.as_ref();
	info!("Uploading {} from {}", key, path.display());
	bucket
		.put_object_stream(path, key)
		.await
		.context("Failed to upload to bucket")?;

	Ok(())
}
