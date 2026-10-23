# Changelog

All notable changes to Jelly are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and Jelly uses
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/JoaoTRoxo/jelly/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/JoaoTRoxo/jelly/releases/tag/v0.1.0
