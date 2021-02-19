# -*- mode: Shell-script; eval: (setq indent-tabs-mode 't); eval: (setq tab-width 4) -*-

# Maintainer: Dominic Meiser <alpine@msrd0.de>

_crate={{ crate_name }}
pkgname=$(printf ${_crate} | tr '_' '-' | tr '[:upper:]' '[:lower:]')
pkgver={{ version }}
pkgrel={{ pkgrel }}
pkgdesc="{{ description }}"
url=https://crates.io/crate/$_crate
arch="x86_64"
license="{{ license }}"
depends=""
case $_crate in cargo-*)
	depends="$depends cargo"
esac
makedepends="cargo-stable"
source="$_crate-$pkgver.tar.gz::https://crates.io/api/v1/crates/$pkgname/$pkgver/download"
sha512sums="{{ sha512sum }}"
builddir="$srcdir/$_crate-$pkgver"

{%- if !check %}
# this crate does not seem to ship test code with its crates.io releases
options="!check"
{%- endif %}

# search through common -sys crates and add the necessary dependencies
{% for dep in dependencies -%}
case "{{ dep }}" in
	libgit2-sys)
		makedepends="$makedepends libgit2-dev"
		export LIBGIT2_SYS_USE_PKG_CONFIG=1
		;;
	libsqlite3-sys)
		makedepends="$makedepends sqlite-dev"
		;;
	libssh2-sys)
		makedepends="$makedepends libssh2-dev"
		export LIBSSH2_SYS_USE_PKG_CONFIG=1
		;;
	libz-sys)
		makedepends="$makedepends zlib-dev"
		;;
	mysqlclient-sys)
		makedepends="$makedepends mariadb-connector-c-dev"
		;;
	openssl-sys)
		makedepends="$makedepends openssl-dev"
		;;
	pq-sys)
		makedepends="$makedepends postgresql-dev"
		;;
esac
{%- endfor %}

prepare() {
	default_prepare
	
	# turn on lto and minimize size
	for file in $(find . -name Cargo.toml -type f)
	do
		sed -i -e '/^opt-level/d' -e '/^lto/d' "$file"
		echo '[profile.release]' >>"$file"
		echo 'opt-level = "z"' >>"$file"
		echo 'lto = true' >>"$file"
	done
}

build() {
	_locked=
	[ -e Cargo.lock ] && _locked=--locked
	
	cargo build $_locked --workspace --release
}

check() {
	_locked=
	[ -e Cargo.lock ] && _locked=--locked
	
	cargo test $_locked --workspace --release
}

package() {
	_locked=
	[ -e Cargo.lock ] && _locked=--locked
	
	cargo install $_locked --path . --root "$pkgdir/usr" --no-track
	
	# copy any sort of license files found in the crate
	for file in $(ls | grep -i -e license -e copying -e copyright)
	do
		install -Dm644 "$file" -t "$pkgdir/usr/share/licenses/$pkgname"
	done
}
