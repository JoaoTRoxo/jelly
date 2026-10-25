# Jelly

[![CI](https://github.com/JoaoTRoxo/jelly/actions/workflows/ci.yml/badge.svg)](https://github.com/JoaoTRoxo/jelly/actions/workflows/ci.yml)

A native GTK 4 and libadwaita client for [Jellyfin](https://jellyfin.org).
Jelly plays every file as it is with [mpv](https://mpv.io), so the server
never has to transcode: MKV, 10-bit HEVC, AV1, Opus, FLAC, styled ASS
subtitles with their fonts and PGS subtitles all play directly, with hardware
decoding. That suits home servers too slow to transcode, and it keeps the
picture and sound exactly as they are.

![Jelly's home page](docs/screenshots/home.png)

<p>
  <img src="docs/screenshots/movie.png" alt="A movie's page" width="49%">
  <img src="docs/screenshots/show.png" alt="A show's page" width="49%">
</p>

<sub>Shown with the public-domain films of Jellyfin's
[demo server](https://demo.jellyfin.org/stable).</sub>

- **Home**: a carousel of new titles, Continue Watching and Next Up (click to
  resume straight away), the libraries, and what's new in each
- **Movies and shows**: what they play in (4K, Dolby Vision, Atmos…), seasons
  and episodes, cast, and similar titles; right-click any card to play from
  the beginning, go to its show, or mark it watched
- **Playback** resumes where you left off on any device, picks the audio and
  subtitle tracks your Jellyfin language settings ask for (or, in a show,
  the ones you last picked in it), turns on subtitles when you switch to
  audio in another language, and reports progress back. It offers to skip
  intros and recaps, counts down to the next episode over the credits, marks
  chapters on the seek bar and shows when it will end
- **Requests** through [Seerr](https://github.com/seerr-team/seerr)
  (Jellyseerr or Overseerr) for what isn't in the library yet, with how its
  download is going
- **A command line** for keyboard shortcuts and launchers

## Install

### Linux

Jelly needs GTK 4.16 or newer, libadwaita 1.6 or newer and libmpv: Arch,
Fedora 41+, Debian 13, Ubuntu 25.04+, openSUSE Tumbleweed and the like.
(Ubuntu 24.04 and distributions based on it have a libadwaita that's too
old.) It works on any desktop, GNOME, KDE Plasma or a Wayland compositor, and
follows its light or dark style.

From a [release](https://github.com/JoaoTRoxo/jelly/releases), with the
libraries installed (`gtk4 libadwaita mpv` on Arch, `libgtk-4-1
libadwaita-1-0 libmpv2` on Debian and Ubuntu):

```sh
tar -xzf jelly-*-linux-x86_64.tar.gz
jelly-*-linux-x86_64/install.sh
```

That puts `jelly` in `~/.local/bin`, and its launcher entry and icon in
`~/.local/share`. To build it from source instead, with Rust and the
libraries' development files:

```sh
sudo pacman -S rust gtk4 libadwaita mpv                            # Arch
sudo apt install cargo libgtk-4-dev libadwaita-1-dev libmpv-dev    # Debian, Ubuntu
./install.sh
```

Either `install.sh` installs elsewhere with `PREFIX=DIR`, and updates Jelly
when run again. Installed this way, Jelly offers to update itself when
there's a new release: Update, then Restart Now.

### Windows

Download
[`jelly-windows-setup.exe`](https://github.com/JoaoTRoxo/jelly/releases/latest/download/jelly-windows-setup.exe)
and run it. It installs Jelly for you alone (no administrator needed), with
a Start menu and desktop shortcut, and from then on Jelly keeps itself up to
date: a new release downloads in the background and starts the next time you
open Jelly. Uninstall it from Settings → Apps.

Or, without installing, download `jelly-*-windows-x86_64.zip` from a
[release](https://github.com/JoaoTRoxo/jelly/releases), unzip it anywhere and
run `Jelly\jelly.exe`. Keep it in its folder: it starts the app in `runtime`,
next to it, which has the libraries it needs. That copy doesn't update
itself, but says when there's a new release.

Windows warns about either the first time, as Jelly isn't signed: choose
More info, then Run anyway.

### macOS

Jelly should build with Homebrew's `gtk4`, `libadwaita` and `mpv` and
`cargo build --release`, but it hasn't been tried there yet.

## Using it

Sign in with your server's address (`jellyfin.example.com`, or
`192.168.1.5:8096` at home) and your Jellyfin account, or with Quick Connect
from a device that's signed in.

| Key | |
| --- | --- |
| Ctrl+F | Search |
| F5 | Reload the page |
| Space, K, click | Pause |
| ← / → | Back / forward 5 s |
| J / L | Back / forward 10 s |
| ↑ / ↓ | Volume |
| F, F11, double-click | Fullscreen |
| S / A | Next subtitle / audio track |
| M | Mute |
| N / P | Next / previous episode |
| Esc | Leave fullscreen, then go back |

Skipping intros and recaps needs the server to know where they are, from the
[Intro Skipper](https://github.com/intro-skipper/intro-skipper) plugin or
Jellyfin's media segments. Without a credits segment, the next episode's
card shows for the last 30 s.

### Requests

What the search finds through Seerr that isn't in the library shows under
the library's results: click a movie to request it, or a show to pick its
seasons. Signing in to Jelly with your password signs in to Seerr too, if
it's found; otherwise sign in there with your Jellyfin password, once. Jelly
keeps Seerr's session, not the password. It looks for Seerr at `requests.`,
`request.`, `seerr.`, `jellyseerr.` or `overseerr.` on your server's domain,
then on the Jellyfin machine at port 5055, and the sign-in dialog can change
that.

What's on its way shows under **On the Way** on Home and at the top of the
library it'll land in (anime in a library named "Anime", if there is one),
with how its download is going, which Seerr hears from Radarr and Sonarr
every minute. Each has a page with its request's state, each episode's
download and the cast, and Open in Library once it's there.

### Command line

```sh
jelly continue            # what you were watching: resume it, or the next
                          # episode of the show you finished one of
jelly play frieren        # a movie or episode, or a show's next episode
jelly open la la land     # a movie's or show's page
jelly search frieren      # "description<TAB>ID" lines
jelly list continue       # also next-up, latest; --json for scripts
jelly list requests       # what's on its way through Seerr, and how far along
```

Tab completes the commands, and in `play` and `open` the movies and shows
on your server (`jelly play fri` Tab → `jelly play Frieren: Beyond Journey's
End`; with nothing typed, what you're watching). `install.sh` sets it up for
bash and fish; for zsh, add `fpath+=(~/.local/share/zsh/site-functions)` to
`~/.zshrc` before `compinit`. Or load it from the shell's startup file:
`source <(jelly completions bash)` or `zsh`, or `jelly completions fish |
source`.

`continue`, `play` and `open` use the window that's open, if there is one
(on Windows, they open a window of their own); `search` and `list` print. A
name can also be an ID from `search` or `list`. To put `continue` on a key,
give the shortcut the full path, such as `/home/you/.local/bin/jelly continue`:

- **KDE Plasma**: System Settings → Keyboard → Shortcuts → Add New → Command
- **GNOME**: Settings → Keyboard → View and Customize Shortcuts → Custom Shortcuts
- **Sway, Hyprland and other compositors**: a `spawn` or `exec` binding
- **Windows**: a shortcut to `jelly.exe continue`, with a Shortcut key in its
  Properties

### Files

- `~/.config/jelly/session.json`: the sign-in (readable only by you)
- `~/.config/jelly/seerr.json`: the Seerr session (readable only by you)
- `~/.config/jelly/settings.json`: the last server, the volume, Seerr's address
- `~/.config/jelly/mpv/mpv.conf`: mpv settings of your own, if you make one
- `~/.cache/jelly/images`: the poster cache

On Windows the first four are in `%LOCALAPPDATA%\jelly`, and the poster cache
in `%LOCALAPPDATA%\Microsoft\Windows\INetCache\jelly`. The setup installs
Jelly itself in `%LOCALAPPDATA%\io.github.joaotroxo.Jelly`.

When it starts, Jelly asks GitHub for its latest release, to update or to say
there's a new one.

## Troubleshooting

- **Playback stutters**: Jelly streams the original file, so playing it needs
  as much bandwidth as the file's bitrate, from the server's upload to your
  connection. A 4K remux can need 60–100 Mbit/s.
- **Nothing plays, or the wrong tracks**: `JELLY_MPV_LOG=all=v jelly` prints
  mpv's log, and `JELLY_MPV_OPTIONS="hwdec=no,sid=1"` passes it options of
  your own (or put them in `mpv.conf`). On Windows, started from its icon,
  Jelly writes its messages and mpv's warnings to
  `%LOCALAPPDATA%\jelly\jelly.log` instead. Please include the log when you
  [report a problem](https://github.com/JoaoTRoxo/jelly/issues).
- **Windows says it protected your PC**: Jelly isn't signed. Choose More
  info, then Run anyway.

## Contributing

Bug reports, ideas and pull requests are welcome: see
[CONTRIBUTING.md](CONTRIBUTING.md) for how to build, test and send a change,
and [SECURITY.md](SECURITY.md) to report a vulnerability privately.

## License

Jelly is free software: you can redistribute it and modify it under the
terms of the [GNU General Public License](LICENSE), version 3 or (at your
option) any later version.
