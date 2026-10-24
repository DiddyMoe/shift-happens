# Security policy

## Reporting a vulnerability

Please report security problems privately: open the repository's **Security**
tab and choose **Report a vulnerability**. Don't put details in a public issue.
If private reporting isn't available, open an issue that asks for a private
contact and leaves the details out.

Include the version (shown on **About**), the steps to reproduce, and what an
attacker could do. Shift Happens has one maintainer, so responses are best
effort.

## Supported versions

Shift Happens is pre-1.0. Only the latest release and the `main` branch receive
security fixes.

## Security model

Shift Happens is a Windows desktop app for one manager on one computer. It has
no server, accounts, network connections, telemetry, updater, or remote
content. Its data is a SQLite database in the signed-in user's
`%LOCALAPPDATA%` folder.

The app doesn't encrypt its database, backups, or exports. It relies on
Windows instead, and assumes:

- each manager signs in with their own Windows account, never a shared or
  kiosk account;
- the drive is protected by full-disk encryption such as BitLocker;
- notes stay free of patient and medical information.

Under those assumptions, the Windows account boundary keeps other users out of
the data folder, and disk encryption protects a lost or stolen computer.

### Out of scope

- Someone using the computer while it is signed in and unlocked.
- Malware or other programs running under the same Windows account, and
  administrators of the computer.
- Backups, Excel and PDF exports, and printouts once saved. They are ordinary
  unencrypted files and paper. Keep backups on an access-controlled network
  drive or a BitLocker-encrypted removable drive.
- Computers without disk encryption or used with shared Windows accounts.

If you run your own copy, check these assumptions before you rely on it.

### In scope

Problems in the app itself, for example: a crafted backup file or input that
runs code, corrupts or replaces data without confirmation, gets past the
restore and integrity checks, or keeps the app busy without limit; unsafe file
writes; injection into exported documents; and personal data written to logs
or diagnostics.

## Unencrypted local storage

Leaving local data unencrypted is a deliberate, documented decision rather than
an oversight. The reasoning and the conditions that would change it are in the
[technical reference](docs/technical-reference.md#desktop-security).
