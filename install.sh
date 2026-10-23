#!/bin/sh
# Install Jelly for this user: the program in ~/.local/bin, and its launcher
# entry, icon, AppStream metadata and shell completions in ~/.local/share
# (PREFIX=DIR installs into DIR instead). From the source code it builds Jelly first; from a
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
    bash=data/completions/jelly.bash
    zsh=data/completions/_jelly
    fish=data/completions/jelly.fish
else
    program=bin/jelly
    desktop=share/applications/$id.desktop
    icon=share/icons/hicolor/scalable/apps/$id.svg
    metainfo=share/metainfo/$id.metainfo.xml
    bash=share/bash-completion/completions/jelly
    zsh=share/zsh/site-functions/_jelly
    fish=share/fish/vendor_completions.d/jelly.fish
fi
install -Dm755 "$program" "$prefix/bin/jelly"
install -Dm644 "$icon" "$prefix/share/icons/hicolor/scalable/apps/$id.svg"
install -Dm644 "$metainfo" "$prefix/share/metainfo/$id.metainfo.xml"
install -Dm644 "$bash" "$prefix/share/bash-completion/completions/jelly"
install -Dm644 "$zsh" "$prefix/share/zsh/site-functions/_jelly"
install -Dm644 "$fish" "$prefix/share/fish/vendor_completions.d/jelly.fish"
# With the full path: the desktop's PATH may not have $prefix/bin in it
sed "s|^Exec=jelly|Exec=$prefix/bin/jelly|" "$desktop" \
    | install -Dm644 /dev/stdin "$prefix/share/applications/$id.desktop"
gtk-update-icon-cache -qtf "$prefix/share/icons/hicolor" 2>/dev/null || true
update-desktop-database -q "$prefix/share/applications" 2>/dev/null || true
echo "Installed $prefix/bin/jelly"
# bash and fish look in ~/.local/share by themselves; zsh doesn't
case $prefix:${SHELL:-} in
/usr:* | /usr/local:*) ;;
*/zsh) echo "For completion in zsh, add fpath+=($prefix/share/zsh/site-functions) to ~/.zshrc, before compinit" ;;
esac
