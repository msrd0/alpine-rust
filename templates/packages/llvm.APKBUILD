# -*- mode: Shell-script; eval: (setq indent-tabs-mode 't); eval: (setq tab-width 4) -*-

# Adopted from the official aports
# Contributor: Rasmus Thomsen <oss@cogitri.dev>
# Contributor: Martell Malone <martell@marinelayer.io>
# Contributor: Travis Tilley <ttilley@gmail.com>
# Contributor: Mitch Tishmack <mitch.tishmack@gmail.com>
# Contributor: Jakub Jirutka <jakub@jirutka.cz>
# Contributor: Ariadne Conill <ariadne@dereferenced.org>
# Maintainer: Dominic Meiser <alpine@msrd0.de>

_pkgname=llvm
pkgver={{ pkgver }}
_majorver=${pkgver%%.*}
pkgname=$_pkgname$_majorver
pkgrel={{ pkgrel }}
pkgdesc="Low Level Virtual Machine compiler system, version $_majorver"
arch="all"
url="https://llvm.org/"
license="Apache-2.0"
depends_dev="$pkgname=$pkgver-r$pkgrel"
makedepends_host="binutils-dev libffi-dev zlib-dev libxml2-dev"
makedepends_build="cmake chrpath python3 py3-setuptools"
{% if paxmark -%}
makedepends_host="$makedepends_host paxmark"
{%- endif %}
# diffutils for diff: unrecognized option: strip-trailing-cr
# coreutils for 'od' binary
checkdepends="bash coreutils diffutils"
subpackages="$pkgname-static $pkgname-libs $pkgname-dev $pkgname-test-utils:_test_utils"
source="https://github.com/llvm/llvm-project/releases/download/llvmorg-$pkgver/llvm-$pkgver.src.tar.xz"
_aports_rev=4280f227d08b8b1ba76efbe14cc4380be4ad4949
_aports_patches="
	0001-Disable-dynamic-lib-tests-for-musl-s-dlclose-is-noop.patch
	fix-memory-mf_exec-on-aarch64.patch
	fix-LLVMConfig-cmake-install-prefix.patch
	python3-test.patch
"
for _patch in $_aports_patches
do
	source="$source ${_aports_rev}_$_patch::https://github.com/alpinelinux/aports/raw/$_aports_rev/main/llvm10/$_patch"
done
builddir="$srcdir/$_pkgname-$pkgver.src"

# If crosscompiling, we need llvm-tblgen on the build machine.
if [ -n "$BOOTSTRAP" ]; then
	makedepends_build="$makedepends_build cmd:llvm-tblgen"
	cmake_cross_options="
		-DCMAKE_CROSSCOMPILING=ON
		-DLLVM_TABLEGEN=/usr/bin/llvm-tblgen
	"
fi

# ARM has few failures in test suite that we don't care about currently and
# also it takes forever to run them on the builder.
# MIPS as well...
case "$CARCH" in
	arm*) options="$options !check";;
	mips*) options="$options !check";;
esac

# NOTE: It seems that there's no (sane) way how to change includedir, sharedir
# etc. separately, just the CMAKE_INSTALL_PREFIX. Standard CMake variables and
# even  LLVM-specific variables, that are related to these paths, actually
# don't work (in llvm 3.7).
#
# When building a software that depends on LLVM, utility llvm-config should be
# used to discover where is LLVM installed. It provides options to print
# path of bindir, includedir, and libdir separately, but in its source, all
# these paths are actually hard-coded against INSTALL_PREFIX. We can patch it
# and move paths manually, but I'm really not sure what it may break...
#
# Also note that we should *not* add version suffix to files in llvm bindir!
# It breaks build system of some software that depends on LLVM, because they
# don't expect these files to have a sufix.
#
# So, we install all the LLVM files into /usr/lib/llvm$_majorver.
# BTW, Fedora and Debian do the same thing.
#
_prefix="usr/lib/llvm$_majorver"

