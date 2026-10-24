# Releasing Novero Mail

Releases are GitHub releases built by `.github/workflows/release.yml` when a version tag is
pushed, or when a merge to `main` changes the app version. Each one carries the Windows
installer and a `latest.json` update manifest. Installed copies read that manifest and offer
the update.

## How installed copies update

- 30 seconds after start, and every six hours after that, the app reads the manifest on the
  latest release (`releases/latest/download/latest.json`).
- With **Download updates automatically** on (the default), a newer version is downloaded in the
  background; the sidebar shows the progress and then "Version x is ready · Restart to update".
  With it off, the sidebar says "Version x is out" and downloads it on **Download**.
- **What's new** shows the release notes, without the pull request links and authors.
- **Restart to update** first closes any message windows, which saves what was typed as drafts,
  then runs the installer. It closes the app, shows its progress and opens the new version.
- Settings → Updates shows the same state, when it last checked, and **Check now**.
- There is one channel: every release reaches everyone. Version names may still carry
  `-beta.n`, but the workflow always publishes the release as the latest, never as a
  pre-release, because GitHub's `latest` link skips pre-releases.

## One-time setup

### 1. Update signing key (required)

Every installer is signed with a key that only the release workflow has. Installed copies hold
the public half and refuse any update that wasn't signed with it, so a release can't be
replaced by someone who only gets hold of the download link.

On your own computer, in `apps/desktop`:

```sh
npx tauri signer generate -w novero-updater.key
```

Choose a password when asked. This writes `novero-updater.key` (private) and
`novero-updater.key.pub` (public). Then in the repository, under **Settings → Secrets and
variables → Actions**:

- **Secrets** tab → **New repository secret**:
  - `TAURI_SIGNING_PRIVATE_KEY`: the whole contents of `novero-updater.key`
  - `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`: the password you chose
- **Variables** tab → **New repository variable**:
  - `NOVERO_UPDATER_PUBKEY`: the whole contents of `novero-updater.key.pub` (one line that starts
    with `dW50cnVzdGVk`; paste it as it is, without decoding it)

Keep `novero-updater.key` and its password somewhere safe outside the repository, such as a
password manager. If it is lost, installed copies can't be updated any more and everyone has to
download a new installer by hand.

Builds without `NOVERO_UPDATER_PUBKEY` (local builds, forks) don't look for updates; Settings
says so.

### 2. Code signing (recommended before a public release)

Unsigned installers work, but Windows SmartScreen warns "Windows protected your PC" until a file
has built up reputation. A code-signing certificate removes that. The workflow supports
[Azure Trusted Signing](https://learn.microsoft.com/azure/trusted-signing/), the cheapest
option (a monthly fee, no hardware token):

1. Create a Trusted Signing account and a certificate profile in the Azure portal (the identity
   validation takes a few days).
2. Create an app registration in Microsoft Entra with a client secret, and give it the
   **Trusted Signing Certificate Profile Signer** role on the signing account.
3. Add these to the repository:
   - Secrets: `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`, `AZURE_TENANT_ID`
   - Variables: `AZURE_SIGNING_ENDPOINT` (for example `https://eus.codesigning.azure.net`),
     `AZURE_SIGNING_ACCOUNT`, `AZURE_SIGNING_PROFILE`

Without them the workflow still releases, unsigned, with a warning in the run summary.

## Making a release

1. From an up-to-date `main`, set the version and open a pull request with it:

   ```sh
   node scripts/set-version.mjs 0.2.0-beta.3   # or 0.2.0
   ```

   This changes `Cargo.toml`, `Cargo.lock`, `tauri.conf.json`, `package.json` and
   `package-lock.json`. CI checks they agree.
2. Merge it. `.github/workflows/auto-release.yml` sees that `main` now has a version without a
   tag, tags the merge commit `v0.2.0-beta.3` and runs the release build. Merges that don't
   change the version find the tag already there and release nothing.

   Releasing by hand still works: push a version tag yourself, or publish a release with a new
   tag from GitHub's **Releases** page, and the release build runs as before.

   ```sh
   git checkout main && git pull
   git tag v0.2.0-beta.3
   git push origin v0.2.0-beta.3
   ```

3. Watch the run under **Actions** (**Auto release** after a merge, **Release** for a tag you
   pushed). It checks the tag matches the app version,
   builds and signs the installer, checks the signature against `NOVERO_UPDATER_PUBKEY`,
   publishes the release with notes generated from the merged pull requests, marks it as the
   latest release, and attaches `latest.json`. If it fails (for example because a key is wrong), fix
   the cause, delete the release and its tag on GitHub, and use **Re-run all jobs** on the
   failed **Auto release** run (it tags the version again) or publish the release again from `main`.

Copies that download updates automatically find the release within six hours, or straight
away with **Check now** in Settings.

## The first release, and checking that updates work

No release exists until the first tag is pushed, so until then installed copies find nothing.
To see a real update end to end:

1. Do the one-time setup above (the update key is required; the release fails without it).
2. Release `v0.2.0-beta.1` (the current version) as described above, and install it from the
   release's assets. (Installers from hand-started CI runs can update too, once
   `NOVERO_UPDATER_PUBKEY` is set before they are built; `npm run tauri dev` builds can't.)
3. Merge anything, set the version to `0.2.0-beta.2`, and release that too.
4. In the installed copy, open Settings → Updates and press **Check now**. It downloads
   `0.2.0-beta.2`; **Restart to update** installs it, and Settings then shows Novero Mail
   0.2.0-beta.2.

## Trying an update before release

`NOVERO_UPDATE_ENDPOINT` makes an installed copy read a manifest from somewhere else, such as a
`latest.json` from a release's assets saved on a local web server. The installer it points to
must still be signed with the update key.

## Also in a release

- **Crash reports.** A crash writes a short report (where the code failed, without addresses,
  quoted text or the user's name) to the app's data folder. On the next start the app offers to
  send it, which opens a prefilled GitHub issue in the browser; nothing is sent otherwise.
  Settings has a switch to stop asking.
- **Default email app.** The installer registers Novero Mail with Windows as an email app.
  Windows doesn't let apps make themselves the default, so Settings has a button that opens
  **Default apps**, where the person picks Novero Mail for email links. Clicked `mailto:` links
  then open a new message from the selected account.
