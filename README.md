# Shift Happens

A Windows desktop app for building hospital staff schedules. It keeps track of
employees, availability and PTO, shifts, staffing needs, and holidays, then
generates a schedule you can adjust, finalize, print, or export to Excel and PDF.

Everything stays on your computer in a local database. There is no server,
account, or cloud sync.

> **Intended use:** an administrative tool for one department manager. It holds
> no patient data, gives no clinical advice, and is not a medical device. It
> supplements your workplace's official scheduling, timekeeping, and payroll
> systems.

## What's in the app

- **Schedule**: generate a draft for a month or a custom date range, then adjust
  it on the board. Select a shift to see who can take it. **Issues** lists rule
  problems, and **Finalize…** locks the schedule when it's done.
- **All schedules**: find, open, and archive past schedules.
- **People**: employees and their rules, plus availability and PTO.
- **Setup**: shift templates, positions and coverage (who is needed each day),
  and holidays.
- **Backup**: save a full copy of your data or restore one.
- **Settings**: workplace rules and the theme (dark, light, or system).
- **Help** (F1): getting started, how-tos, and keyboard shortcuts.

## Getting started

You need Windows with:

- Node.js 24 LTS (22.22 or newer) and npm
- Rust 1.94 or newer (stable MSVC toolchain)
- Microsoft C++ Build Tools with **Desktop development with C++**, and the
  Windows SDK
- Microsoft Edge WebView2

The [Tauri prerequisites guide](https://v2.tauri.app/start/prerequisites/) shows
how to install them. Then, in PowerShell from the project folder:

```powershell
npm ci --ignore-scripts
npm run tauri:dev
```

The development app uses its own throwaway database, separate from an installed
copy's data.

## Building

| Command                   | Result                                                 |
| ------------------------- | ------------------------------------------------------ |
| `npm run tauri:build`     | Release `.exe`, no installer                           |
| `npm run tauri:installer` | Per-user Windows installer                             |
| `npm run release:windows` | Full release build with every check, plus checksums \* |

\* Needs a clean Git checkout, `cargo-deny` 0.20.2, and the full 7-Zip.
Builds are unsigned unless you provide signing credentials. Details are in the
[technical reference](docs/technical-reference.md#windows-installer-and-local-release).

### From WSL

You can cross-compile the Windows `.exe` from WSL, though the app itself only
runs on Windows. Install `cargo-xwin`, LLVM, Clang, and LLD, then:

```sh
rustup target add x86_64-pc-windows-msvc
export XWIN_CACHE_DIR="$HOME/.cache/shifthappens-xwin"
export XWIN_ARCH=x86_64
npm run tauri:build -- --runner cargo-xwin --target x86_64-pc-windows-msvc
```

The first build downloads Microsoft's SDK libraries. The result is
`src-tauri/target/x86_64-pc-windows-msvc/release/shift-happens.exe`.

To run the development app from WSL, close any installed copy, then run:

```sh
npm run tauri:dev:wsl
```

It builds the Windows debug executable and opens it on Windows. Frontend
changes appear immediately; Rust changes rebuild and reopen the app.

## Checking your changes

```sh
npm run format:check   # formatting
npm run typecheck      # TypeScript types
npm run lint           # ESLint
npm run build          # frontend build
npm run rust:fmt       # Rust formatting
npm run rust:clippy    # Rust lint (on Windows)
```

From WSL, with the two `XWIN_` variables above exported, run Clippy through
`cargo-xwin` instead:

```sh
cargo xwin clippy --manifest-path src-tauri/Cargo.toml --locked --target x86_64-pc-windows-msvc --bins --lib -- -D warnings
```

This project deliberately has **no automated tests**. Verify a change by using
the app and checking the result yourself. See [AGENTS.md](AGENTS.md) for the
full policy.

## Where your data lives

- The database is
  `%LOCALAPPDATA%\com.diddymoe.shifthappens\shift-happens.sqlite`. Development
  builds use a `development` subfolder there instead.
- Before upgrading the database, the app saves a copy in `migration-backups`
  next to it.
- A restore keeps the data it replaced in `restore-backups`.
- The app never deletes its own backups, and uninstalling keeps your data.

Use **Backup** in the app to save a copy anywhere you like or to restore one. A
restore takes effect the next time the app starts.

The app doesn't encrypt the database, backups, or exports. It relies on your
Windows account and drive encryption such as BitLocker; see
[SECURITY.md](SECURITY.md) for the assumptions and how to report a problem.

## Maintenance

- **Version:** `package.json` is the source of truth. Change it with
  `npm version patch --no-git-tag-version`; the build copies it into the Rust
  files.
- **Dependencies:** after changing any, run `npm run notices` to refresh the
  license list shown in **About**.
- **Development data:** the development database is disposable. Close the app,
  delete it along with its `-wal` and `-shm` files, and the app recreates it.
  Never reset an installed database to get past a startup error. Use a backup
  and restore instead.

## Project layout

| Path                          | Contents                                                           |
| ----------------------------- | ------------------------------------------------------------------ |
| `src/`                        | React and TypeScript frontend: screens, components, styles         |
| `src-tauri/src/`              | Rust backend: database, rules, generation, backup, restore, export |
| `src-tauri/migrations/`       | Database schema changes                                            |
| `src-tauri/installer/`        | Windows installer template                                         |
| `scripts/`                    | Version sync, license notices, Windows release script              |
| `docs/technical-reference.md` | How everything works in detail                                     |

Built with Tauri 2, Rust, React 19, TypeScript, Vite, and Tailwind CSS. The
`.exe` also has command-line options for backup, export, validation, and
diagnostics, which are covered in the technical reference.

## License

[MIT](LICENSE). Bundled dependencies keep their own licenses: see the
[third-party notices](src/generated/third-party-notices.txt), the PDF font's
[OFL](src-tauri/assets/fonts/OFL.txt), and the installer template's
[MIT](src-tauri/installer/LICENSE-MIT) and
[Apache-2.0](src-tauri/installer/LICENSE-APACHE) notices.
