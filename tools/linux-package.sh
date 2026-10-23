#!/bin/sh
# Make the Linux release tarball, target/package/jelly-VERSION-linux-ARCH.tar.gz:
# bin/jelly, its launcher entry, icon, AppStream metadata and shell
# completions under share/,
# the documentation, and install.sh to install it all into ~/.local. It uses
# the system's GTK, libadwaita and libmpv (see the README for versions).
#
#   tools/linux-package.sh
set -eu
cd "$(dirname "$0")/.."
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
name=jelly-$version-linux-$(uname -m)
stage=target/package/$name
id=io.github.joaotroxo.Jelly
rm -rf target/package
cargo build --release --locked
install -Dm755 target/release/jelly "$stage/bin/jelly"
install -Dm644 data/$id.desktop "$stage/share/applications/$id.desktop"
install -Dm644 data/$id.svg "$stage/share/icons/hicolor/scalable/apps/$id.svg"
install -Dm644 data/$id.metainfo.xml "$stage/share/metainfo/$id.metainfo.xml"
install -Dm644 data/completions/jelly.bash "$stage/share/bash-completion/completions/jelly"
install -Dm644 data/completions/_jelly "$stage/share/zsh/site-functions/_jelly"
install -Dm644 data/completions/jelly.fish "$stage/share/fish/vendor_completions.d/jelly.fish"
install -Dm644 -t "$stage/share/doc/jelly" README.md CHANGELOG.md LICENSE
install -Dm755 install.sh "$stage/install.sh"
tar -C target/package -czf "target/package/$name.tar.gz" "$name"
echo "target/package/$name.tar.gz"
