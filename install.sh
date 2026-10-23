#!/bin/sh
# Install Jelly for this user: the program in ~/.local/bin, and its launcher
# entry, icon and AppStream metadata in ~/.local/share (PREFIX=DIR installs
# into DIR instead). From the source code it builds Jelly first; from a
# release's tarball it installs what came in it. Run it again to update.
set -eu
cd "$(dirname "$0")"
prefix=${PREFIX:-$HOME/.local}
id=io.github.joaotroxo.Jelly
if [ -f Cargo.toml ]; then
    cargo build --release --locked
    program=target/release/jelly
    desktop=data/$id.desktop
    icon=data/$id.svg
    metainfo=data/$id.metainfo.xml
else
    program=bin/jelly
    desktop=share/applications/$id.desktop
    icon=share/icons/hicolor/scalable/apps/$id.svg
    metainfo=share/metainfo/$id.metainfo.xml
fi
install -Dm755 "$program" "$prefix/bin/jelly"
install -Dm644 "$icon" "$prefix/share/icons/hicolor/scalable/apps/$id.svg"
install -Dm644 "$metainfo" "$prefix/share/metainfo/$id.metainfo.xml"
# With the full path: the desktop's PATH may not have $prefix/bin in it
sed "s|^Exec=jelly|Exec=$prefix/bin/jelly|" "$desktop" \
    | install -Dm644 /dev/stdin "$prefix/share/applications/$id.desktop"
gtk-update-icon-cache -qtf "$prefix/share/icons/hicolor" 2>/dev/null || true
update-desktop-database -q "$prefix/share/applications" 2>/dev/null || true
echo "Installed $prefix/bin/jelly"
