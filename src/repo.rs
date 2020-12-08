use anyhow::{anyhow, Context};
use s3::{creds::Credentials, Bucket, Region};
use std::{env, ffi::OsString, path::Path};
use tokio::{
	fs::{self, File},
	io::{self, AsyncReadExt, AsyncWriteExt}
};

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
	info!("Synchronizing repository to {}", dest.display());
	let bucket = Bucket::new_public_with_path_style(MINIO_BUCKET_NAME, REGION.clone()).context("Failed to open bucket")?;

	let list = bucket.list("/".to_owned(), None).await.context("Failed to list bucket")?;
	let objs = list.into_iter().flat_map(|res| res.contents.into_iter());

	for obj in objs {
		let key = obj.key;
		let key_relative = if key.starts_with("/") { &key[1..] } else { &key };
		let path = dest.join(key_relative);
		let parent = path.parent().ok_or(anyhow!("Destination doesn't have a parent"))?;

		let mut etag_name = OsString::from(".");
		etag_name.push(path.file_name().ok_or(anyhow!("Key does not have a filename"))?);
		etag_name.push(".etag");
		let etag_path = parent.join(&etag_name);

		let etag = match File::open(&etag_path).await {
			Ok(mut file) => {
				let mut etag = String::new();
				file.read_to_string(&mut etag).await.context("Failed to read etag file")?;
				Some(etag)
			},
			Err(err) if err.kind() == io::ErrorKind::NotFound => None,
			Err(err) => return Err(err).context("Failed to read etag file")
		};
		if etag.as_deref() == Some(&obj.e_tag) {
			continue;
		}

		info!("Downloading {} to {}", key, path.display());
		if let Some(parent) = path.parent() {
			fs::create_dir_all(parent)
				.await
				.context("Failed to create destination path")?;
		}

		let mut file = std::fs::File::create(&path).context("Failed to create destination file")?;
		bucket
			.get_object_stream(&key, &mut file)
			.await
			.context("Failed to download from bucket")?;

		let mut etag_file = match File::create(etag_path).await {
			Ok(file) => file,
			Err(err) => {
				error!("Failed to create etag file: {}", err);
				continue;
			}
		};
		if let Err(err) = etag_file.write_all(obj.e_tag.as_bytes()).await {
			error!("Failed to write etag file: {}", err);
		}
	}

	info!("Synchronization finished");
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
