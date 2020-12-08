use serde::Deserialize;

#[derive(Deserialize)]
pub struct Config {
	pub alpine: String,
	pub pubkey: String,
	pub privkey: String,
	pub versions: Vec<Version>
}

#[derive(Deserialize)]
pub struct Version {
	pub rustminor: u32,
	pub rustpatch: u32,
	pub pkgrel: u32,
	pub llvmver: u32,
	pub bootver: String,
	pub bootsys: bool,
	pub sysver: Option<String>,
	pub python: Option<String>,
	pub sha512sums: String
}
