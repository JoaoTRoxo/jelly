#!/bin/sh
# Screenshot the app without touching the desktop session: it runs on a
# private GTK Broadway display with its own D-Bus, and renders itself to
# target/screenshots/NAME.png (see src/shot.rs for the JELLY_SHOT_* options).
#
#   tools/screenshot.sh home
#   JELLY_SHOT_NO_ANIM=1 JELLY_SHOT_SCROLL=400 tools/screenshot.sh item open ID
#
# Don't use a nested compositor for this: some (mango) stop the real session's
# systemd target when they exit.
set -eu
cd "$(dirname "$0")/.."
name=$1
shift
CARGO_TARGET_DIR=target/screenshot cargo build --quiet --features screenshot
mkdir -p target/screenshots
gtk4-broadwayd --address 127.0.0.1 --port 8087 :7 > target/screenshots/broadwayd.log 2>&1 &
broadway=$!
trap 'kill $broadway 2>/dev/null' EXIT
sleep 1
env GDK_BACKEND=broadway BROADWAY_DISPLAY=:7 GTK_A11Y=none NO_AT_BRIDGE=1 ADW_DISABLE_PORTAL=1 GDK_DEBUG=no-portals GIO_USE_VFS=local \
  JELLY_SHOT="$PWD/target/screenshots/$name.png" \
  timeout 60 dbus-run-session -- target/screenshot/debug/jelly "$@" 2>&1 | grep -v frame_timings || true
