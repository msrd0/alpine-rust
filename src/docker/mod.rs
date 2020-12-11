use std::io::Cursor;

mod caddy;
pub use caddy::*;
mod cidr_v6;
pub use cidr_v6::*;
mod keys;
pub use keys::*;

pub fn tar_header(path: &str, len: usize) -> tar::Header {
	let mut header = tar::Header::new_old();
	header.set_path(path).unwrap();
	header.set_mode(0o644);
	header.set_uid(0);
	header.set_gid(0);
	header.set_size(len as u64);
	header.set_cksum();
	header
}

async fn build_tar(caddyfile: &str, dockerfile: &str) -> anyhow::Result<Vec<u8>> {
	let mut tar_buf: Vec<u8> = Vec::new();
	let mut tar = tar::Builder::new(&mut tar_buf);

	// write the Caddyfile file
	let bytes = caddyfile.as_bytes();
	let header = tar_header("Caddyfile", bytes.len());
	tar.append(&header, Cursor::new(bytes))?;

	// write the Dockerfile file
	let bytes = dockerfile.as_bytes();
	let header = tar_header("Dockerfile", bytes.len());
	tar.append(&header, Cursor::new(bytes))?;

	// finish the tar archive
	tar.finish()?;
	drop(tar);
	Ok(tar_buf)
}
