use crate::{config::*, docker::IPv6CIDR};
use askama::Template;
use chrono::NaiveDate;
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
			alpine: &self.alpine.version,
			pubkey: &self.alpine.pubkey
		}
	}

	pub fn caddyfile<'a>(&'a self) -> impl Template + 'a {
		#[derive(Template)]
		#[template(path = "caddy/Caddyfile")]
		struct Caddyfile<'t> {
			alpine: &'t str
		}

		Caddyfile {
			alpine: &self.alpine.version
		}
	}

	pub fn caddy_dockerfile<'a>(&'a self) -> impl Template + 'a {
		#[derive(Template)]
		#[template(path = "caddy/Dockerfile")]
		struct Dockerfile;

		Dockerfile
	}

	pub fn packages_dockerfile_abuild<'a>(&'a self, jobs: u16) -> impl Template + 'a {
		#[derive(Template)]
		#[template(path = "packages/abuild.Dockerfile")]
		struct DockerfileAbuild<'t> {
			alpine: &'t str,
			pubkey: &'t str,
			privkey: &'t str,
			jobs: u16
		}

		DockerfileAbuild {
			alpine: &self.alpine.version,
			pubkey: &self.alpine.pubkey,
			privkey: &self.alpine.privkey,
			jobs
		}
	}

	pub fn package_llvm_apkbuild<'a>(&'a self, llvm: &'a PackageLLVM) -> impl Template + 'a {
		#[derive(Template)]
		#[template(path = "packages/llvm.APKBUILD")]
		struct LLVMApkbuild<'t> {
			pkgver: &'t str,
			pkgrel: u32,
			paxmark: bool,
			sha512sum: &'t str
		}

		LLVMApkbuild {
			pkgver: &llvm.pkgver,
			pkgrel: llvm.pkgrel,
			paxmark: llvm.paxmark,
			sha512sum: &llvm.sha512sum
		}
	}

	pub fn rust_dockerfile_abuild<'a>(&'a self, channel: &str, jobs: u16) -> impl Template + 'a {
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
			alpine: &self.alpine.version,
			pubkey: &self.alpine.pubkey,
			privkey: &self.alpine.privkey,
			sysver: self.rust[channel].sysver.as_deref(),
			jobs
		}
	}

	pub fn rust_dockerfile_default<'a>(&'a self, channel: &'a str) -> impl Template + 'a {
		#[derive(Template)]
		#[template(path = "rust/default.Dockerfile")]
		struct DockerfileDefault<'t> {
			alpine: &'t str,
			pubkey: &'t str,
			channel: &'t str
		}

		DockerfileDefault {
			alpine: &self.alpine.version,
			pubkey: &self.alpine.pubkey,
			channel
		}
	}

	pub fn rust_dockerfile_minimal<'a>(&'a self, channel: &'a str) -> impl Template + 'a {
		#[derive(Template)]
		#[template(path = "rust/minimal.Dockerfile")]
		struct DockerfileMinimal<'t> {
			alpine: &'t str,
			pubkey: &'t str,
			channel: &'t str
		}

		DockerfileMinimal {
			alpine: &self.alpine.version,
			pubkey: &self.alpine.pubkey,
			channel
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
			alpine: &self.alpine.version,
			pubkey: &self.alpine.pubkey,
			cidr_v6
		}
	}

	pub fn rust_apkbuild<'a>(&'a self, channel: &'a str) -> impl Template + 'a {
		#[derive(Template)]
		#[template(path = "rust/APKBUILD")]
		struct ApkbuildTemplate<'t> {
			channel: &'t str,
			pkgver: &'t str,
			pkgrel: u32,
			date: Option<&'t NaiveDate>,
			llvmver: u32,
			bootver: &'t str,
			bootsys: bool,
			sysver: Option<&'t str>,
			python: Option<&'t str>,
			sha512sums: &'t str
		}

		let rust: &'a Rust = &self.rust[channel];
		ApkbuildTemplate {
			channel,
			pkgver: &rust.pkgver,
			pkgrel: rust.pkgrel,
			date: rust.date.as_ref(),
			llvmver: rust.llvmver,
			bootver: &rust.bootver,
			bootsys: rust.bootsys,
			sysver: rust.sysver.as_deref(),
			python: rust.python.as_deref(),
			sha512sums: &rust.sha512sums
		}
	}
}
