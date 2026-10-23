# AGENTS.md: Jelly

Jelly is a GTK 4 / libadwaita Jellyfin client in Rust that plays everything
directly with libmpv's render API. [CONTRIBUTING.md](CONTRIBUTING.md) has the
layout of the code, the checks, the commit style and the release steps; this
file adds what's easy to get wrong.

## Invariants

- The Jellyfin and Seerr clients block: call them through `ui::run`,
  `ui::run_result` or `gio::spawn_blocking`, never on the main thread.
- mpv's render context (`mpv::Renderer`) is created, used and dropped with
  the GLArea's OpenGL context current, and before its `Mpv` goes away.
  mpv's callbacks run on its own threads: they only wake the main loop.
- Never print, log or put in an error message the Jellyfin token or Seerr's
  cookie. They go only to their own server; mpv gets the token only through
  `http-header-fields`, and checks certificates (`tls-verify`).
- Windows builds with MSYS2's UCRT64 packages. Keep Windows-only code behind
  `cfg(windows)` (and the EGL/X11/Wayland code behind its Unix `cfg`), and
  remember that CI on Linux doesn't compile it: the Windows package job does.
- Files stay under about 1,000 lines and functions under about 100.

## Checks

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-features --locked
```

`cargo run -- --play FILE` plays a local file without a server.
`tools/screenshot.sh NAME [ARGS]` renders the app on a private Broadway
display (no OpenGL there, so no video). Don't test in a nested Wayland
compositor: some stop the real session when they exit.

## Scope

Keep a change to what was asked. Pre-1.0, a clean break beats compatibility
code, but say so in CHANGELOG.md when settings, files or the command line
change.
