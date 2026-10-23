#!/bin/sh
# Check that a release's version agrees everywhere: the tag (vX.Y.Z),
# Cargo.toml, CHANGELOG.md's dated section for it, and the AppStream
# metadata's release. Prints that CHANGELOG section, the release's notes.
#
#   tools/release-check.sh v0.1.0 > notes.md
set -eu
cd "$(dirname "$0")/.."
tag=$1
version=${tag#v}
fail() {
    echo "release-check: $*" >&2
    exit 1
}
case $tag in
    v[0-9]*.[0-9]*.[0-9]*) ;;
    *) fail "$tag isn't a version tag like v1.2.3" ;;
esac
cargo=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
[ "$cargo" = "$version" ] || fail "Cargo.toml says $cargo, the tag $version"
grep -A1 '^name = "jelly"$' Cargo.lock | grep -q "^version = \"$version\"$" \
    || fail "Cargo.lock isn't at $version: run cargo build"
grep -q "^## \[$version\] - [0-9]\{4\}-[0-9]\{2\}-[0-9]\{2\}$" CHANGELOG.md \
    || fail "CHANGELOG.md has no '## [$version] - YYYY-MM-DD' section"
grep -q "<release version=\"$version\" date=\"[0-9-]*\"" data/io.github.joaotroxo.Jelly.metainfo.xml \
    || fail "data/io.github.joaotroxo.Jelly.metainfo.xml has no release $version"
# The section, without its heading and the blank lines around it
awk -v heading="## [$version]" '
    index($0, heading) == 1 { inside = 1; next }
    inside && /^## \[/ { exit }
    inside && /^\[/ { exit }
    inside { print }
' CHANGELOG.md | sed -e '/./,$!d' | sed -e ':a' -e '/^\n*$/{$d;N;ba' -e '}'
