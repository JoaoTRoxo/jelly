# Changelog

All notable changes to Jelly are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and Jelly uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- A button to go back to the previous episode, next to the next episode's
  (or P).
- The audio and subtitles picked in an episode are picked again in the
  show's other episodes: the same language, and the same track among
  several in it ("Dialogue" rather than "Signs/Songs"). They're kept per
  show in `settings.json`.
- Switching the audio to another language turns on the subtitles for all
  the dialogue in the language you were listening to (from an English dub
  to Japanese: the English "Dialogue" ones), and switching back to the
  episode's own audio puts its own subtitles back.

### Changed

- A simpler seek bar: a line filled up to where the video is, with a dot
  there only while the pointer is on it, in place of the large knob.
- A and S open the audio and subtitle menus, on the track playing (the
  arrows and Enter pick one), instead of switching to the next track.

### Fixed

- The time shown over the seek bar follows the pointer, and leaves out
  chapters that are only numbered ("Chapter 01"): a chapter shows only
  when it has a name.
- Blu-ray and DVD subtitles no longer go missing after a seek with
  hardware decoding: going back a few seconds often left the line on
  screen out until the next one.
- The Skip Intro button's text is dark, not white on white, and
  the audio button is no longer greyed out when there's a single track.

## [0.3.0] - 2026-10-23

### Added

- A Windows setup, `jelly-windows-setup.exe`, that installs Jelly for the
  user alone, with shortcuts, and keeps it up to date: a new release
  downloads in the background and starts the next time Jelly does (or right
  away, from the notice).
- Everywhere else (Linux, the Windows zip), Jelly says when there's a new
  release, with a link to it.

## [0.2.0] - 2026-10-23

### Added

- Tab completion in bash, zsh and fish, for the commands and for the movies
  and shows on your server in `play` and `open`. `install.sh` and the Linux
  tarball include it, and `jelly completions SHELL` prints it.
- Signing in with a password signs in to Seerr too, with the same account,
  when Jelly finds Seerr next to the Jellyfin server.

### Changed

- The Windows folder no longer shows a few hundred DLLs: the app and its
  libraries moved into `runtime`, and the `jelly.exe` next to it starts the
  app, with the same command line.

### Fixed

- The Windows release no longer includes Mesa's `opengl32.dll`, which Windows
  used in place of the graphics card's own OpenGL driver (rendering through
  Direct3D 12 instead), and the 175 MB of Mesa and LLVM it brought along.

## [0.1.0] - 2026-10-23

The first release.

### Added

- Home: a carousel of new titles, Continue Watching and Next Up (click to
  resume straight away), the libraries, and what's new in each.
- Libraries, search (Ctrl+F), and pages for movies and shows: what they play
  in (4K, Dolby Vision, Atmos…), seasons and episodes, cast, and similar
  titles. Right-click a card to play from the beginning, go to its show, or
  mark it watched or unwatched.
- Playback with mpv: every file plays as it is, with hardware decoding, so
  the server never transcodes. It resumes where you left off on any device,
  picks audio and subtitles by your Jellyfin language settings, reports
  progress back, offers to skip intros and recaps, counts down to the next
  episode over the credits, marks chapters on the seek bar, and shows when
  it will end.
- Requests through Seerr (Jellyseerr or Overseerr): what search finds that
  isn't in the library, a page for each with its request and downloads, and
  On the Way on Home and in the library it'll land in.
- Sign-in with a password or Quick Connect.
- A command line: `continue`, `play`, `open`, `search`, `list` (with
  `--json`) and `version`.
- Releases for Linux (a tarball that uses the system's GTK, libadwaita and
  libmpv) and Windows (a folder that runs without installing anything).

[Unreleased]: https://github.com/JoaoTRoxo/jelly/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/JoaoTRoxo/jelly/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/JoaoTRoxo/jelly/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/JoaoTRoxo/jelly/releases/tag/v0.1.0