prepare() {
	default_prepare
	mkdir -p "$builddir"/build

	# Known broken test on musl
	rm -v test/CodeGen/AArch64/wineh4.mir
	case "$CARCH" in
		x86) rm -v test/Object/macho-invalid.test;;
	esac
	
	# This test fails in llvm 11: https://bugs.llvm.org/show_bug.cgi?id=48313
	if [ "$_majorver" == 11 ]
	then
		rm -v test/ExecutionEngine/Interpreter/intrinsics.ll
	fi
	
	# Also some Hexagon architecture tests fail
	for file in csr-stubs-spill-threshold.ll long-calls.ll mlong-calls.ll pic-regusage.ll runtime-stkchk.ll swp-memrefs-epilog.ll vararg-formal.ll
	do
		test ! -e test/CodeGen/Hexagon/$file || rm -v test/CodeGen/Hexagon/$file
	done	
}

build() {
	cd "$builddir"/build

	# Auto-detect it by guessing either.
	local ffi_include_dir="$(pkg-config --cflags-only-I libffi | sed 's|^-I||g')"
	case "$CARCH" in
		x86) LDFLAGS="$LDFLAGS -Wl,--no-keep-memory";; # avoid exhausting memory limit
	esac

	cmake -Wno-dev \
		-DCMAKE_BUILD_TYPE=MinSizeRel \
		-DCMAKE_C_FLAGS_MINSIZEREL_INIT="$CFLAGS" \
		-DCMAKE_CXX_FLAGS_MINSIZEREL_INIT="$CXXFLAGS" \
		-DCMAKE_EXE_LINKER_FLAGS_MINSIZEREL_INIT="$LDFLAGS" \
		-DCMAKE_INSTALL_PREFIX=/$_prefix \
		-DFFI_INCLUDE_DIR="$ffi_include_dir" \
		-DLLVM_BINUTILS_INCDIR=/usr/include \
		-DLLVM_BUILD_DOCS=OFF \
		-DLLVM_BUILD_EXAMPLES=OFF \
		-DLLVM_BUILD_EXTERNAL_COMPILER_RT=ON \
		-DLLVM_BUILD_LLVM_DYLIB=ON \
		-DLLVM_BUILD_TESTS=ON \
		-DLLVM_DEFAULT_TARGET_TRIPLE="$CBUILD" \
		-DLLVM_ENABLE_ASSERTIONS=OFF \
		-DLLVM_ENABLE_CXX1Y=ON \
		-DLLVM_ENABLE_FFI=ON \
		-DLLVM_ENABLE_LIBCXX=OFF \
		-DLLVM_ENABLE_PIC=ON \
		-DLLVM_ENABLE_RTTI=ON \
		-DLLVM_ENABLE_SPHINX=OFF \
		-DLLVM_ENABLE_TERMINFO=ON \
		-DLLVM_ENABLE_ZLIB=ON \
		-DLLVM_HOST_TRIPLE="$CHOST" \
		-DLLVM_INCLUDE_EXAMPLES=OFF \
		-DLLVM_LINK_LLVM_DYLIB=ON \
		-DLLVM_APPEND_VC_REV=OFF \
		$cmake_cross_options \
		"$builddir"

	make llvm-tblgen
	make
	
{% if paxmark -%}
	paxmark m \
		bin/llvm-rtdyld \
		bin/lli \
		bin/lli-child-target \
		unittests/ExecutionEngine/MCJIT/MCJITTests \
		unittests/ExecutionEngine/Orc/OrcJITTests \
		unittests/Support/SupportTests
{%- endif %}

	python3 ../utils/lit/setup.py build
}

check() {
	cd "$builddir"/build

	make check-llvm
}

package() {
	cd "$builddir"/build

	make DESTDIR="$pkgdir" install

	cd "$pkgdir"/$_prefix

	# Remove RPATHs.
	file lib/*.so bin/* \
		| awk -F: '$2~/ELF/{print $1}' \
		| xargs -r chrpath -d

	# Symlink files from /usr/lib/llvm*/bin to /usr/bin.
	mkdir -p "$pkgdir"/usr/bin
	local name newname path
	for path in bin/*; do
		name=${path##*/}
		# Add version infix/suffix to the executable name.
		case "$name" in
			llvm-*) newname="llvm$_majorver-${name#llvm-}";;
			*) newname="$name$_majorver";;
		esac
		case "$name" in
			FileCheck | obj2yaml | yaml2obj) continue;;
		esac
		ln -s ../lib/llvm$_majorver/bin/$name "$pkgdir"/usr/bin/$newname
	done

	# Move /usr/lib/$pkgname/include/ into /usr/include/$pkgname/
	# and symlink it back.
	mkdir "$pkgdir"/usr/include/
	mv include "$pkgdir"/usr/include/$pkgname
	ln -s ../../include/$pkgname include

	# Move /usr/lib/$pkgname/lib/cmake/llvm/ into /usr/lib/cmake/$pkgname/
	# and symlink it back.
	mkdir "$pkgdir"/usr/lib/cmake/
	mv lib/cmake/llvm "$pkgdir"/usr/lib/cmake/$pkgname
	ln -s ../../../cmake/$pkgname lib/cmake/llvm
}

