# -*- mode: Shell-script; eval: (setq indent-tabs-mode 't); eval: (setq tab-width 4) -*-

# Adopted from the official aports
# Contributor: Rasmus Thomsen <oss@cogitri.dev>
# Contributor: Jakub Jirutka <jakub@jirutka.cz>
# Contributor: Shiz <hi@shiz.me>
# Contributor: Jeizsm <jeizsm@gmail.com>
# Maintainer: Dominic Meiser <alpine@msrd0.de>
_aportsha={{ aportsha }}
_rustver={{ rustver }}
pkgname=rust-$_rustver
pkgver={{ pkgver }}
_llvmver=10
_bootver={{ bootver }}
pkgrel=1
pkgdesc="The Rust Programming Language"
url="https://www.rust-lang.org"
arch="x86_64"
license="Apache-2.0 AND MIT"

# gcc is needed at runtime just for linking. Someday rustc might invoke
# the linker directly, and then we'll only need binutils.
# See: https://github.com/rust-lang/rust/issues/11937
depends="$pkgname-stdlib=$pkgver-r$pkgrel gcc musl-dev"

_python=python3
# * Rust is self-hosted, so you need rustc (and cargo) to build rustc...
#   The last revision of this abuild that does not depend on itself (uses
#   prebuilt rustc and cargo) is 2e6769eb39eaff3029d8298fc02856623c563cd8.
makedepends_build="
        $_python
        file
        tar
        coreutils
        llvm$_llvmver-dev
        llvm$_llvmver-test-utils
        {% if bootsys %}rust=>$_bootver{% else %}rust-$_bootver{% endif %}
        {% if bootsys %}cargo=>$_bootver{% else %}cargo-$_bootver{% endif %}
"
makedepends_host="
        curl-dev
        libgit2-dev
        openssl-dev
        llvm$_llvmver-dev
        zlib-dev
"

# This is needed for -src that contains some testing binaries.
# Disable tests for now, while we use foreign triplets
options="!archcheck !check"

subpackages="
        $pkgname-dbg
        $pkgname-stdlib
        $pkgname-analysis
        $pkgname-gdb::noarch
        $pkgname-lldb::noarch
        $pkgname-doc
        $pkgname-src::noarch
        cargo-$_rustver-$pkgver
        cargo-$_rustver-bash-completions:_cargo_bashcomp:noarch
        cargo-$_rustver-zsh-completion:_cargo_zshcomp:noarch
        cargo-$_rustver-doc:_cargo_doc:noarch
        "
source="https://static.rust-lang.org/dist/rustc-$pkgver-src.tar.gz
        https://gitlab.alpinelinux.org/alpine/aports/-/raw/$_aportsha/community/rust/musl-fix-linux_musl_base.patch
        https://gitlab.alpinelinux.org/alpine/aports/-/raw/$_aportsha/community/rust/need-rpath.patch
        https://gitlab.alpinelinux.org/alpine/aports/-/raw/$_aportsha/community/rust/need-ssp_nonshared.patch
        https://gitlab.alpinelinux.org/alpine/aports/-/raw/$_aportsha/community/rust/alpine-move-py-scripts-to-share.patch
        https://gitlab.alpinelinux.org/alpine/aports/-/raw/$_aportsha/community/rust/alpine-target.patch
        https://gitlab.alpinelinux.org/alpine/aports/-/raw/$_aportsha/community/rust/install-template-shebang.patch
        https://gitlab.alpinelinux.org/alpine/aports/-/raw/$_aportsha/community/rust/check-rustc
        https://gitlab.alpinelinux.org/alpine/aports/-/raw/$_aportsha/community/rust/link-musl-dynamically.patch
        https://gitlab.alpinelinux.org/alpine/aports/-/raw/$_aportsha/community/rust/musl-dont-use-crt-static.patch
        https://gitlab.alpinelinux.org/alpine/aports/-/raw/$_aportsha/community/rust/0006-Prefer-libgcc_eh-over-libunwind-for-musl.patch
        "
builddir="$srcdir/rustc-$pkgver-src"

# secfixes:
#   1.34.2-r0:
#     - CVE-2019-12083
#   1.26.0-r0:
#     - CVE-2019-16760

