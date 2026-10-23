#!/bin/sh
# Make the Windows release: target/windows/Jelly, a folder that runs on any
# 64-bit Windows 10 or 11 without installing anything (the app, every DLL it
# loads and the data GTK looks for next to them, with their licenses), and
# target/windows/jelly-VERSION-windows-x86_64.zip of it. Run it in an MSYS2
# UCRT64 shell after cargo build --release.
#
#   pacman -S zip mingw-w64-ucrt-x86_64-{ntldd,librsvg,webp-pixbuf-loader}
#   tools/windows-bundle.sh
set -eu
cd "$(dirname "$0")/.."
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
out=target/windows/Jelly
sources=target/windows/sources.txt
rm -rf target/windows
mkdir -p "$out/lib" "$out/share/glib-2.0/schemas" "$out/share/icons/hicolor/scalable/apps" "$out/etc"

# Copy something of MSYS2's, remembering it for its license
take() {
    cp -r "$MINGW_PREFIX/$1" "$out/$2"
    echo "$MINGW_PREFIX/$1" >> "$sources"
}

cp target/release/jelly.exe "$out/"
# Loaded by name rather than linked: the image loaders (SVG icons, WebP posters)
take lib/gdk-pixbuf-2.0 lib/
# What GLib, GTK and fontconfig look for in share/ and etc/ next to their
# DLLs: settings schemas, icons, the font setup
take share/glib-2.0/schemas/gschemas.compiled share/glib-2.0/schemas/
take share/icons/Adwaita share/icons/
rm -rf "$out/share/icons/Adwaita/cursors"
take share/icons/hicolor/index.theme share/icons/hicolor/
take etc/fonts etc/
cp data/io.github.joaotroxo.Jelly.svg "$out/share/icons/hicolor/scalable/apps/"

# Every DLL all of that needs, from MSYS2's (Windows' own stay out)
find "$out" -name '*.exe' -o -name '*.dll' | while read -r f; do ntldd -R "$f"; done \
    | tr '\\' / | grep -io "[^ ]*/${MINGW_PREFIX#/}/bin/[^ ]*\.dll" | sort -fu \
    | while read -r dll; do
        cp -n "$(cygpath -u "$dll")" "$out/"
        cygpath -u "$dll" >> "$sources"
    done

# The licenses of the MSYS2 packages all that came from, and where their
# source code is
mkdir "$out/licenses"
sort -u "$sources" | while read -r f; do pacman -Qqo "$f"; done | sort -u > target/windows/packages.txt
{
    echo "Jelly for Windows includes these MSYS2 packages, unchanged. Their build"
    echo "recipes are at https://github.com/msys2/MINGW-packages and the source code"
    echo "they were built from at https://repo.msys2.org/mingw/sources/."
    echo
    xargs pacman -Q < target/windows/packages.txt
} > "$out/licenses/README.txt"
while read -r package; do
    name=${package#"$MINGW_PACKAGE_PREFIX"-}
    if [ -d "$MINGW_PREFIX/share/licenses/$name" ]; then
        cp -r "$MINGW_PREFIX/share/licenses/$name" "$out/licenses/"
    fi
done < target/windows/packages.txt
cp LICENSE "$out/LICENSE.txt"
cat > "$out/README.txt" <<EOF
Jelly $version for Windows

Run jelly.exe. Keep the other files next to it: they're the libraries and data
it needs. Settings and sign-ins are kept in %LOCALAPPDATA%\\jelly.

Jelly is free software under the GNU General Public License, version 3 or
later (LICENSE.txt). The licenses of what it includes are in licenses\\.
https://github.com/JoaoTRoxo/jelly
EOF

(cd target/windows && zip -qr "jelly-$version-windows-x86_64.zip" Jelly)
echo "target/windows/jelly-$version-windows-x86_64.zip: $(du -sh "$out" | cut -f1) unzipped"
