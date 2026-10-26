# Novero Mail

A Windows desktop email client with a Rust core and a Tauri 2 shell. IMAP and SMTP first.

## Download

Get the installer (`Novero-Mail_…_x64-setup.exe`) from the
[releases page](https://github.com/Durim-KLMND/Novero-webmail/releases). It installs for the
current user without administrator rights and keeps itself up to date. Test versions are marked
as pre-releases; to receive them, pick **Beta** under Settings → Updates. How releases are made
is in [docs/releasing.md](docs/releasing.md).

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

Inside the desktop app, files are grouped by feature:

| Path | What it is |
| --- | --- |
| `src/lib/mail` | Mail view, folder sidebar, message list, reader, rules, attachment viewer |
| `src/lib/compose` | Composer, its own window, address input, rich text editor |
| `src/lib/calendar` | Calendar view and event dialog |
| `src/lib/todo` | Task board and notes |
| `src/lib/cleanup` | Mail cleanup: mail sorted into categories by sender |
| `src/lib/accounts` | First-run setup, account form, server settings, sign in again |
| `src/lib/settings` | Settings dialog, signatures, templates |
| `src/lib/plan` | Free/Pro plan, license key, upgrade prompt |
| `src/lib/shell` | Title bar, app rail, status bar, menus, toasts, update and crash dialogs |
| `src/lib/ui` | Shared pieces: icons, small dialogs, theme, scroll bars |
| `src/lib/api.ts`, `format.ts`, `later.ts` | Backend calls and helpers used everywhere |
| `src-tauri/src/mail` | Reading, sending, folders, rules, cleanup, snooze, attachments, invites, mailto links |
| `src-tauri/src/calendar` | Calendar events, reminders and Google/Outlook sync |
| `src-tauri/src/shell` | Windows, tray, taskbar badge, updates, crash reports |
| `src-tauri/src` | Startup (`lib.rs`), account and message commands, app state, settings, plan, to-dos |

On Windows, file names are not case sensitive, so a folder must never hold both `Name.svelte`
and `name.svelte.ts`; give the script file a different name.

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
npm run tauri build      # build the installer into target/release/bundle/nsis
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
cargo test -p mail-send -- --ignored
cargo test -p mail-auth -- --ignored
cargo test -p mail-store --release -- --ignored --nocapture   # search speed over 100,000 messages
docker compose -f testing/docker-compose.yml down
```

The compose file also starts Mailpit (SMTP on `1025`, web inbox on `http://localhost:8025`) and a
small fake OAuth server next to Dovecot that lets the `mail-auth` tests log in with XOAUTH2.

To try the app against the test servers, start it with `npm run tauri dev`, enter any address that
isn't at a known provider, and continue. Enter the password `secret`, then under **Server settings**
use IMAP host `127.0.0.1`, port `1143`, SMTP host `127.0.0.1`, port `1025`, security
**None (unsafe)** for both, and username `alice`.
Add the same login again under a second address to try multiple accounts.

On Windows the password is kept in Windows Credential Manager. Elsewhere it is held in memory only,
so on Linux or macOS an account stops checking mail after the app restarts; remove and add it again.

To check the TLS path against a real server (bad credentials on purpose; success is a refused login):

```sh
cargo run -p mail-sync --example tls_probe -- imap.gmail.com
```

## Searching

The search box (press `/`) looks through the subject, addresses, text and attachment names of
every account's mail. Words match as prefixes ("inv" finds "invoice"), ignoring case and accents;
put words in quotes to match a phrase. Filters can be mixed with words:

| Filter | Finds |
| --- | --- |
| `from:ann` | Mail from Ann, by name or address |
| `to:ann` | Mail to or copied to ann |
| `subject:invoice` | Words in the subject only |
| `is:unread`, `is:read`, `is:flagged` | Mail by its state |
| `has:attachment` | Mail with attachments |
| `in:sent` | Mail in a folder (by name or role: inbox, sent, drafts, archive, trash, junk) |

Trash and Junk are left out unless `in:` names them. Message text is searchable once the message
has been downloaded: opening it does that, and each sync downloads the last 30 days ahead of time.

## Cleaning up mail

The broom on the rail (`Ctrl+4`) sorts the mail in every account's inbox (or, with **All mail**,
also Archive and your own folders) into Personal, Updates, Social, Forums, Newsletters, Promotions
and Spam, grouped by sender. The guesses are made on the computer from the sender, the subject and
the headers bulk mail carries (`List-Unsubscribe`, `List-Id`, `Precedence`, `Auto-Submitted`,
spam-filter flags), and from whether you have written to the sender; nothing is sent anywhere.
The rules live in `crates/mail-core/src/categories.rs`.

**To review** lists the suggestions you haven't confirmed. Accepting one for a sender whose mail
is all in one category files the sender there, so new mail from them follows; picking another
category does the same. Mail can then be archived, deleted or moved to Junk by sender, and lists
with an unsubscribe link can be left from the sender's menu. New inbox mail from senders in Spam
goes straight to Junk. Mail cached before this feature has its list headers read once, from the
message file when it was downloaded or else from the server.

## Keyboard shortcuts

Press `?` in the app for the list. `J`/`K` move through the list, `C` writes, `R`/`A`/`F` reply,
reply all and forward, `E` archives, `#` deletes, `S` flags, `Shift+U`/`Shift+I` mark unread or
read, and `Ctrl+Enter` sends.

## Settings

The gear at the bottom of the sidebar sets the theme (follow Windows, light or dark), whether
messages are grouped into conversations, new-mail notifications, and a signature per account.
Settings are kept in the app's database.

## Google and Microsoft sign-in

Signing in through Google's or Microsoft's own page needs app registrations whose client IDs are
built into the app. [docs/oauth-apps.md](docs/oauth-apps.md) has the steps. Without them, the app
offers app passwords for Gmail and can't add Outlook.com or Microsoft 365 accounts.

## CI

`.github/workflows/ci.yml` runs on every pull request and on pushes to `main`:

- **Frontend**: checks the app version agrees across files, then `svelte-check` and a production build.
- **Rust**: `cargo fmt --check`, `cargo clippy` with warnings as errors, and the workspace tests on Linux.
- **Integration**: starts the Dovecot test server and runs the IMAP tests against it.
- **Dependencies**: `cargo deny check` against `deny.toml`.
- **Windows**: workspace tests. Installers come from releases; to get one for a branch, run the workflow on it by hand (**Actions → CI → Run workflow**), which also builds the installer and uploads it as a build artifact.

Pull requests that change no Rust code (only the UI or the docs) skip the Rust, Integration, Dependencies and Windows jobs. Pushes to `main` always run everything.

Pull requests reuse the build cache from `main` but don't save their own, which saves several minutes per run. Releases keep a separate cache of release-mode dependencies, saved by each release from `main`, so a release only rebuilds Novero's own crates.

`.github/workflows/release.yml` runs on version tags and publishes releases; see
[docs/releasing.md](docs/releasing.md).
