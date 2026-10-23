# Security

## Reporting a vulnerability

Please don't open a public issue. Report it privately through GitHub's
[security advisories](https://github.com/JoaoTRoxo/jelly/security/advisories/new)
for this repository, with what's affected, how to reproduce it, and what
someone could do with it. You'll hear back within a week, and the fix goes in
the next release, crediting you unless you'd rather not be.

Only the latest release is supported.

## What Jelly keeps, and where

- **Your Jellyfin sign-in**: the access token the server gave Jelly, never
  your password, in `~/.config/jelly/session.json` (on Windows,
  `%LOCALAPPDATA%\jelly`). On Linux and macOS the file and its folder are
  readable only by you; on Windows, your user folder already is. Signing out
  in Jelly ends the session on the server too.
- **Your Seerr sign-in**: Seerr's session cookie, never the password, in
  `seerr.json` next to it.
- Jelly sends the token only to the Jellyfin server you signed in to (in an
  `Authorization` header, and to mpv for its requests to that server), and
  Seerr's cookie only to Seerr. Posters from TMDB, for Seerr, are fetched
  without either, and so is Jelly's latest release, from GitHub, when it
  starts.
- **Updates**: Jelly installed with the Windows setup downloads new releases
  from this repository's GitHub releases and installs them (through
  [Velopack](https://velopack.io)), checking each package against the
  SHA-256 in the release's feed. Releases aren't signed, so updates are as
  trustworthy as this repository's releases.
- HTTPS certificates are always checked: Jelly's own requests against the
  Mozilla root certificates built into it, and mpv's against the system's.
  An address typed without `https://` gets HTTPS unless it looks like a
  server at home (an IP address, a port, or a name without dots, such as
  `nas:8096`); a server reached over plain HTTP gets the token unencrypted.