static() {
	pkgdesc="LLVM $_majorver static libraries"

	_mv "$pkgdir"/$_prefix/lib/*.a "$subpkgdir"/$_prefix/lib/
}

libs() {
	pkgdesc="LLVM $_majorver runtime library"
	local soname="libLLVM-$_majorver.so"
	local soname2="libLLVM-$pkgver.so"

	mkdir -p "$subpkgdir"
	cd "$subpkgdir"

	# libLLVM should be in /usr/lib. This is needed for binaries that are
	# dynamically linked with libLLVM, so they can find it on default path.
	_mv "$pkgdir"/$_prefix/lib/$soname usr/lib/
	ln -s $soname usr/lib/$soname2

	# And also symlink it back to the LLVM prefix.
	mkdir -p $_prefix/lib
	ln -s ../../$soname $_prefix/lib/$soname
	ln -s ../../$soname $_prefix/lib/$soname2
}

dev() {
	default_dev
	cd "$subpkgdir"

	_mv "$pkgdir"/$_prefix/lib $_prefix/
	_mv "$pkgdir"/$_prefix/include $_prefix/

	_mv "$pkgdir"/$_prefix/bin/llvm-config $_prefix/bin/

	# Move libLTO and LLVMgold back
	_mv "$subpkgdir"/$_prefix/lib/libLTO.so.* \
		"$subpkgdir"/$_prefix/lib/LLVMgold* \
		"$pkgdir"/$_prefix/lib
}

_test_utils() {
	pkgdesc="LLVM $_majorver utilities for executing LLVM and Clang style test suites"
	depends="python3 py3-setuptools"
	replaces=""

	local litver=$(python3 "$builddir"/utils/lit/setup.py --version 2>/dev/null \
		| sed 's/\.dev.*$//')
	test -n "$litver"
	provides="$provides lit=$litver-r$pkgrel"

	cd "$builddir"/build

	install -D -m 755 bin/count "$subpkgdir"/$_prefix/bin/count
	install -D -m 755 bin/FileCheck "$subpkgdir"/$_prefix/bin/FileCheck
	install -D -m 755 bin/not "$subpkgdir"/$_prefix/bin/not

	python3 ../utils/lit/setup.py install --prefix=/usr --root="$subpkgdir"
	ln -s ../../../bin/lit "$subpkgdir"/$_prefix/bin/lit
}


_mv() {
	local dest; for dest; do true; done  # get last argument
	mkdir -p "$dest"
	mv "$@"
}

sha512sums="{{ sha512sum }}
695502bd3b5454c2f5630c59a8cf5f8aeb0deac16a76a8a4df34849e1e35c12ed4234572a320fe4c7e96f974f572f429eb816c5aa3dcfb17057f550eac596495  4280f227d08b8b1ba76efbe14cc4380be4ad4949_0001-Disable-dynamic-lib-tests-for-musl-s-dlclose-is-noop.patch
64b9ecb246cc94ce7f617b3699b3306de0872a1a9b0ade88563330aa6f9a60742bb1d73f95743d0f033ea8b1535e2e612250c8f50bddf4419741ca18f40eca1d  4280f227d08b8b1ba76efbe14cc4380be4ad4949_fix-memory-mf_exec-on-aarch64.patch
7d4825d16107e56a14b7b05be847f03d75e2e05952bea0742a1233b5b0441c9934d8058e612abb6471272884372d9bfd3348355fbd3c19cba82a554003cc3eec  4280f227d08b8b1ba76efbe14cc4380be4ad4949_fix-LLVMConfig-cmake-install-prefix.patch
53cc0d13dd871e9b775bb4e7567de4f9a97d91b8246cd7ce74607fd88d6e3e2ab9455f5b4195bc7f9dbdedbc77d659d43e98ec0b7cd78cd395aaea6919510287  4280f227d08b8b1ba76efbe14cc4380be4ad4949_python3-test.patch"
