use crate::config::*;
use askama::Template;

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

	pub fn dockerfile_abuild<'a>(&'a self, ver: &'a Version, jobs: u16) -> impl Template + 'a {
		#[derive(Template)]
		#[template(path = "abuild.Dockerfile")]
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

	pub fn dockerfile_default<'a>(&'a self, ver: &'a Version) -> impl Template + 'a {
		#[derive(Template)]
		#[template(path = "default.Dockerfile")]
		struct DockerfileDefault<'t> {
			alpine: &'t str,
			pubkey: &'t str,
			rustver: String
		}

		DockerfileDefault {
			alpine: &self.alpine,
			pubkey: &self.pubkey,
			rustver: format!("1.{}", ver.rustminor)
		}
	}

	pub fn dockerfile_minimal<'a>(&'a self, ver: &'a Version) -> impl Template + 'a {
		#[derive(Template)]
		#[template(path = "minimal.Dockerfile")]
		struct DockerfileMinimal<'t> {
			alpine: &'t str,
			pubkey: &'t str,
			rustver: String
		}

		DockerfileMinimal {
			alpine: &self.alpine,
			pubkey: &self.pubkey,
			rustver: format!("1.{}", ver.rustminor)
		}
	}
}

impl Version {
	pub fn apkbuild<'a>(&'a self) -> impl Template + 'a {
		#[derive(Template)]
		#[template(path = "APKBUILD")]
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
