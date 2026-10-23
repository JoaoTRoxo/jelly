#!/bin/sh
# Make the Windows release: target/windows/Jelly, a folder that runs on any
# 64-bit Windows 10 or 11 without installing anything, and
# target/windows/jelly-VERSION-windows-x86_64.zip of it. Its jelly.exe starts
# the app in runtime\bin, next to every DLL it loads; the data GTK looks for
# is in runtime too, and the licenses of all that in licenses. Run it in an
# MSYS2 UCRT64 shell after cargo build --release.
#
#   pacman -S zip mingw-w64-ucrt-x86_64-{ntldd,librsvg,webp-pixbuf-loader}
#   tools/windows-bundle.sh
set -eu
cd "$(dirname "$0")/.."
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
out=target/windows/Jelly
runtime=$out/runtime
sources=target/windows/sources.txt
rm -rf target/windows
mkdir -p "$runtime/bin" "$runtime/lib" "$runtime/share/glib-2.0/schemas" \
    "$runtime/share/icons/hicolor/scalable/apps" "$runtime/etc"

# Copy something of MSYS2's, remembering it for its license
take() {
    cp -r "$MINGW_PREFIX/$1" "$runtime/$2"
    echo "$MINGW_PREFIX/$1" >> "$sources"
}

# The app in runtime\bin, where GLib, GTK and fontconfig look for their data
# one folder up, and the launcher that starts it on top
cp target/release/jelly.exe "$runtime/bin/"
cargo build --release --locked --bin jelly-launcher --features windows-launcher
cp target/release/jelly-launcher.exe "$out/jelly.exe"
# The launcher can only use Windows' own DLLs: it doesn't see runtime\bin
if ntldd -R "$out/jelly.exe" | tr '\\' / | grep -i "/${MINGW_PREFIX#/}/bin/"; then
    echo "The launcher needs those DLLs of MSYS2's" >&2
    exit 1
fi
# Loaded by name rather than linked: the image loaders (SVG icons, WebP posters)
take lib/gdk-pixbuf-2.0 lib/
# What GLib, GTK and fontconfig look for in share/ and etc/, one folder up
# from their DLLs: settings schemas, icons, the font setup
take share/glib-2.0/schemas/gschemas.compiled share/glib-2.0/schemas/
take share/icons/Adwaita share/icons/
rm -rf "$runtime/share/icons/Adwaita/cursors"
take share/icons/hicolor/index.theme share/icons/hicolor/
take etc/fonts etc/
cp data/io.github.joaotroxo.Jelly.svg "$runtime/share/icons/hicolor/scalable/apps/"

# Every DLL all of that needs, from MSYS2's (Windows' own stay out)
find "$runtime" -name '*.exe' -o -name '*.dll' | while read -r f; do ntldd -R "$f"; done \
    | tr '\\' / | grep -io "[^ ]*/${MINGW_PREFIX#/}/bin/[^ ]*\.dll" | sort -fu \
    | while read -r dll; do
        cp -n "$(cygpath -u "$dll")" "$runtime/bin/"
        cygpath -u "$dll" >> "$sources"
    done

# OpenGL has to be Windows' own, which loads the graphics card's driver: with
# MSYS2's Mesa installed, its opengl32.dll (emulating OpenGL on Direct3D 12 or
# the CPU, with 150 MB of LLVM) would be taken in its place
if [ -e "$runtime/bin/opengl32.dll" ]; then
    echo "The DLLs include Mesa's opengl32.dll: pacman -R mingw-w64-ucrt-x86_64-mesa, then run this again" >&2
    exit 1
fi

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

Run jelly.exe. Keep it in this folder: it starts the app in runtime\\, which
has the libraries and data it needs. Settings and sign-ins are kept in
%LOCALAPPDATA%\\jelly.

Jelly is free software under the GNU General Public License, version 3 or
later (LICENSE.txt). The licenses of what it includes are in licenses\\.
https://github.com/JoaoTRoxo/jelly
EOF

(cd target/windows && zip -qr "jelly-$version-windows-x86_64.zip" Jelly)
echo "target/windows/jelly-$version-windows-x86_64.zip: $(du -sh "$out" | cut -f1) unzipped"
