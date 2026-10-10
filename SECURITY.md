# Security policy

## Reporting a vulnerability

Report privately, **not** as a public issue, through GitHub Security Advisories:
<https://github.com/BradSainty/jf-native/security/advisories/new>.

JF Native is a one-person hobby fork, so there is no service level or bounty. Please allow a
reasonable window for a fix before disclosing.

Vulnerabilities in code inherited unchanged from [PlxNative](https://github.com/GLinnik21/plx-native)
are likely to affect it too; please report those upstream as well, following its own security
policy.

## What is in scope

The app, its packaging, and the host-side tools in `tools/` and `ci/`. In particular:

- **The Jellyfin layer** (`rust-modules/plex/src/jellyfin/`, `rust-modules/session/src/auth/jellyfin.rs`):
  anything that sends the Jellyfin access token to a host other than the signed-in server, or to
  plex.tv, or writes it to a log.
- **Developer triggers.** `plxnative-*` files in the runtime directory change behaviour in
  development builds. A release build compiles them out (the `devtriggers` cargo feature); a
  release binary that still honours any of them is a valid report.
- **Logs.** `plxnative-events.log` is meant to be scrubbed of credentials and personal data. A token
  or server credential in it is a valid report.
- **The session file**, which holds the Jellyfin access token: a way to read it from another
  process, or to make the app write it world-readable.
- **TLS.** Certificate verification is meant to be on for every HTTPS request.
- **The bundled FFmpeg**, which parses untrusted bytes from the network.

## What is not in scope

- Attacks that require root on the television already.
- Jellyfin Server, webOS, LG's own libraries, or the Homebrew Channel. Report those to their
  maintainers.

## What this app does not have

No account of its own, no server, no payment path, and no telemetry: the crash reporting and
analytics inherited from PlxNative are switched off in this fork. It signs in to **your** Jellyfin
server and talks only to it. [PRIVACY.md](PRIVACY.md) has the details.
