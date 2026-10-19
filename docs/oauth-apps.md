# Signing in with Google and Microsoft

Novero Mail signs in to Gmail and Microsoft accounts through the provider's own page in the
browser (OAuth 2.0 with PKCE and a loopback redirect), then logs in to IMAP and SMTP with
XOAUTH2. For that, each provider needs an app registration whose client ID is built into the
app. Without one, the setup wizard offers app passwords for Gmail and explains that Outlook.com
and Microsoft 365 accounts can't be added yet.

The IDs are read at build time, or at run time if the environment variable is set:

| Variable | What it is |
| --- | --- |
| `NOVERO_GOOGLE_CLIENT_ID` | Google OAuth client ID (ends in `.apps.googleusercontent.com`) |
| `NOVERO_GOOGLE_CLIENT_SECRET` | The secret Google issues with a desktop client |
| `NOVERO_MICROSOFT_CLIENT_ID` | Application (client) ID of the Microsoft Entra app |

Google's "client secret" for a desktop app is not confidential: Google says so, since it ships
inside every copy of the app. It is kept out of the repository anyway so forks use their own.

## Google

1. In the [Google Cloud console](https://console.cloud.google.com/), create a project, for
   example "Novero Mail".
2. **APIs & Services → Library**: enable the **Gmail API**.
3. **Google Auth Platform → Branding**: app name "Novero Mail", support email, and the
   developer contact. **Audience**: user type **External**.
4. **Data Access → Add or remove scopes**: add `https://mail.google.com/`, `openid` and
   `.../auth/userinfo.email`.
5. **Clients → Create client**: application type **Desktop app**. No redirect URI is needed;
   desktop clients accept `http://127.0.0.1` on any port.
6. Copy the client ID and client secret.

While the app is in **Testing**, only the Google accounts listed under **Audience → Test users**
(up to 100) can sign in, and Google expires their sign-in after 7 days, so Novero Mail asks
them to sign in again. Publishing removes both limits, but `https://mail.google.com/` is a
restricted scope: Google requires verification and a yearly third-party security assessment
before the app can be used by anyone.

## Microsoft (Outlook.com and Microsoft 365)

1. In the [Microsoft Entra admin center](https://entra.microsoft.com/), go to
   **App registrations → New registration**.
2. Name "Novero Mail". Supported account types: **Accounts in any organizational directory and
   personal Microsoft accounts**.
3. Redirect URI: platform **Public client/native (mobile & desktop)**, URI `http://localhost`.
   Microsoft accepts any port on `localhost` for this kind of app.
4. **Authentication**: set **Allow public client flows** to **Yes**. No client secret is used.
5. **API permissions → Add a permission → Microsoft Graph → Delegated**: add
   `IMAP.AccessAsUser.All`, `SMTP.Send`, `offline_access`, `openid` and `email`.
6. Copy the **Application (client) ID** from the overview page.

Work and school accounts may need their admin to consent to the app, and SMTP sending must be
allowed for the mailbox (Exchange admin center: **Authenticated SMTP**).

## Using the IDs

**CI and installers.** In the GitHub repository, open **Settings → Secrets and variables →
Actions** and add the three variables above as repository secrets. The Windows job passes them
to the installer build. Pull requests from forks don't get secrets, so their builds have no
provider sign-in.

**Local builds.** Export the variables before `npm run tauri dev` or `npm run tauri build`:

```powershell
$env:NOVERO_GOOGLE_CLIENT_ID = "…apps.googleusercontent.com"
$env:NOVERO_GOOGLE_CLIENT_SECRET = "…"
$env:NOVERO_MICROSOFT_CLIENT_ID = "…"
```

Changing them later needs a rebuild of `mail-auth` (`cargo clean -p mail-auth`) for the
built-in values, or just setting them before starting the app.
