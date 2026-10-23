# Contributing to Jelly

Thanks for helping. Bug reports and ideas go in
[GitHub Issues](https://github.com/JoaoTRoxo/jelly/issues); for anything
bigger than a fix, open an issue first so we can agree on the approach before
you spend time on it. Security problems go through [SECURITY.md](SECURITY.md)
instead, privately.

## Building

You need Rust (the version in `rust-toolchain.toml`, which rustup installs by
itself), and GTK 4.16+, libadwaita 1.6+ and libmpv with their development
files: see the [README](README.md#linux) for the packages.

```sh
cargo run                      # the app
cargo run -- --play FILE       # play a local file, without a server
cargo test
```

`JELLY_MPV_LOG=all=v` prints mpv's log, and `JELLY_MPV_OPTIONS="sid=1,hwdec=no"`
passes mpv options of your own.

`tools/screenshot.sh NAME [ARGS]` saves `target/screenshots/NAME.png` from a
private GTK Broadway display, without touching your desktop (its options are
in `src/shot.rs`, which only the `screenshot` feature builds). Broadway has
no OpenGL, so it can't show video.

On Windows, build in an [MSYS2](https://www.msys2.org) UCRT64 shell:

```sh
pacman -S zip mingw-w64-ucrt-x86_64-{rust,gtk4,libadwaita,mpv,pkgconf,ntldd,librsvg,webp-pixbuf-loader}
cargo build --release
tools/windows-bundle.sh        # target/windows/Jelly, which runs outside MSYS2
```

## The code

- `src/api.rs`: the Jellyfin API client (blocking; the UI calls it off the
  main thread through `ui::run`)
- `src/seerr.rs`: the Seerr client
- `src/mpv.rs`: the libmpv bindings, its client API and its OpenGL render API
- `src/cli.rs`: the command line
- `src/session.rs`: the saved sign-ins and settings
- `src/images.rs`: posters, fetched off the main thread and cached
- `src/ui/`: the window and its pages; `src/ui/player/` is the player and
  `src/ui/request/` the Seerr pages

Keep files under about 1,000 lines and functions under about 100: split them
by what they do when they grow. Follow what the code around a change already
does, in naming, comments and how it's put together.

## Checks

CI runs these on every pull request, and they need to pass:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-features --locked
```

It also builds with the oldest supported Rust (`rust-version` in
`Cargo.toml`), validates the desktop entry and AppStream metadata, and builds
and starts the Linux and Windows packages. For a change you can see, also run
the app and try it; say in the pull request what you tried, and on what
(distribution or Windows version, desktop, GPU).

## Commits and pull requests

- One topic per pull request, on its own branch.
- Commit messages and pull request titles follow
  [Conventional Commits](https://www.conventionalcommits.org): `feat(player):
  …`, `fix(windows): …`, `refactor(request): …`, `docs: …`, `build: …`,
  `ci: …`, `chore(deps): …`. The summary says what changes, in the present
  tense; the body says why, when it isn't obvious.
- Reference the issue: `Closes #N` when the pull request finishes it,
  `Refs #N` otherwise.
- Add what users would notice to `CHANGELOG.md`, under Unreleased.

## Releases

1. In a pull request, move CHANGELOG.md's Unreleased entries under a new
   `## [X.Y.Z] - YYYY-MM-DD` heading (and update the links at its end), set
   `version` in `Cargo.toml` (then `cargo build`, for `Cargo.lock`), and add
   the release to `data/io.github.joaotroxo.Jelly.metainfo.xml`.
   `tools/release-check.sh vX.Y.Z` checks they agree.
2. After it's merged, tag that commit on `main` with an annotated tag, and
   push it:

   ```sh
   git tag -a vX.Y.Z -m "Jelly X.Y.Z"
   git push origin vX.Y.Z
   ```

3. The [Release workflow](.github/workflows/release.yml) checks the version
   again, runs the checks, builds the Linux tarball and the Windows zip,
   starts them, and publishes them with their SHA-256 checksums and the
   changelog's notes. Running the workflow by hand builds the packages
   without publishing.

## License

By contributing, you agree that your contributions are licensed under the
[GNU General Public License](LICENSE), version 3 or later, like the rest of
Jelly.
