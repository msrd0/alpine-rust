use crate::{config::*, server::IPv6CIDR};
use askama::Template;
use std::fmt::{self, Display};

struct Rustver {
	rustminor: u32
}

impl Display for Rustver {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "1.{}", self.rustminor)
	}
}

impl Config {
	pub fn index_html<'a>(&'a self) -> impl Template + 'a {
		#[derive(Template)]
		#[template(path = "index.html")]
		struct IndexHtmlTemplate<'t> {
			alpine: &'t str,
			pubkey: &'t str
		}

		IndexHtmlTemplate {
			alpine: &self.alpine,
			pubkey: &self.pubkey
		}
	}

	pub fn caddyfile<'a>(&'a self) -> impl Template + 'a {
		#[derive(Template)]
		#[template(path = "caddy/Caddyfile")]
		struct Caddyfile<'t> {
			alpine: &'t str
		}

		Caddyfile { alpine: &self.alpine }
	}

	pub fn caddy_dockerfile<'a>(&'a self) -> impl Template + 'a {
		#[derive(Template)]
		#[template(path = "caddy/Dockerfile")]
		struct Dockerfile;

		Dockerfile
	}

	pub fn rust_dockerfile_abuild<'a>(&'a self, ver: &'a Version, jobs: u16) -> impl Template + 'a {
		#[derive(Template)]
		#[template(path = "rust/abuild.Dockerfile")]
		struct DockerfileAbuild<'t> {
			alpine: &'t str,
			pubkey: &'t str,
			privkey: &'t str,
			sysver: Option<&'t str>,
			jobs: u16
		}

		DockerfileAbuild {
			alpine: &self.alpine,
			pubkey: &self.pubkey,
			privkey: &self.privkey,
			sysver: ver.sysver.as_deref(),
			jobs
		}
	}

	pub fn rust_dockerfile_default<'a>(&'a self, ver: Option<&'a Version>) -> impl Template + 'a {
		#[derive(Template)]
		#[template(path = "rust/default.Dockerfile")]
		struct DockerfileDefault<'t> {
			alpine: &'t str,
			pubkey: &'t str,
			rustver: Option<Rustver>
		}

		DockerfileDefault {
			alpine: &self.alpine,
			pubkey: &self.pubkey,
			rustver: ver.map(|ver| Rustver {
				rustminor: ver.rustminor
			})
		}
	}

	pub fn rust_dockerfile_minimal<'a>(&'a self, ver: Option<&'a Version>) -> impl Template + 'a {
		#[derive(Template)]
		#[template(path = "rust/minimal.Dockerfile")]
		struct DockerfileMinimal<'t> {
			alpine: &'t str,
			pubkey: &'t str,
			rustver: Option<Rustver>
		}

		DockerfileMinimal {
			alpine: &self.alpine,
			pubkey: &self.pubkey,
			rustver: ver.map(|ver| Rustver {
				rustminor: ver.rustminor
			})
		}
	}

	pub fn rust_dockerfile_test<'a, P: Display>(&'a self, cidr_v6: &'a IPv6CIDR<P>) -> impl Template + 'a {
		#[derive(Template)]
		#[template(path = "rust/test.Dockerfile")]
		struct DockerfileTest<'t, P: Display> {
			alpine: &'t str,
			pubkey: &'t str,
			cidr_v6: &'t IPv6CIDR<P>
		}

		DockerfileTest {
			alpine: &self.alpine,
			pubkey: &self.pubkey,
			cidr_v6
		}
	}
}

impl Version {
	pub fn apkbuild<'a>(&'a self) -> impl Template + 'a {
		#[derive(Template)]
		#[template(path = "rust/APKBUILD")]
		struct ApkbuildTemplate<'t> {
			rustminor: u32,
			rustpatch: u32,
			pkgrel: u32,
			llvmver: u32,
			bootver: &'t str,
			bootsys: bool,
			sysver: Option<&'t str>,
			python: Option<&'t str>,
			sha512sums: &'t str
		}

		ApkbuildTemplate {
			rustminor: self.rustminor,
			rustpatch: self.rustpatch,
			pkgrel: self.pkgrel,
			llvmver: self.llvmver,
			bootver: &self.bootver,
			bootsys: self.bootsys,
			sysver: self.sysver.as_deref(),
			python: self.python.as_deref(),
			sha512sums: &self.sha512sums
		}
	}
}
