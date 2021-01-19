use std::{
	fs::{read_dir, File},
	io::Write,
	path::{Path, PathBuf}
};
use tar::Builder;

fn main() {
	println!("cargo:rerun-if-changed=build.rs");

	let out_dir: PathBuf = std::env::var("OUT_DIR").unwrap().into();
	let archive_path = out_dir.join("simple-compiler-test.tar");
	println!("cargo:rustc-env=SIMPLE_COMPILER_TEST={}", archive_path.display());

	let mut archive_file = File::create(archive_path).unwrap();
	let mut archive = tar::Builder::new(&mut archive_file);
	let dir: PathBuf = "simple-compiler-test".parse().unwrap();
	let archive_dir = PathBuf::new();
	add(&mut archive, &dir, &archive_dir);
}

fn add<W: Write>(archive: &mut Builder<W>, dir: &Path, archive_dir: &Path) {
	if dir.file_name().unwrap().to_str().unwrap() == "target" {
		return;
	}

	if dir.is_file() {
		println!("cargo:rerun-if-changed={}", dir.display());
		let mut file = File::open(dir).unwrap();
		archive.append_file(archive_dir, &mut file).unwrap();
		return;
	}

	for entry in read_dir(dir).unwrap() {
		let entry = entry.unwrap();
		let path = entry.path();
		let archive_path = archive_dir.join(path.file_name().unwrap());
		add(archive, &path, &archive_path);
	}
}
