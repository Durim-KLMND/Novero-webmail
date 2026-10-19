# Novero Mail

A Windows desktop email client with a Rust core and a Tauri 2 shell. IMAP and SMTP first.

## Layout

| Path | What it is |
| --- | --- |
| `crates/mail-core` | Shared types, errors and the event enum |
| `crates/mail-sync` | IMAP sync engine (one actor per account) |
| `crates/mail-send` | SMTP submission and outbox |
| `crates/mail-mime` | MIME parsing and building, HTML sanitizing, threading |
| `crates/mail-store` | SQLite cache, migrations, message files |
| `crates/mail-auth` | Credentials, OAuth2, autodiscovery |
| `apps/desktop` | Svelte + TypeScript frontend (Vite) |
| `apps/desktop/src-tauri` | Tauri host crate (`desktop`) |

The protocol crates do not depend on Tauri, so they build and test headless.

## Prerequisites

- Rust stable (1.90 or newer), via [rustup](https://rustup.rs)
- Node.js 22 and npm
- Windows: the Microsoft C++ Build Tools and WebView2 (preinstalled on Windows 11)
- Linux (for CI or development): `libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev`

## Common commands

```sh
cd apps/desktop
npm install
npm run tauri dev        # run the app with hot reload
npm run tauri build      # build MSI and NSIS installers into target/release/bundle
```

From the repository root:

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace   # needs apps/desktop/dist: run `npm run build` once first
cargo deny check         # licenses, advisories, sources (install with `cargo install cargo-deny`)
```

## Integration tests

`testing/` holds a Dovecot IMAP server with a seeded mailbox (user `alice`, password `secret`,
150 messages in INBOX, SPECIAL-USE folders). It needs Docker.

```sh
docker compose -f testing/docker-compose.yml up -d --build --wait
cargo test -p mail-sync -- --ignored
docker compose -f testing/docker-compose.yml down
```

To try the app against the test server, start it with `npm run tauri dev`, enter any address
and the password `secret`, open **Server settings** and use IMAP host `127.0.0.1`, port `1143`,
security **None (unsafe)**, username `alice`. Any SMTP host works for now; sending is not built yet.
Add the same login again under a second address to try multiple accounts.

On Windows the password is kept in Windows Credential Manager. Elsewhere it is held in memory only,
so on Linux or macOS an account stops checking mail after the app restarts; remove and add it again.

To check the TLS path against a real server (bad credentials on purpose; success is a refused login):

```sh
cargo run -p mail-sync --example tls_probe -- imap.gmail.com
```

## CI

`.github/workflows/ci.yml` runs on every pull request and on pushes to `main`:

- **Frontend**: `svelte-check` and a production build.
- **Rust**: `cargo fmt --check`, `cargo clippy` with warnings as errors, and the workspace tests on Linux.
- **Integration**: starts the Dovecot test server and runs the IMAP tests against it.
- **Dependencies**: `cargo deny check` against `deny.toml`.
- **Windows**: workspace tests and an unsigned MSI and NSIS installer build. Pushes to `main` upload the installers as a build artifact.
