# JF Native

A fast, unofficial [Jellyfin](https://jellyfin.org/) client for LG webOS televisions. Native, not a
web page: the interface is drawn straight on the GPU, and video plays on the TV's own decoder.

*A fork of [PlxNative](https://github.com/GLinnik21/plx-native) by Gleb Linnik. JF Native is an
independent project and is not affiliated with Jellyfin or LG.*

## Why this exists

The official Jellyfin app on my LG is slow to get around. Scrolling a shelf stutters, opening a
film takes a beat too long. It behaves like a web page because it is one: a web app running in the
television's browser. Playback was never the problem.

PlxNative had already fixed this for Plex. It draws straight on the GPU and hands video to the same
silicon the built-in apps use. No Chromium, no JavaScript, no web view. This fork keeps all of that
and points it at a Jellyfin server instead.

## How it works

The interface, data layer and player are PlxNative's. Each request the app would send to a Plex
server is answered from the Jellyfin API instead (`rust-modules/plex/src/jellyfin/`), so the rest of
the app runs unchanged.

## What it does

- **Sign in with Quick Connect.** The TV shows a code and a QR code. Approve it from Jellyfin on
  your phone or computer.
- **Finds your server** through Jellyfin's network discovery, or by scanning the local network on
  port 8096.
- **Browse and search** your movies, TV shows and collections: Continue Watching, Next Up, Recently
  Added, cast, seasons and episodes.
- **Direct play** H.264 and HEVC, including 4K, HDR and Dolby Vision profiles 5 and 8. Anything else
  Jellyfin transcodes.
- **Everything you expect while watching**: resume, seek, chapters, audio and subtitle tracks, Skip
  Intro and Skip Credits from Jellyfin's media segments, and progress reported back to your server.

## Will it work on my television?

It should run wherever PlxNative does: webOS 4.0 or newer. PlxNative's README lists the
[known issues](https://github.com/GLinnik21/plx-native#will-it-work-on-my-television) with
particular sets, and they apply here too.

## Installing

JF Native installs through LG Developer Mode. No root is required.

1. Enable Developer Mode on the TV and set up the
   [webOS CLI](https://webostv.developer.lge.com/develop/tools/cli-installation).
2. Download the `.ipk` from a successful **CI** run on the `jellyfin` branch (the `plxnative-<sha>`
   artifact).
3. If discovery won't find your server, add a `jellyfin-server.txt` with its URL next to
   `appinfo.json` inside the package.
4. `ares-install -d tv com.jfnative.app.debug_*.ipk`, then `ares-launch -d tv com.jfnative.app.debug`.

Developer Mode needs periodic renewal. If the session lapses, LG removes the apps installed through
it.

## Privacy

JF Native talks to your Jellyfin server and nothing else. The crash reports and analytics PlxNative
offers are switched off. [`PRIVACY.md`](PRIVACY.md) has the details.

## The honest scope

I built this for how *I* watch:

- **Movies and TV shows.** No music, no photos, no live TV.
- **Quick Connect is the only sign-in**, so it has to be enabled on the server
  (Dashboard > General > Quick Connect).
- **No typing in server addresses** yet. Use discovery or `jellyfin-server.txt`.
- **English only** for the Jellyfin-specific text so far.
- **One household's TV and server.** There will be bugs I haven't hit.

## Developing

[`docs/building.md`](docs/building.md) is the build and the test loop, inherited from PlxNative.
`make sim-linux` builds a desktop simulator that runs the same code as the TV (WSLg works on
Windows). It can sign in, browse and search a real server, but it can't play video: that needs the
TV's hardware decoder.

## Licence

[GPL-3.0-or-later](LICENSE), © 2026 Gleb Linnik and contributors (PlxNative) and the JF Native
contributors. The PlxNative name and artwork are not used by this fork, see
[`TRADEMARKS.md`](TRADEMARKS.md). Third-party components and their licences are in
[`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md) and `licenses/`, notably the app's own LGPL build
of FFmpeg and the Jellyfin logo used for the launcher icon. Those notices ship inside the `.ipk` too.

"Jellyfin", "LG" and "webOS" are trademarks of their respective owners and are used here only to say
what this software works with.