# We have to add new arches in multiple steps:
# 1. Compile with the upstream triplets, compiling alpine's triplets in
# 2. Compile again, now with our triplets selected as build/target, now that
#    rustc knows about them
_build="$CBUILD"
_target="$CTARGET"

_rlibdir="usr/lib/rustlib/$_target/lib"
_sharedir="usr/share/rust"

ldpath="/$_rlibdir"

export RUST_BACKTRACE=1
# Don't use system libgit2 for now...
# https://github.com/rust-lang/rust/issues/63476
# Convince libgit2-sys to use the distro libgit2.
#export LIBGIT2_SYS_USE_PKG_CONFIG=1

# rust checksums files in vendor/, but we have to patch a few files...
_clear_vendor_checksums() {
        sed -i 's/\("files":{\)[^}]*/\1/' vendor/$1/.cargo-checksum.json
}

prepare() {
        #default_prepare

        # patches don't contain the correct prefix directory so we need
        # to patch them ourselves
        for file in $(ls ../*.patch | sort)
        do
                patch -N -p 1 -i $file
        done

        sed -i /LD_LIBRARY_PATH/d src/bootstrap/bootstrap.py

        # to dynamically link against musl
        _clear_vendor_checksums libc

        # Remove bundled dependencies.
        rm -Rf src/llvm-project/
}

build() {
        # Fails to compile libbacktrace-sys otherwise
        case "$CARCH" in
                x86)
                        export CFLAGS="$CFLAGS -fno-stack-protector"
                        ;;
        esac
        if [ "$_build" != "$_target" ]; then
                export PKG_CONFIG_ALLOW_CROSS=1
        fi

        ./configure \
                --build="$_build" \
                --host="$_target" \
                --target="$_target" \
                --prefix="/usr" \
                --release-channel="stable" \
                --enable-local-rust \
                --local-rust-root="/usr" \
                --llvm-root="/usr/lib/llvm$_llvmver" \
                --disable-docs \
                --enable-extended \
                --tools="analysis,cargo,src" \
                --enable-llvm-link-shared \
                --enable-option-checking \
                --enable-locked-deps \
                --enable-vendor \
                --python="$_python" \
                --set="rust.musl-root=/usr" \
                --set="target.$_target.llvm-config=/usr/lib/llvm$_llvmver/bin/llvm-config" \
                --set="target.$_target.musl-root=/usr" \
                --set="target.$_target.crt-static=false" \
                --set="target.$_target.cc=${CROSS_COMPILE}gcc" \
                --set="target.$_target.cxx=${CROSS_COMPILE}c++" \
                --set="target.$_target.ar=${CROSS_COMPILE}ar" \
                --set="target.$_target.linker=${CROSS_COMPILE}gcc" \
                --set="target.$_build.musl-root=/usr" \
                --set="target.$_build.crt-static=false" \
                --set="target.$_build.cc=gcc" \
                --set="target.$_build.cxx=c++" \
                --set="target.$_build.ar=ar" \
                --set="target.$_build.linker=gcc"

        # Allow warnings instead of just aborting the build
        sed 's/#deny-warnings = .*/deny-warnings = false/' -i config.toml
        sed 's|deny(warnings,|deny(|' -i src/bootstrap/lib.rs

        $_python ./x.py build --jobs ${JOBS:-2}
}

check() {
        # At this moment lib/rustlib/$CTARGET/lib does not contain a complete
        # copy of the .so libs from lib (they will be copied there during
        # 'x.py install'). Thus we must set LD_LIBRARY_PATH for tests to work.
        # This is related to change-rpath-to-rustlib.patch.
        export LD_LIBRARY_PATH="$builddir/build/$CTARGET/stage2/lib"

        "$srcdir"/check-rustc "$builddir"/build/$CTARGET/stage2/bin/rustc

# XXX: There's some problem with these tests, we will figure it out later.
#       make check \
#               LD_LIBRARY_PATH="$_stage0dir/lib" \
#               VERBOSE=1

        msg "Running tests for cargo..."
        CFG_DISABLE_CROSS_TESTS=1 $_python ./x.py test --no-fail-fast src/tools/cargo

        unset LD_LIBRARY_PATH
}

package() {
        DESTDIR="$pkgdir" $_python ./x.py install -v

        cd "$pkgdir"

        # Python scripts are noarch, so move them to /usr/share.
        # Requires move-py-scripts-to-share.patch to be applied.
        _mv usr/lib/rustlib/etc/*.py $_sharedir/etc/
        rmdir -p usr/lib/rustlib/etc 2>/dev/null || true

        # Remove some clutter.
        cd usr/lib/rustlib
        rm components install.log manifest-* rust-installer-version uninstall.sh
        if [ "$_build" != "$_target" ]; then
                rm -rf "$pkgdir"/usr/lib/rustlib/$_build
        fi
}

stdlib() {
        pkgdesc="Standard library for Rust (static rlibs)"
        depends=

        _mv "$pkgdir"/$_rlibdir/*.rlib "$subpkgdir"/$_rlibdir/
}

analysis() {
        pkgdesc="Compiler analysis data for the Rust standard library"
        depends="$pkgname-stdlib=$pkgver-r$pkgrel"

        _mv "$pkgdir"/$_rlibdir/../analysis "$subpkgdir"/${_rlibdir%/*}/
}

gdb() {
        pkgdesc="GDB pretty printers for Rust"
        depends="$pkgname=$pkgver-r$pkgrel gdb"

        mkdir -p "$subpkgdir"
        cd "$subpkgdir"

        _mv "$pkgdir"/usr/bin/rust-gdb usr/bin/
        _mv "$pkgdir"/$_sharedir/etc/gdb_*.py $_sharedir/etc/
}

lldb() {
        local _pyver=${_python#python}
        pkgdesc="LLDB pretty printers for Rust"
        depends="$pkgname=$pkgver-r$pkgrel lldb py$_pyver-lldb"

        mkdir -p "$subpkgdir"
        cd "$subpkgdir"

        _mv "$pkgdir"/usr/bin/rust-lldb usr/bin/
        _mv "$pkgdir"/$_sharedir/etc/lldb_*.py $_sharedir/etc/
}

src() {
        pkgdesc="$pkgdesc (source code)"
        depends="$pkgname=$pkgver-r$pkgrel"
        license="$license OFL-1.1 GPL-3.0-or-later GPL-3.0-with-GCC-exception CC-BY-SA-3.0 LGPL-3.0"

        _mv "$pkgdir"/usr/lib/rustlib/src/rust "$subpkgdir"/usr/src/
        rmdir -p "$pkgdir"/usr/lib/rustlib/src 2>/dev/null || true

        mkdir -p "$subpkgdir"/usr/lib/rustlib/src
        ln -s ../../../src/rust "$subpkgdir"/usr/lib/rustlib/src/rust
}

cargo() {
        pkgdesc="The Rust package manager"
        license="Apache-2.0 MIT UNLICENSE"
        depends="$pkgname=$pkgver-r$pkgrel"

        _mv "$pkgdir"/usr/bin/cargo "$subpkgdir"/usr/bin/
}

_cargo_bashcomp() {
        pkgdesc="Bash completions for cargo"
        license="Apache-2.0 MIT"
        depends=""
        install_if="cargo-$_rustver=$pkgver-r$pkgrel bash-completion"

        cd "$pkgdir"
        _mv etc/bash_completion.d/cargo \
                "$subpkgdir"/usr/share/bash-completion/completions/
        rmdir -p etc/bash_completion.d 2>/dev/null || true
}

_cargo_zshcomp() {
        pkgdesc="ZSH completions for cargo"
        license="Apache-2.0 MIT"
        depends=""
        install_if="cargo-$_rustver=$pkgver-r$pkgrel zsh"

        cd "$pkgdir"
        _mv usr/share/zsh/site-functions/_cargo \
                "$subpkgdir"/usr/share/zsh/site-functions/_cargo
        rmdir -p usr/share/zsh/site-functions 2>/dev/null || true
}

_cargo_doc() {
        pkgdesc="The Rust package manager (documentation)"
        license="Apache-2.0 MIT"
        install_if="docs cargo-$_rustver=$pkgver-r$pkgrel"

        # XXX: This is hackish!
        cd "$pkgdir"/../$pkgname-doc
        _mv usr/share/man/man1/cargo* "$subpkgdir"/usr/share/man/man1/
}

_mv() {
        local dest; for dest; do true; done  # get last argument
        mkdir -p "$dest"
        mv "$@"
}

# The SHA512 checksums will be updated by running `abuild checksum`
sha512sums=""
