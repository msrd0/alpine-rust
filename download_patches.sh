#!/bin/bash
set -e

cd "$(dirname "$0")"

rustver="$1"
commitsha="$2"
if test -z "$rustver" || test -z "$commitsha"; then
	echo "Usage: $0 <rust-version> <commit-sha>"
fi

test ! -d "$rustver" || git rm -r "$rustver" || rm -r "$rustver"
mkdir -p "patches-$rustver"

tmpfile=$(mktemp)
url="https://gitlab.alpinelinux.org/alpine/aports/-/archive/$commitsha/aports-$commitsha.tar.bz2?path=community/rust"
wget -qO $tmpfile "$url"
tar xfj $tmpfile -C "patches-$rustver" --strip-components=3 --wildcards '*.patch'
rm $tmpfile

branch="$(git rev-parse --abbrev-ref HEAD)"
git checkout --orphan "patches/$rustver"
git add "patches-$rustver"/*.patch
git commit "patches-$rustver"/*.patch -F - << EOF
Import patches for $rustver
Imported from: $url
EOF
echo "branch 'patches/$rustver' created"
git checkout "$branch"