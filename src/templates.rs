use crate::{build::packages::Package, config::*, docker::IPv6CIDR};
use askama::Template;
use chrono::NaiveDate;
use std::fmt::{self, Display};

const GIT_COMMIT: &str = env!("GIT_COMMIT");

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

	pub fn package_crate_apkbuild<'a>(&'a self, krate: &'a PackageCrate) -> impl Template + 'a {
		#[derive(Template)]
		#[template(path = "packages/crate.APKBUILD")]
		struct CrateApkbuild<'t> {
			crate_name: &'t str,
			version: &'t str,
			pkgrel: u32,
			description: &'t str,
			license: &'t str,
			check: bool,
			dependencies: &'t [String],
			sha512sum: &'t str
		}

		CrateApkbuild {
			crate_name: &krate.crate_name,
			version: &krate.version,
			pkgrel: krate.pkgrel,
			description: &krate.description,
			license: &krate.license,
			check: krate.check,
			dependencies: &krate.dependencies,
			sha512sum: &krate.sha512sum
		}
	}

	pub fn package_crate_dockerfile<'a>(&'a self, krate: &'a PackageCrate) -> impl Template + 'a {
		#[derive(Template)]
		#[template(path = "packages/crate.Dockerfile")]
		struct CrateDockerfile<'t> {
			alpine: &'t str,
			pubkey: &'t str,
			crate_name: &'t str,
			pkgname: String,
			git_commit: &'t str
		}

		CrateDockerfile {
			alpine: &self.alpine.version,
			pubkey: &self.alpine.pubkey,
			crate_name: &krate.crate_name,
			pkgname: krate.pkgname(),
			git_commit: GIT_COMMIT
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
			channel: &'t str,
			git_commit: &'t str
		}

		DockerfileDefault {
			alpine: &self.alpine.version,
			pubkey: &self.alpine.pubkey,
			channel,
			git_commit: GIT_COMMIT
		}
	}

	pub fn rust_dockerfile_minimal<'a>(&'a self, channel: &'a str) -> impl Template + 'a {
		#[derive(Template)]
		#[template(path = "rust/minimal.Dockerfile")]
		struct DockerfileMinimal<'t> {
			alpine: &'t str,
			pubkey: &'t str,
			channel: &'t str,
			git_commit: &'t str
		}

		DockerfileMinimal {
			alpine: &self.alpine.version,
			pubkey: &self.alpine.pubkey,
			channel,
			git_commit: GIT_COMMIT
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
			python: rust.python.as_deref(),
			sha512sums: &rust.sha512sums
		}
	}
}
