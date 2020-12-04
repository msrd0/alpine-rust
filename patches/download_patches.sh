#!/bin/bash
set -e

cd "$(dirname "$0")"

rustver="$1"
commitsha="$2"
if test -z "$rustver" || test -z "$commitsha"; then
	echo "Usage: $0 <rust-version> <commit-sha>"
fi

test ! -d "$rustver" || git rm -r "$rustver" || rm -r "$rustver"
mkdir -p "$rustver"

tmpfile=$(mktemp)
url="https://gitlab.alpinelinux.org/alpine/aports/-/archive/$commitsha/aports-$commitsha.tar.bz2?path=community/rust"
wget -qO $tmpfile "$url"
tar xfj $tmpfile -C "$rustver" --strip-components=3 --wildcards '*.patch'
rm $tmpfile

git add "$rustver"/*.patch
git commit "$rustver"/*.patch -F - << EOF
Import patches for $rustver
Imported from: $url

There were no changes that need recompilation, so [skip ci]
EOF
