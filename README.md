# Shift Happens

A local-first Windows desktop app for hospital staff scheduling, with availability
and PTO, coverage rules, schedule generation, and PDF/Excel exports. It serves one
hospital department manager as an administrative staff-scheduling tool: it uses no
patient data, gives no clinical recommendations, and is not a medical device. It
supplements the workplace's official scheduling, timekeeping, and payroll
systems. This repository implements **Prompts 01–15**, with the later
navigation, restore, and deletion changes described below.

## License

Shift Happens is licensed under the [MIT License](LICENSE). Bundled third-party
dependencies and assets retain their own licenses; see
[`src/generated/third-party-notices.txt`](src/generated/third-party-notices.txt),
the font's [`OFL.txt`](src-tauri/assets/fonts/OFL.txt), and the installer template's
[MIT](src-tauri/installer/LICENSE-MIT) and
[Apache-2.0](src-tauri/installer/LICENSE-APACHE) notices.

## How the app is organized

The sidebar has the **Schedule** workspace and **All schedules**, a **People**
group (Employees; Availability and PTO), a **Setup** group (Shift templates;
Positions and coverage; Holidays), and Backup and Settings pinned at the
bottom. The footer opens **Help** (also F1) and links to **About**.

- **Schedule** (`/`) is where each schedule is built and finished. It opens the
  current schedule, or the most recent non-archived one when none is current
  (labelled **Most recent**, without changing the setting; a stale setting stays
  visible until cleared). Its toolbar has Undo/Redo (Ctrl+Z, Ctrl+Y or
  Ctrl+Shift+Z), **Issues** (read-only validation in a side panel that rechecks
  after changes), **Export…**, **Print…** (Ctrl+P), **Finalize…**, and **More**
  (set or clear current, rename and notes, reopen as draft, archive, delete).
  **Generate…** creates a new draft for a calendar month or a custom range.
  The board shows operational dates across and employees (open shifts first)
  or positions down; selecting a shift opens a sheet with its details,
  candidates, notes, and lock. With no schedules yet, the workspace shows a
  setup checklist computed from the saved data.
- **All schedules** (`/schedules`) lists every schedule, including archived
  ones, with search and a status filter. Older addresses (`/schedule-editor`,
  `/validation`, `/generate-schedule`) redirect into the workspace.
- **Settings** holds the typed workplace rules and theme (dark, the default;
  light; or system). A missing two-week pay period start blocks generation when
  active two-week rules need it. **About** shows the version, database health,
  intended use, limitations, and third-party notices.
- **Help** has Getting started (live setup status and the routine for each
  schedule period), How to, Keyboard shortcuts, Data and privacy, and About. It
  closes with Escape, Close, or a click outside it. Print quick reference prints
  a one-page Letter summary through the WebView print dialog.

Durations are stored and calculated as whole minutes and shown and entered as
decimal hours (for example 7.5 h). Exports keep the stored minute columns.

Styles live in `src/styles/`: design tokens for both themes, base element
styles, shared components, the shell, the board, one file per screen, and print
rules. Typefaces are the Windows system fonts Bahnschrift and Segoe UI Variable,
so nothing is bundled or fetched. The finalization celebration appears only
after a backend-confirmed success with no errors or warnings; reduced motion uses
a static treatment and the result is always stated in text.

Run `npm run notices` after changing dependencies. It regenerates
`src/generated/third-party-notices.txt` from the Windows dependency graph and
the production npm packages; About loads it on demand.

## Stack and prerequisites

- Tauri 2.12.3, CLI 2.12.2, and Rust 1.94 or newer (stable MSVC toolchain, edition 2024).
- React 19, strict TypeScript 6, Vite 8, React Router 8.
- Tailwind CSS 4 with the official Vite plugin and CSS import.
- Node.js 24 LTS (minimum supported Node 22.22), npm only.
- On Windows: Microsoft C++ Build Tools with **Desktop development with C++**,
  the Windows SDK, and Microsoft Edge WebView2.

Run native commands in Windows PowerShell or a Windows developer terminal with
Node and Cargo on PATH. WSL can build and inspect the frontend and cross-compile
the Windows executable; the desktop application still runs only on Windows.
Install prerequisites using the official
[Tauri guide](https://v2.tauri.app/start/prerequisites/).

```sh
npm ci --ignore-scripts
npm run tauri:dev
```

The Vite development server binds only to `127.0.0.1:1420`. Packaged application
assets are embedded; production has no listener or browser-hosted mode.

### Building the Windows executable from WSL

In addition to Node and Rust, use `cargo-xwin`, LLVM (including `llvm-rc`), Clang,
and LLD (including `lld-link`) on PATH. Install the Rust target with
`rustup target add x86_64-pc-windows-msvc`. See the official
[Tauri cross-compilation guide](https://v2.tauri.app/distribute/windows-installer/#build-windows-apps-on-linux-and-macos)
and [cargo-xwin instructions](https://github.com/rust-cross/cargo-xwin).
NSIS is also required when producing the installer rather than the executable.

```sh
export XWIN_CACHE_DIR="$HOME/.cache/shifthappens-xwin"
export XWIN_ARCH=x86_64
npm run tauri:build -- --runner cargo-xwin --target x86_64-pc-windows-msvc
cargo xwin clippy --manifest-path src-tauri/Cargo.toml --locked --target x86_64-pc-windows-msvc --bins --lib -- -D warnings
```

The first build downloads Microsoft's SDK and runtime libraries. The executable
is `src-tauri/target/x86_64-pc-windows-msvc/release/shift-happens.exe`.
Launch it on Windows with WebView2 installed. This builds an executable without
an installer; cross-compilation alone does not verify Windows runtime behavior.
Installed startup uses the preservation workflow described below. Development
startup uses a separate disposable database. To inspect
the exact release SQLite linkage without opening application data, run the
executable with `--sqlite-engine-info` and capture its standard output. Close other
instances first: single-instance registration also precedes local CLI dispatch.
The existing diagnostic checks both SQLx and backup bindings and prints the
compiled migration versions/checksums without opening application data.

For a Windows debug executable with bundled frontend assets (no Vite server):

```sh
npm run build
cargo xwin build --manifest-path src-tauri/Cargo.toml --locked --target x86_64-pc-windows-msvc --features tauri/custom-protocol --bins --lib
```

## Commands

| Command                   | Purpose                                                     |
| ------------------------- | ----------------------------------------------------------- |
| `npm run dev`             | Frontend development view                                   |
| `npm run preview`         | Local inspection of the built frontend                      |
| `npm run format`          | Format source and configuration                             |
| `npm run format:check`    | Check frontend/documentation formatting                     |
| `npm run typecheck`       | Strict frontend and Vite configuration type checks          |
| `npm run lint`            | ESLint, with warnings denied                                |
| `npm run build`           | Synchronize version, type-check, and build bundled assets   |
| `npm run rust:fmt`        | Rust formatting check                                       |
| `npm run rust:clippy`     | Rust static analysis, with warnings denied (Windows)        |
| `npm run tauri:dev`       | Launch the native development app (Windows)                 |
| `npm run tauri:build`     | Build the release executable without an installer (Windows) |
| `npm run tauri:installer` | Build the current-user NSIS installer (Windows)             |
| `npm run release:windows` | Run fail-fast local internal-candidate build/audit gates    |
| `npm run tauri -- info`   | Inspect desktop environment and configuration               |

Repository policy in `AGENTS.md` requires direct verification and forbids tests,
test setup, and testing dependencies. No test assets or test commands are added.
Inspect relevant code and commands before running checks. Directly exercise the
app, navigate all routes and history, check keyboard focus and resizing, and
inspect runtime requests. Rust formatting/static analysis and frontend builds
supplement these manual checks; they do not establish native runtime behavior.

## Identity and version

Product/window title: **Shift Happens**. Main window: `main`.
Identifier: `com.diddymoe.shifthappens`. Publisher: **DiddyMoe**. Initial version: `0.1.0`.

`package.json` is the version source of truth. Tauri reads its version from
`../package.json`; Vite derives the displayed version from the same file.
`scripts/sync-version.mjs` synchronizes the Cargo package and lockfile versions, both during
`npm run build` and npm's `version` lifecycle. Use `npm version patch --no-git-tag-version`
to change the version and npm lockfile together. Keep `package-lock.json` and
`src-tauri/Cargo.lock` with the application. Installer, executable, startup report
and About use the same version. A development database reset does not
change the application or migration versions.

## Desktop security

Only the explicitly selected `main` capability is enabled, scoped to the local
Windows main window. Its permission list grants typed settings, database health,
employee, shift-template, position, coverage and holiday CRUD/archive, holiday
date/rotation/group/collision previews, schedule read/lifecycle/current-selection commands, interval previews,
weekday default selection, employment default preview/reset,
and restriction CRUD/archive, using generated application
permissions from `build.rs`'s supported `AppManifest::commands` list. The first-party
Tauri v2 dialog plugin has only `dialog:allow-save`, scoped to the Windows main
window. No frontend filesystem permissions are granted. Backup commands use
the same central command registration and ACL generation. `src-tauri/src/commands.rs` is
the single command-registration point; `src/lib/ipc.ts` mirrors its tagged result
contract. Unknown, read-only, and schedule-owned settings cannot be updated through IPC.

The [Tauri CSP configuration](https://v2.tauri.app/security/csp/) permits bundled
assets and Tauri's `ipc:` / `http://ipc.localhost` transport, which is a custom
protocol rather than a REST listener. Production allows no remote assets, inline
scripts, inline styles, evaluation, frames, or arbitrary network connections.
The separate development policy permits only the exact Vite HTTP/WebSocket
endpoint and inline styles for CSS hot reload. Release inspector support is not
enabled. Fonts use the local Windows/system font stack; the icon is bundled.
Capability selection follows the official
[Tauri capability documentation](https://v2.tauri.app/security/capabilities/).

Local confidentiality relies on the Windows account and filesystem permissions.
Do not place sensitive personal details, secrets, or local scheduling data in
source control or logs.

## Local values

`src/lib/localDate.ts`, `localTime.ts`, `localDateTime.ts`, and `localIntervals.ts`
parse validated immutable values and return explicit success/error results.
Canonical formats are `YYYY-MM-DD`, `HHMM`, and `YYYY-MM-DDTHH:MM:SS` without
timezone suffixes. Dates range from `0001-01-01` through `9999-12-31`.
Native time controls convert explicitly between `HH:MM` and `HHMM`.
Gregorian leap-year arithmetic and weekdays use integer day indices, and
intervals use nonempty half-open `[start, end)` semantics. Adjacent intervals
do not overlap. No helper parses a schedule value as a JavaScript `Date` or
converts it to UTC. Scheduling will use wall-clock minutes without
daylight-saving adjustments.

The equivalent Rust helpers in `src-tauri/src/local_values.rs` provide exact
parsing/formatting, checked calendar arithmetic, explicit datetime day offsets,
whole wall-clock minute differences, half-open overlaps, configured workweeks,
and anchored 14-day ranges. Dates before the payroll anchor use floor division.
Arithmetic that crosses the canonical date boundaries returns an error.
`text.rs` normalizes notes, validates Unicode text limits, and provides NFC plus
locale-independent lowercase ordering with stable IDs. `metadata.rs` supplies
the injectable clock, canonical UTC timestamps with nine fractional digits,
and precision-preserving RFC 3339 instant parsing/comparison for older metadata.

## Database foundation

`database.rs` resolves the stable Tauri v2 application-local data directory.
The installed path is `%LOCALAPPDATA%/com.diddymoe.shifthappens/shift-happens.sqlite`.
Debug startup uses its `development` subdirectory; there is no working-directory,
repository, install-directory, or temporary-directory fallback.

Changing the application identifier selects a different application-data directory.
To keep data from a build with a previous identifier, create a backup in that build
and restore it through **Backup** in the new build. The new build does not move or
delete the previous directory automatically.

The stages are explicitly callable: `resolve_database_paths`,
`create_owned_directories`, `inspect_database_file_set`, `claim_new_database`,
`inspect_migration_state`, `apply_embedded_migrations`, `initialize_startup_wal`,
`open_normal_pool`, and `run_post_schema_initialization`. `startup.rs` is the common
development/installed decision owner; the development wrapper calls it rather
than bypassing preservation. The first registered plugin is first-party
`tauri-plugin-single-instance` 2.5.2. A second launch only restores/shows/focuses
the existing main window, before CLI dispatch, app-data resolution or database work.

Startup distinguishes absent, uninitialized empty, current and pending databases.
Ambiguous file sets, access failures, integrity/foreign-key failures, dirty or
non-prefix histories, checksum mismatch, unknown/newer versions and missing
migration metadata block normal startup. Blocking reasons have that deterministic
precedence. Existing databases are inspected through a create-disabled read-only,
normal-locking connection in one read transaction; exact compiled versions and
SHA-384 checksums must match an applied prefix. Current startup uses `quick_check`;
pending startup additionally requires full integrity and foreign-key checks.
Inspection closes before the preservation/migration decision. Main and directory
file identities and safe sidecar types are rechecked at stage boundaries; SQLite
housekeeping establishes the next boundary rather than retaining the absent/empty
observation after authorized writes.

Only pending migrations create an app-owned pre-migration backup in
`migration-backups` beside the database. The existing SQLite backup primitive
uses `pre_migration_strict`, an initially absent unique destination and no-replace
publication. Full integrity, zero foreign-key violations, synchronization, closed
destination handles and destination revalidation are required before migration.
Failure blocks the migrator. Migration/full post-migration health failure preserves
and reports an already published backup. Optional `backup_runs` metadata is written
only after migration, pool creation and the ordered initializer; a metadata error
is a warning, not loss of the authoritative backup. Old backups are never deleted.

Blocked startup retains a narrow `StartupContext` and the main shell, with version,
path, reasons, stage and backup result. Physically readable, safely classified
sources offer an explicit **Save Preservation Backup** using the usual save dialog
and overwrite protocol with `manual_preserve`. This works without normal `AppState`,
pool, seeders or `backup_runs`; logical foreign-key violations are preserved with
a warning. Domain commands remain unavailable and successful preservation leaves
startup blocked. Blocked startup also offers **Restore from backup…** (see
[Restore from a backup](#restore-from-a-backup)); there is no continue, repair or
downgrade action.

Only an exactly absent and immediately rechecked main/sidecar file set can be
claimed with OS create-new semantics. The closed claim is inspected without
following links and must be a zero-length regular main file without sidecars.
Every main-database SQLite connection has creation disabled. Migration SQL cannot
change journal mode. One explicit startup connection sets WAL only when needed;
each normal pooled connection verifies WAL, foreign keys, FULL synchronous mode,
and a 5,000 ms busy timeout. The pool is limited to four connections. Read-only
health/inspection connections use normal SQLite locking and observe committed WAL data.

SQLx 0.9 uses one bundled `libsqlite3-sys` linkage, locked to SQLite 3.51.3.
Startup queries its actual version and source ID and rejects engines without
the [official WAL-reset fix](https://www.sqlite.org/wal.html#walresetbug).
`migrations/0001_app_settings.sql` creates `app_settings` and
`0002_employees.sql` adds `employees` and `employee_restrictions`;
`0003_shift_templates.sql` adds `shift_templates` and
`employee_shift_template_restrictions`; `0004_positions_coverage.sql` adds
`shift_positions`, `coverage_templates`, `coverage_template_items`, and
`employee_position_restrictions`; `0005_availability_pto.sql` adds separate
`availability_entries` and `pto_entries`; `0006_holidays.sql` adds `holidays`; `0007_schedule_storage.sql` adds
`schedules` and non-archivable `schedule_assignments`; `0008_backup_runs.sql` adds
operational backup attempt history; `0009_schedule_deletion.sql` adds
`schedules.finalized_once` and limits deletion to never-finalized drafts.
SQLx owns its
migration-history table. The centralized embedded migrator and build-script
directory watch include newly added migrations without an unrelated Rust edit.

The ordered post-schema initializer owns one transaction. It seeds typed settings,
seeds the eleven canonical shifts directly after settings, then positions and coverage,
validates retained values, weekday defaults, active item intervals, restrictions and
metadata, and commits once; failures roll back and
prevent managed state. Later seeders extend this same call site. Seeding preserves
existing values, types, and revisions. Six manager settings are editable with an
expected revision; unchanged requests preserve revisions and timestamps. Invalid
stored settings produce configuration errors and are never silently repaired.
The payroll anchor starts null and Settings explains how to configure it.

## Employees and restrictions

Employees stores an explicit display name and all scheduling capabilities, holiday
group, time preference, minimum/maximum rules, explicit availability requirement,
weekend rotation, and notes. Full-time defaults to 2,400 paid minutes per workweek;
part-time defaults to 2,400 per anchored two-week period. PRN has no default minimum
and no blanket availability requirement. Automatic extra PRN assignments on ordinary
non-holiday weekdays require availability covering the complete actual shift;
regular weekend/holiday rotation has no blanket availability requirement.

Changing a saved employment type retains its rules. The revision-aware reset
previews and changes only minimum metric/period and explicit availability. A
conflicting retained maximum blocks reset without mutation. Every update/archive
requires the expected revision; normalized no-change requests retain metadata.
Archive sets inactive, preserves history and restrictions, and is permanent in V1.
The archive service checks active configuration dependencies in its transaction.
Employee exclusions, including template restrictions, remain historical and do not
block employee archive.

Weekend Sunday anchors normalize to the preceding Saturday; weekdays reject.
Parity uses operational dates and works before the anchor. Category and recurring
weekday/time restrictions use typed transactional services and SQLite shape,
foreign-key, and partial unique-index constraints. Recurring end-before-start
extends into the next actual date. `RestrictionIndex` merges candidate source-date
windows from the prior day through the actual end date and expands each source
occurrence once. Its shared `ExpandedIntervalBudget` caps expanded intervals at
250,000 per analysis snapshot; availability/PTO callers share that budget.
Half-open overlap preserves adjacent boundaries. An out-of-window evaluation or
date overflow blocks rather than returning a partial eligibility result.

The Employees screen includes search/status/type filters, all scheduling fields,
integer minutes with hours/minutes display, reset/archive confirmations, restriction
editing, and explicit stale-revision reload. Scheduling and eligibility decisions
remain authoritative in the canonical Rust services. Installed-data upgrades pass
through the backup-before-migration startup workflow; do not reset a manager's
database to bypass a blocking classification.

## Shift templates and paid time

Shift Templates provides search/status filters, create/edit
forms, overnight labels, independent day/evening/night categories, recovery/night
flags, optional labels/colors/notes, and archive/deactivation confirmation. Colors
are decorative: category and flags always have textual labels. Validated SVG fill
attributes show swatches without relaxing the production CSP.

`shift_time.rs` is the canonical backend calculation for create/update, seed
validation, and authoritative previews. It parses exact `HHMM`, rejects equal
clocks, and allows one overnight crossing for template duration. Elapsed time is
1–1,439 wall-clock minutes. Lunch is exactly 30 unpaid minutes at 480 elapsed
minutes or more, otherwise zero. Paid time is elapsed minus lunch; an eight-hour
elapsed shift therefore has 450 paid minutes. SQLite independently constrains the
same derivation, positive paid minutes, clock formats, enums, and integer fields.
There are no authoritative lunch/paid fields in mutation requests. `shiftTime.ts`
uses existing local-time helpers for immediate non-authoritative previews; the
form separately shows the backend preview and saved result. No DST adjustment
or operational coverage offset is inferred.

The eleven seed rows use immutable keys protected by a unique index across all
statuses and an update trigger. Startup validates canonical seed calculations and
retained shapes while preserving every manager edit, inactive status, archived row,
revision, and timestamp. There is no second seeding hook or seed-owned transaction.

Template restrictions are reachable from each saved employee and from Shift
Templates. New links require active, non-archived employee/template sources;
reactivation or moving a link rechecks those sources. A partial unique index
prevents duplicate active exclusions. Restrictions survive source retirement and
can still have notes updated, be deactivated, or be archived; they do not apply
while the restriction or either source is inactive/archived. Restrictive foreign
keys preserve references. All mutations use expected revisions, normalized
no-change detection, and transactions; archive is permanent and physical deletion
is not exposed.

Shift, position and coverage-template retirement share one transaction-scoped
source-dependency service. Active coverage items block retirement and invalidating
shift-time changes. Dependency results include counts and bounded item IDs/names.
Existing databases requiring migrations pass through the startup preservation
guard; no manager database is reset to bypass it.

## Positions and stored coverage

Nine positions and seven selected weekday templates seed 42 items from the exact
matrix in `prompts/00_Shared_Context_and_Rules.md`. Monday/Tuesday/Thursday/Friday
have six items each; Wednesday has seven, Saturday six and Sunday five. Saturday
uses T8, Sunday uses T12 and omits T5. Wednesday Midnight is operational Wednesday
but actual Thursday 0015–0700 with offsets 1,1; Wednesday T5 crosses midnight with
0,1. Canonical seed keys are immutable and retained manager edits are never reset.

Coverage configuration stores explicit offsets 0,0, 0,1 or 1,1, integer sort order,
and required count 1–100. The backend constructs actual civil intervals and requires
positive elapsed time equal to the linked shift's wall-clock duration. Labels and
sort order cannot distinguish duplicate active position/shift/offset combinations;
additional slots use required count. Generation reads the selected stored coverage.

A partial unique index permits at most one active normal default per weekday;
transaction validation requires exactly one populated default for all seven days.
Manager-created alternatives start nondefault and may be empty. Only the dedicated
revision-aware default-selection command swaps defaults atomically. It checks both
the target and current default revisions, source activity and populated coverage.
Holiday custom templates never become normal defaults. Archive, deactivation or
moving the last active item cannot empty a selected default; select a populated
alternative first. All rejected mutations roll back their complete transaction.

Inactive and archived items retain historical offsets even after a permitted source
time change or archive. Startup checks their own shape and source existence, while
active items and reactivation also check current source activity and valid offsets.
Archived records have no reactivation path. Position exclusions use revision-aware
CRUD/archive, active-source checks and partial duplicate prevention. They are linked
from Employees and managed on Positions and Coverage; they remain historical when
sources retire. Generation, editing, and validation apply the active exclusions.

## Availability and PTO

The manager-only Availability and PTO screen has separate lists and forms with
employee, inclusive source-date, activity/archive, availability-type, and PTO-status
filters. Full-day entries have no times. Partial entries use distinct `HHMM` times;
an earlier end time finishes the following actual day. Every date in a multi-day
source range repeats that window independently, as explained by the form preview.
Notes use the shared operational-text limits. Stale updates offer an explicit reload;
archive retains history permanently and sets inactive.

Migration `0005_availability_pto.sql` uses restrictive employee foreign keys,
canonical date/time and enum constraints, integer boolean/revision constraints,
archive metadata, and employee/date/type/status indexes. No supported migration
contains `recurrence_rule_json`, so the converged schema omits it. Requests reject
unknown fields, and neither a recurrence command nor recurrence controls exist.
Existing migrations are unchanged. Installed and existing meaningful development
databases with pending migrations use the same preservation workflow.

`availability_pto.rs` provides typed transactional create/get/list/update/archive
services. Every write leaving an entry active rechecks an active, non-archived
employee. Deactivation and archive remain possible after employee retirement.
Expected revisions protect updates/archive, and normalized no-change requests
preserve metadata. Startup verifies retained entry shape, employee links, and
metadata without imposing present-day employee activity on historical rows.

`entry_intervals.rs` selects bounded merged source-date windows before expanding,
keeps prior-day overnight carry-in, excludes an exact-midnight end from preferred
relevance, and retains complete half-open intervals. `AvailabilityPtoIndex` is an
immutable index reused across candidate reads. Callers share the existing
`ExpandedIntervalBudget` with `RestrictionIndex` for the entire snapshot, counting
occurrences before coalescing/materialization. `SourceRowBudget` independently
bounds selected physical rows, deduplicated across windows. Bounds errors return
no partial eligibility result.

`evaluate_availability` loads an owned snapshot in one read transaction, releases
it, and evaluates on the per-request blocking boundary. It returns note-free stable
codes, bounded sorted source IDs with omitted counts, and material warning IDs
computed from complete relevant facts before display truncation. `rule_facts.rs`
uses streaming SHA-256 for versioned deterministic identities; names, notes, and
unrelated revisions do not enter them.

Approved PTO, unavailable, and hard block produce hard conflicts. Requested PTO
is soft; denied/canceled PTO has no effect. Available intervals coalesce, including
adjacent boundaries, before complete-coverage evaluation. Preferred entries are
relevant on the actual civil days touched even when they do not overlap. Explicit
facts separately report available overlap, full coverage, preferred relevance,
preferred overlap, and positive-availability completeness. Positive availability
never cancels a hard conflict. Generation, editing, and validation use full
coverage for PRN extra ordinary non-holiday weekdays and suppress the redundant
PRN warning when the blanket explicit-availability rule already blocks.

## Holidays

Holidays edits annual fixed dates, first-through-fourth weekdays, last weekdays,
and one-time exact dates. Annual February 29 is rejected; a leap-day holiday uses
an exact date. No observed/substitute dates or external calendars are added.
The eight seeds use immutable keys and normal coverage: Memorial Day, Labor Day,
Christmas Eve and New Year's Day in Set A; July 4th, Thanksgiving, New Year's Eve
and Christmas Day in Set B. Startup calls the holiday seeder directly after
settings, shifts, positions and coverage in the same initializer transaction.
Retained manager edits, inactive records and archives are preserved.

`holidays.rs` resolves civil dates, reads the typed rotation-start setting, and
uses even/odd year differences including years before the baseline. Set `none`
has no group requirement; employee group `none` mismatches Set A/B. Date and
range resolution use deterministic date/name/ID order. Collision previews report
all colliding dates with bounded diagnostic details; collisions remain valid
saved configuration and do not block startup. The preflight guard is ready for
the generator. Collisions block generation before any schedule is inserted.

Revision-aware transactions protect create/update/archive and reject stale,
invalid or dependency-breaking changes without mutation. The canonical coverage
dependency service and SQLite guards protect active custom references from
retirement, day-type changes and last-active-item removal or movement. Inactive
and archived holidays retain their FK without requiring a currently usable
template; every activation rechecks populated active holiday-custom coverage.
Archive is soft and permanent. The manager screen shows filters, rule editing,
year/date and rotation previews, collisions, valid custom sources, archive
confirmation, and explicit stale-revision reload.

## Stored schedules

Schedules retain immutable operational ranges and canonical version-1 generation
settings JSON. The snapshot captures generator version, typed setting values and
revisions, coverage selection by operational date, and an integer
`expected_assignment_count` from 1 through 100,000. Generation captures the complete
slot count and checks it against persisted rows before commit; no IPC command inserts
a metadata-only schedule. The setting/coverage snapshot records generation context,
not every employee or source row, and does not guarantee reproduction after edits.

Assignment rows capture coverage, position, shift policy, optional holiday, and
assigned employee names. Explicit coverage offsets determine actual local intervals.
Structural snapshots cannot change, and unique slots use schedule/date/item/index.
The retained migration inventory never contained `operational_day_label`, so it is
absent from storage. Read DTOs derive English weekdays from the canonical Rust helper.
Historical reads use captured facts even after sources are renamed or archived.

`schedule_assignments.rs` owns typed creation, editable-target, and backend-captured
replay intents inside the caller's transaction. Complete plans start at revision 1;
later changed assignment rows and their parent each advance once. Narrow editor
commands enforce eligibility, confirmation, and history authorization.
`schedule_write.rs` supplies one process-wide asynchronous coordinator and one
SQLx `BEGIN IMMEDIATE` entry point. Lifecycle/current commands acquire it once;
inner services never acquire it or start another transaction.

Only non-archived drafts allow metadata/assignment changes. Final rows remain
readable and require confirmed reopening; archive retains both statuses. Current
selection reports `none`, `current`, or `stale_setting`, with revision-aware set/clear.
Archiving the selected schedule clears its setting in the same transaction.

`delete_schedule` removes scratch drafts. Only a draft that was never finalized can
be deleted, archived or not. Finalization sets `finalized_once`, which reopening keeps,
so final schedules and drafts that were once final stay as payroll/hours records
(29 CFR 516 retention) and can only be archived; deleting one returns "Schedules
that were finalized are kept as records. Archive it instead." The request carries the
expected revision and `confirmed`. `confirmed: false` returns `requires_confirmation`
with the name, slot count and whether it is current, and writes nothing. Confirmed
deletion runs under the schedule-write coordinator in one `BEGIN IMMEDIATE`
transaction: it clears the current selection if it points there, archives an active
draft, deletes its slots and then the schedule, verifies the counts and commits, then
clears editor history. Triggers enforce the same rule in storage: schedules and slots
can be deleted only for an archived draft with `finalized_once = 0`, and
`finalized_once` can never be cleared. The only public finalization command is `finalize_schedule`; there is no raw-status,
generic patch, lifecycle-only finalize, or quick-finalize bypass.

Finalization requires an explicit schedule ID, exact expected revision, and a nullable
exact warning-ID set. Duplicate IDs are rejected before sorting. Under the process-wide
schedule-write coordinator it starts one real `BEGIN IMMEDIATE` transaction, rejects
archived schedules, checks the revision before the already-final branch, and runs the
same bounded validator used by public validation against that transaction. Errors,
incomplete analysis, or truncated findings block mutation. Null and empty acknowledgements
both mean the empty set; unknown, partial, superset, changed, or obsolete nonempty sets
return fresh confirmation context. Every retry rechecks current source facts. Warning
identity binds material facts, so same-ID PTO interval changes invalidate confirmation
without needing a schedule revision change; unrelated names/notes do not.

Only a complete error-free validation with exactly acknowledged current warnings
changes draft to final, advances metadata and revision once, and commits. History is
cleared while the coordinator is still held. If auxiliary history maintenance fails,
the committed finalization remains successful with `history_unavailable`, and the
quarantined in-memory store makes stale undo/redo unreachable. An already-final schedule
with the current revision returns `no_change` without validation or a new revision;
a stale revision still returns `stale_revision`.

The Schedule workspace uses this confirmation for Finalize….
It identifies the plan and each warning's facts, including every open shift. Finalization
locks editing and may include acknowledged warnings/open shifts; it does not mean all
coverage is filled. The returned counts describe that operation, never a stored approval
history or permanent clean-policy status. Later validation uses current policy. Explicit
reopening starts clean editor history. No findings or acknowledgements are persisted.

Asynchronous reads and confirmations capture IDs and revision context, and obsolete
responses are discarded. Typed IPC emits small completion/invalidation notifications
through the existing React state mechanism. A mutation completing after navigation
retains its original record outcome in the shared shell. Validation results are labeled
last checked and cleared by local successful writes; backend revalidation is authoritative.
Native modal confirmations keep focus inside, restore it on close, and refuse Escape
while saving.

## Deterministic schedule generation

`generation_inputs.rs` owns note-free bounded reads and structural preflight;
`generation.rs` owns deterministic greedy selection and warnings. The public
`generate_schedule` command takes a name, canonical inclusive start/end dates,
make-current flag, and an expected current-setting revision only when making
current. It acquires the existing process coordinator and one `BEGIN IMMEDIATE`
transaction before settings/source reads. The complete preflight and in-memory
selection finish before storage writes anything. The existing assignment service
inserts every assigned/open slot at revision 1, verifies the captured count, and
optionally changes current inside that transaction. Any failure rolls everything
back. No migration or new dependency was added for this phase.

Normal dates and normal holidays use exactly the selected active nonempty weekday
default. Custom holidays use their active populated custom template; collision,
missing/inactive links, bad offsets, elapsed-duration mismatch, corrupt snapshots,
and duplicate slots are fatal. Slot order is operational date, template seed key
or ID, item sort order, item seed key or ID, slot index, actual start, then item ID.
Actual datetimes retain the explicit coverage offsets, including Wednesday Midnight
on Thursday 00:15–07:00. Runtime never uses a hard-coded coverage matrix.

Hard exclusions cover approved PTO, unavailable/hard blocks, full positive
availability when required, PRN extra-weekday full coverage, recurring/category,
shift and position restrictions, night/ORTHO capabilities, weekend/holiday
capabilities, automatic rotation/group eligibility, maximum paid minutes, and
employee overlap. The shared indexes expand once, including restriction carry-in
and restrictions starting after midnight. Supported storage has no availability
recurrence or legacy preference fields; unknown/corrupt stored shapes fail closed.
Active two-week minimums or maximums with a null payroll anchor block preflight.

Candidate ranking is the following ascending lexicographic tuple, with no flat
conflict count:

1. Recovery deficit minutes, then recovery pair count.
2. Remaining rest deficit minutes, then remaining rest pair count.
3. Complete consecutive-run excess days, including operational-date bridges.
4. Requested PTO overlap (none first), then projected weekly overtime excess.
5. Current minimum class (complete-period unmet, non-authoritative/no minimum,
   complete-period met), then projected minimum completion in integer basis points.
6. Projected paid minutes in the operational workweek, whole-schedule paid minutes,
   and whole-schedule assignment count.
7. Soft-incomplete rank, availability preference (overlap, none, unmet), time
   preference (match, none/unevaluable, mismatch), normalized Unicode display-name
   key, and employee ID.

A slot that meets the final missing minimum amount stays in the unmet class for
that selection; later slots see the updated total. Completion is capped at 10,000;
missing/non-authoritative completion also uses 10,000. Stored assignment-count and
paid-minute minimums use their respective metrics, including custom PRN minimums.
Recovery suppresses ordinary rest for the same pair. Equivalent soft source rows
only affect diagnostic provenance, not ranking. Chronological interval maps,
maximal worked-date runs, and incremental weekly/two-week/whole totals avoid prior
assignment rescans. Candidate loops have no database handle or per-candidate reads.

Shared ceilings are 1,000 active employees, 250,000 filtered physical source rows,
250,000 expanded occurrences, and 2,000,000 planned slot/employee evaluations;
the separate slot limit is 100,000 and range limit is 370 inclusive days. Limits
are checked before per-slot candidate work; input/expansion overflow returns
`generation_input_limit_exceeded` with limit kind/count, and the slot ceiling uses
`planned_slot_limit_exceeded`. Nothing is partially persisted or returned.

When no hard-safe automatically eligible candidate remains at that greedy step,
the slot stays open. Otherwise soft conflicts never remove a qualifying employee.
Structured warning IDs use the shared material-facts identity, planned identities,
actual intervals, employee/source facts and policy values, excluding fresh schedule
and assignment IDs, names/notes, insertion order, and metadata time. The response
retains at most 9,999 ordinary details, then a final truncation sentinel. Discarded
contexts are neither constructed nor hashed; checked counts still include every
ordinary warning, and selection is unchanged. `total_warning_count` counts ordinary
findings; the sentinel is a response marker. Validation is the current-policy review and blocks if its own safety ceiling
is reached.

The manager screen defaults to the current local calendar month, offers a custom
range, previews canonical dates, checks configuration, exposes the conditional
anchor blocker, prevents repeated submission, preserves inputs after errors, and
opens the saved draft in the Schedule workspace, with Show issues for its findings.
Minimum/maximum/overtime totals use only this schedule; complete periods alone
have authoritative minimum priority. Boundary checks cannot prove facts about
immediately adjacent work outside the schedule or other systems. Minimums may
exceed available coverage, and open slots do not prove that every possible
assignment order would fail.

### Local generation diagnostic

The Windows executable provides an explicit CLI diagnostic that calls the same
real service and prints sanitized counts, warning identities, source/read/evaluation
counts, and elapsed generation milliseconds. It creates a complete draft in the
specified existing development database. It contains no assertions, timing gate,
roster fixtures, or test runner. For a new disposable directory named `development`,
`--initialize-development` invokes the application's normal guarded startup and
seed operation; it never bypasses installed-data upgrade safety. That initializer
is available only in debug builds. The request is one JSON argument with the same
fields as the IPC command:

```sh
shift-happens.exe --generation-diagnostic 'C:\path\development\shift-happens.sqlite' '{"name":"Local diagnostic","start_date":"2026-10-01","end_date":"2026-10-31","make_current":false,"expected_current_setting_revision":null}'
```

For repeated measurements, keep the source database, requested dates, build mode,
CPU/thread configuration, storage, and cache conditions explicit. Different schedules
remain alternative plans and do not change subsequent generation workloads.
Do not infer a representative 15-employee performance result from an empty roster.

## Manual schedule editor

The Schedule workspace defaults to the current schedule only in the UI. Every backend
candidate read, edit and replay names its schedule explicitly. `edit_schedule_assignment`
accepts only assign, open, note, lock and unlock actions with assignment and parent
revisions. `get_assignment_candidates` returns current blockers, warnings, replacement
projections and setting revisions; it releases its consistent read transaction before
CPU analysis. `get_editor_history` and `replay_schedule_assignment` expose session-only
undo/redo, with exact backend entry identity, direction and live revisions.

Candidates, edits and assignment-changing replay reuse the generator evaluator and
bounded rule indexes. A replacement subtracts the existing row before adding the
new employee once. Hard eligibility conflicts cannot be acknowledged. Requested PTO,
preferences, rest/recovery, consecutive days, overtime, off-rotation weekends, holiday
group mismatch, missing complete PRN weekday availability, and worsened authoritative
source minimum shortfalls require acknowledgement. Missing blanket explicit availability
and weekend/holiday capabilities remain hard blockers.

The first warning-bearing attempt changes nothing. Confirming submits the exact current
material warning IDs with unchanged revisions; duplicate, partial, removed or changed
warnings do not authorize a write. IDs bind relevant intervals, totals and rule facts,
exclude display names/notes, and use a distinct entry/direction identity during replay.
No acknowledgement is persisted. Note and lock replay does not re-evaluate unchanged
employee eligibility. Employee snapshots restore exactly as captured, while structural
slot/time/coverage snapshots are immutable and revisions only advance.

History retains at most 100 actions per schedule and clears redo after a new edit.
Schedule metadata/lifecycle changes invalidate it after commit. Live database drift
also clears history; delayed caller requests do not discard otherwise valid history.
History maintenance cannot turn a committed database write into a retryable failure;
it disables history and returns a nonfatal warning if safe maintenance is unavailable.
Restart clears history. Final schedules require explicit confirmed reopening.

All projections are scoped to the selected schedule. Partial minimum periods are not
authoritative, and absence of overlap/rest/consecutive findings at a boundary makes
no claim about work outside the date range or another system. Paid time uses civil
wall-clock minutes without daylight-saving adjustment. Input/expanded-interval and
complete diagnostic limits fail closed, without partial eligibility or mutation.

## Read-only schedule validation

`validation_snapshot.rs` loads an owned, note-free snapshot through a caller-owned
consistent SQLx transaction. `validation.rs` evaluates it without a database handle.
The public `validate_schedule` command requires an explicit schedule ID and accepts
draft, final and archived plans. It opens a dedicated real read-only, create-disabled
SQLite connection using normal locking, finishes its read transaction and closes the
connection before CPU work. It never toggles pooled connection state, rewrites rows,
persists findings, changes current selection or applies fixes. The same loader and
pure evaluator runs inside the canonical finalization write transaction. No new
migration, policy setting or dependency is introduced.

Snapshots preserve historical shift, coverage, position and holiday rules; actual
datetimes remain authoritative. Current employee rules and policy apply deliberately.
A missing legacy snapshot may use its linked source with `incomplete_data`; a present
malformed snapshot is never replaced with current source data. Unsupported or wholly
unparseable generation audit metadata warns without inventing an expected count.
Version 1 count violations are blocking `incomplete_schedule_slots`. Missing days,
slot gaps/excess and inconsistent sibling counts are checked from stored facts.
Complete open slots warn about staffing. Count evidence does not certify exact
historical group membership against coordinated edits.

The complete typed error/warning code reference is returned with results and is
available in the Issues panel. Findings use the shared versioned SHA-256
material-facts builder, canonical entity IDs and relevant intervals, policy values,
source facts, totals and deficits. Full bounded source material is hashed before
display truncation; names, notes, generic source revisions and display messages are
excluded. Context includes the relevant current setting revisions. Identity is the
same for public validation and caller-owned transaction validation.

Minimums include zero-assignment active employees and apply only to complete covered
weekly or anchored two-week periods. Stored metrics determine FT/PT/custom codes;
PRN has no invented minimum. Partial-period maxima and overtime are conclusive only
from included rows. A null anchor yields one schedule warning and an employee hard
error for each unevaluable two-week maximum. Corrupt membership, interval or paid
facts suppress dependent workload claims; independent checks continue.

Overlap uses a deterministic output-sensitive sweep. All error passes finish before
warning passes. Rest/recovery uses original chronological adjacency after a complete
overlap pass, suppressing pairs involving overlap participants or corrupt intervals
and never splicing their neighbors. Unorderable rows suppress rest for that employee.
Recovery suppresses ordinary rest for the same pair. Consecutive days use distinct
operational dates and maximal runs. All findings are schedule-local; clean boundary
results cannot establish facts about adjacent work in other plans or systems.

Shared input bounds are 370 days, 100,000 assignments, 1,000 relevant employees,
250,000 filtered physical sources and 250,000 expanded interval occurrences. At most
9,999 ordinary findings are retained. The next emission stops every remaining pass,
and a blocking truncation sentinel occupies the reserved final slot. Ordinary
presentation order is error before warning, date, datetime, code, assignment ID,
counterpart ID and finding ID, with nulls last. The sentinel is always last.

The manager selects any saved schedule, runs or refreshes validation, filters
severity/code/employee, inspects optional technical context and opens related
assignments on the board. Results stay in memory and are rechecked after
changes reported by the app; Issues does not finalize schedules.

The Windows CLI also exposes the same read-only service for local troubleshooting:

```sh
shift-happens.exe --validate-schedule 'C:\path\development\shift-happens.sqlite' SCHEDULE_ID
```

It prints counts, codes and ordered finding IDs only, with no names, notes,
assignment contents or PTO details. It does not initialize or upgrade a database.

For Linux library inspection in WSL, install Tauri's GTK/WebKit development
dependencies, then use `cargo check --manifest-path src-tauri/Cargo.toml --locked
--lib` or `cargo clippy --manifest-path src-tauri/Cargo.toml --locked --lib -- -D
warnings`. The desktop binary remains Windows-only. Repository policy still
requires direct verification without creating or running tests.

## Full local SQLite backup

The Backup screen uses the first-party Tauri v2 save dialog, a backend-generated
filename and a typed command with only a destination and optional one-time
overwrite token. Cancellation and overwrite-required responses create no attempt
row. The screen prevents duplicate submissions, shows copying/checking progress,
requires explicit confirmation of the exact existing destination and displays
path, byte size, completion time, integrity, foreign-key warnings and last attempt.
An unavailable history table does not prevent file preservation. History is ordered
by parsed UTC metadata instants, then stable ID, including mixed RFC 3339 precision.

`backup.rs::create_consistent_sqlite_backup` is independent of Tauri state, the
normal SQLx pool and migrations. `backup_sqlite.rs` uses SQLite's documented
`sqlite3_backup_init/step/finish` API through the exact `libsqlite3-sys 0.37.0`
linkage already used by SQLx; no second SQLite engine or domain data layer is added.
The command verifies both bindings' loaded SQLite version/source ID and the
existing WAL-reset safety gate. A create-disabled, normal-locking read-only source
includes committed WAL data without changing source journal mode or domain rows.
The synchronous operation runs at Tauri's blocking boundary. Copy steps use 128
pages, the existing five-second busy timeout and a 60-second total monotonic budget
that includes restarts and successful incomplete steps. One in-flight SQLite step
can exceed the deadline by its busy timeout and finite page work.

Only the create-new, empty, unpublished backup temporary uses `MEMORY` rollback
journaling and exclusive locking. Exclusive locking keeps a copied WAL header
from allocating a disk shared-memory index. The writer verifies zero pages before
selecting `MEMORY`, so bulk copy does not retain a database-sized set of original
pages in memory or create a disk rollback journal that a disk-full failure could
strand. The backup object
must finish successfully, and the source connection closes before destination
processing. A copied WAL header is normalized under `MEMORY` first, so its header
rewrite also avoids a disk rollback journal. The destination then explicitly
converts to `DELETE` journal mode, and its SQLite writer closes before the exact
main file is synchronized. Every mode change must return the requested mode.
`MEMORY` does not provide crash recovery for the unpublished temporary; an abrupt
termination can leave a partial main file. It is never used for the source or an
existing published backup, and rollback journaling is never disabled.

A separate create-disabled read-only connection checks journal mode, full physical
integrity and foreign keys, then closes before publication. SQLite
header, size and absence of all destination sidecars are also checked. Manual
`manual_preserve` backups retain physically valid foreign-key violations with a
counted, bounded schema-only warning. `pre_migration_strict` rejects those findings
and requires an app-owned no-overwrite policy. No employee notes, assignment data,
row contents or SQL error text are logged. The pure primitive writes no metadata;
manual orchestration records actual success/failure best effort. A committed file
stays successful when later metadata, clock or cleanup work fails.

`safe_destination.rs` is the shared file output path for backup, XLSX/PDF, and
pre-migration consumers. It normalizes the required extension, resolves
existing parents (including parent junctions), rejects final links/reparse points,
device names and alternate streams, and protects the live database plus its exact
WAL/SHM/journal paths by canonical path and platform file identity. It permits other
files in the data folder. Temporary main files use create-new semantics with one
32-attempt limit. SQLite sidecars are inspected before and after the main claim;
unknown sidecars are never opened or removed. Final backup sidecars block output.
Overwrite confirmations live in a bounded 32-entry process-local store and are
consumed once for the exact observed file state. Parent/destination changes abort
without publication or automatic retry. All writers/readers/synchronizers close
before that final recheck. One shared publication lock serializes observations and
native commits across backup and export callers; format generation stays parallel.
A later invocation must reobserve an earlier commit before it can publish.

On Windows, no-replace publication uses same-directory `MoveFileExW` without
`MOVEFILE_REPLACE_EXISTING`, so a competing new entry cannot be overwritten.
Authorized replacement first creates a unique hard link retaining the original,
then uses same-volume `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING` and
`MOVEFILE_WRITE_THROUGH`; it never deletes the original destination first and
never falls back to cross-volume copying. A folder that cannot create the recovery
link blocks overwrite. After an OS error, observed new-file identity means
committed success with a warning; observed unchanged original means unpublished
failure. Any unconfirmed state retains the original recovery link, reports its path
and publication state, and blocks blind retry for that destination in the process.
Recovery files contain prior destination data and are not disposable temporaries.
Cleanup removes each verified invocation-owned disposable name at most once and
reports failures separately. Windows cleanup checks and marks deletion through the
same no-follow DELETE-access handle, without sharing rename/deletion access; it
never closes an identity check and then deletes an unchecked pathname. Recovery
lookup uses Windows path semantics, including case-insensitive names.
Owned Windows temporary and recovery entries retain their verified directory
handle. Cleanup resolves that directory's current name and verifies its identity
before deleting an owned entry, so an ordinary NTFS folder rename does not leave
an unreported temporary file. A filesystem that cannot resolve the held directory
returns a cleanup warning with the original artifact path; it is never treated as
successful cleanup. This does not authorize publication into a moved folder.
An abrupt process termination can leave temporary or
recovery names; there is no automatic deletion on restart.

File-content synchronization is mandatory before publication. The Windows move
uses write-through, but this is not a claim of crash-proof atomicity, directory
entry durability on every filesystem, or exhaustive protection from a privileged
actor deliberately racing the final operation. Prior file permissions are not
copied to the new file; the selected folder's inherited permissions protect it.
Backups are ordinary unencrypted local files. There is no cloud upload, retention
deletion, background schedule or updater control.

The Windows CLI exposes the same manual orchestration for local backup and recovery:

```sh
shift-happens.exe --backup-database 'C:\path\development\shift-happens.sqlite' 'C:\Backups\shift-happens.sqlite'
```

An existing destination prints its exact confirmation and waits for `REPLACE`;
anything else cancels. The CLI classifies its explicit local source first. It opens
a normal pool only for a current schema already in WAL mode. A current-schema
DELETE-mode source and a pending/pre-migration source are preserved without a
normal pool or history insertion. Backup never changes the source journal mode.
It never initializes or migrates a
source. The running GUI derives its source from application state; blocked
startup supplies only `ClassifiedStartupBackupSource` from trusted classification.

Repository policy prohibits creating or running automated acceptance suites. Direct
Windows application/CLI verification, independent SQLite inspection and the allowed
format, type, lint, Clippy and build checks are the qualification methods here.

## Restore from a backup

**Restore from backup…** on the Backup screen, and on the blocked-startup screen,
replaces all data with a chosen backup file. Restore is staged and applied by the
next startup, because the running database pool cannot be swapped in place:

1. **Check.** The open dialog's path goes to `stage_database_restore`. It must be an
   absolute regular file that is not a link, not the live database or a sidecar and
   not in `restore-staging`. The existing backup primitive copies it into
   `restore-staging/staged-<id>.sqlite` with `pre_migration_strict` (SQLite header,
   online copy, `DELETE` journal mode, full integrity check, zero foreign-key
   violations). Startup's read-only classification then accepts only a current or
   upgradeable Shift Happens history; a newer build's file and non-Shift Happens files
   are refused. The preview shows the file, size, employee/schedule counts, the
   latest schedule end date and whether it will be upgraded. Backup and restore share
   one in-flight gate, and the single-use confirmation lives only in memory.
2. **Confirm.** `apply_database_restore` rechecks the staged copy's length and
   SHA-256, waits for schedule writes, closes the pool, and saves the quiescent live
   data to `restore-backups/shift-happens-before-restore-<time>.sqlite`
   (`manual_preserve`, no history row). It then publishes `restore-pending.json`
   (temporary file plus native move) and restarts through Tauri's exit path, which
   releases the single-instance lock first. A failure after the pool closed removes
   the staged copy and still restarts.
3. **Apply.** Startup handles the marker right after creating its folders and before
   observing the live file set. It verifies the staged copy, records
   `restore-backups/replaced-<time>/` in the marker, moves the live database and any
   `-wal`/`-shm`/`-journal` into it (retrying for about ten seconds while Windows
   releases the files), moves the staged copy into place, verifies its hash and
   removes the marker. Normal classification follows, so an older backup gets the
   usual pre-migration backup and migration. An invalid marker or staged copy is set
   aside as `restore-backups/abandoned-restore-*.json` with a warning, and
   interrupted moves are resumed or rolled back. While a marker remains, startup
   never creates a new database; a persistent failure blocks with "Close all Shift
   Happens windows and open it again."

During blocked startup there is no pool, so no safety copy is made; the move-aside
keeps the blocked files instead, and the screen suggests saving a preservation backup
first. The Backup screen reports the outcome of a restore applied at this startup.
Nothing in `restore-backups` is deleted automatically; unconfirmed staged copies are
removed at the next startup.

## Schedule export and printing

Export… and Print… act on the schedule open in the Schedule workspace, including
final and archived schedules. XLSX, PDF and print share one bounded historical DTO
loaded in a single read-only SQLite transaction. The reader and transaction close
before font preflight, layout, file generation, sync or independent parsing. No
schedule, source, setting, revision, timestamp or backup-history write occurs.
Separate requests capture their own revisions, which can differ after edits.

The workbook has one `Schedule` sheet, six metadata rows and the exact twenty
assignment columns, with a frozen header, column widths and wrapped literal cells.
Paid minutes/lunch are integers, with a separate H:MM string (including totals
above 24 hours). All strings use the writer's literal string API, including
formula-like text and URLs. Unicode text is preserved. Excel cells additionally
have a 32,767 UTF-16-unit limit; oversized cells fail before output.

PDF uses Letter landscape with a repeated ten-column compact table, visible
ellipses for long main-table values, page numbers, and full assignment details in
stable order. Details include every table value plus coverage identity/label/day
type, original position/shift names and labels, shift category/color and employee
snapshots. Notes may continue across pages. Internal detail keys are canonical
(date, coverage-item snapped seed/ID, one-based slot) tuples in inert page metadata.

The font is embedded from `src-tauri/assets/fonts/NotoSans-Regular.ttf`, downloaded
from [the Noto project](https://github.com/notofonts/noto-fonts/blob/main/hinted/ttf/NotoSans/NotoSans-Regular.ttf),
SHA-256 `b85c38ecea8a7cfb39c24e395a4007474fa5a4fc864f6ee33309eb4948d232d5`.
Its SIL Open Font License is in `src-tauri/assets/fonts/OFL.txt`, also bundled as
an application resource. PDF supports NFC Latin letters in U+00A0–024F and
U+1E00–1EFF, ASCII text, and the selected Common punctuation/symbols explicitly
listed in `export_pdf.rs`, subject to verified glyph coverage. NFC composition
provides canonically equivalent rendering without altering XLSX/DTO text.
Residual combining marks, other scripts, bidi controls, variation selectors and
unsupported glyphs fail with `pdf_text_not_renderable`, sanitized code-point/script
counts and an XLSX alternative when within its limits. V1 claims no complex-script
shaping or bidirectional PDF support. XLSX retains such text literally.

Limits are 100,000 assignment rows for XLSX, 10,000 for PDF/print, and 64 MiB
(67,108,864 bytes) of aggregate normalized DTO text. Checked loading preflights
legacy field byte lengths before fetching values and counts visible control
replacements and duplicated display fields before retaining rows. Unsupported
legacy controls become visible `[U+XXXX]` text with bounded diagnostics. Unknown
assigned employees remain assigned; only true open rows display `Open Shift`.

Dependencies: [rust_xlsxwriter 0.100.0](https://docs.rs/rust_xlsxwriter/0.100.0/)
with only `constant_memory`; [calamine 0.37.0](https://docs.rs/calamine/0.37.0/)
without optional features; [pdf-writer 0.15.0](https://docs.rs/pdf-writer/0.15.0/);
[lopdf 0.45.0](https://docs.rs/lopdf/0.45.0/) without optional features;
[skrifa 0.48.0](https://docs.rs/skrifa/0.48.0/) with only `std`; and
[unicode-script 0.5.8](https://docs.rs/unicode-script/0.5.8/). They use MIT and/or
Apache-2.0 licenses and local Rust implementations with no added native renderer,
office, network or system-font dependency. Skrifa uses the maintained Fontations
parser and caches the bundled font's verified glyph IDs and unscaled advances;
its optional autohint shaping feature is disabled. Published MSRVs are at most
Rust 1.88 (1.85 for skrifa); pdf-writer/unicode-script do not declare an MSRV, so
compatibility is established through the application build rather than a published
MSRV claim. The locked Windows release configuration passed `cargo check` with
Rust 1.94; the desktop debug configuration was built and exercised on Windows.
The supported application combinations are normal desktop debug and
release with Tauri's `custom-protocol`, using these same export feature selections;
`--all-features` is not a supported dependency qualification combination.

Both writers finish/close before exact-file synchronization. Calamine independently
checks sheet metadata, headers, every cell type/value/order and absence of formulas;
lopdf independently checks page count, nonempty content streams, exact rendered
text/order/page markers and detail identities. All readers close before the shared
safe-destination service rechecks observed destination/parent/protected-file state
and publishes. Temporary cleanup warnings are separate from the primary outcome.
Exports reuse the backup service's one-time overwrite tokens and native publication
guarantees, with no backup-history insertion. Ordinary local documents are not
encrypted; their confidentiality depends on folder and Windows account permissions.

Print… opens a print preview dialog with immutable captured metadata, compact rows
and full details. Print CSS uses Letter landscape margins and repeated headings
where WebView2 supports them. Only the explicit Print… action inside the preview
opens the OS dialog; printer-driver layout and silent printing are outside V1.
Printing makes no application-owned hidden file.

The Windows executable also exposes the same export operation locally:

```sh
shift-happens.exe --export-schedule 'C:\path\development\shift-happens.sqlite' SCHEDULE_ID xlsx 'C:\Exports\schedule.xlsx'
```

Use `pdf` and a `.pdf` destination for PDF. The CLI requires an existing current
schema, never initializes/migrates it, and prints only file metadata/diagnostics.
An existing destination requires typing `REPLACE`; other input cancels.
Repository policy forbids automated tests and acceptance harnesses. Builds, static
checks and direct application/CLI inspection supplement manual acceptance.

## Windows installer and local release

The primary bundle is NSIS only, explicitly `currentUser`, with the stock default
`%LOCALAPPDATA%/Shift Happens` executable directory. Its directory-selection page
accepts another writable folder and install-over retains the prior selected path.
Start Menu and desktop shortcuts use the project icon. The database and app-owned
migration backups stay under the identifier's app-local directory regardless of
the selected executable folder. No updater, MSI, service or publishing path is added.
The ordinary internal installer uses silent WebView2 `downloadBootstrapper`; a
machine without WebView2 needs network access to install the runtime. There is
no offline installer variant, and NSIS does not provide MSI repair semantics.
Configuration follows the official
[Windows installer guide](https://v2.tauri.app/distribute/windows-installer/).

Windows inspection reproduced the stock uninstaller's **Delete the application
data** checkbox. This pinned Tauri version has no supported switch that removes
it. `src-tauri/installer/installer.nsi` therefore retains the exact stock template
from Tauri CLI 2.12.2 (bundler 2.10.2, commit
`5b23c82ea5ba6b9aec420dd71f60493edc486aa9`), removing only the deletion UI,
callbacks and deletion-only block. Adjacent MIT/Apache notices retain the upstream
licenses. Review/rebase that narrow patch when upgrading the CLI. The effective
generated script is checked for app-data deletion before candidate completion.

The calendar/checkmark artwork in `public/icon.svg` and `src-tauri/icons` is owned
by this project, confirmed by the manager. The configured PNG is 512×512; the ICO
contains 16, 24, 32, 48, 64 and 256 pixel representations. The same ICO is used for
installer and uninstaller. The existing PDF font's SIL Open Font License is bundled.
Missing or invalid configured resources fail the release script.

Use a clean Git checkout with native Windows Git, Node/npm, Cargo/MSVC, SDK,
WebView2, `cargo-deny` 0.20.2 and the full [7-Zip CLI](https://www.7-zip.org/) available.
The script discovers `7z.exe` on PATH or in its standard Program Files directory;
the reduced `7za`/`7zr` builds do not support NSIS extraction.
Use a local Windows drive directory: native `npm.cmd` cannot build from a UNC/WSL
working directory. Keep isolated portable build tools outside the source tree.
Close every running application instance.

```powershell
npm run release:windows
```

When genuine Git metadata is unavailable, capture an external source inventory,
review the exact source inputs, then select it explicitly for the candidate:

```powershell
npm run release:windows -- -CaptureSourceManifest C:\release-inputs\shift-happens-source.json
npm run release:windows -- -SourceManifest C:\release-inputs\shift-happens-source.json
```

Capture only records paths and SHA-256 values; it does not build or qualify an
application. The candidate checks every source path and hash before and after its
gates, rejects added/missing/changed files and links, and records the inventory's
identity with the artifacts. Only known generated directories are excluded.
This mode identifies reviewed source bytes without claiming Git history. The
default mode still requires a genuine clean committed checkout. The same inventory
can verify a local Windows copy of a reviewed WSL workspace.

`scripts/release-windows.ps1` preserves native failure exit codes and requires
locked dependencies. It runs existing formatting, strict types, lint and build
commands, Rust formatting, debug/release Clippy and the supported builds. It
builds the production frontend before release Clippy so Tauri's embedded assets
exist even in a clean source checkout; the installer retains its canonical
`beforeBuildCommand` as well. It
reviews SQLite/export dependency trees, denies audit warnings, bundles NSIS,
compares the existing dual-binding runtime diagnostic and compiled migration
SHA-384 inventory against source, checks effective uninstaller/version/resources,
and writes `SHA256SUMS.txt` beside the release executable. Tauri stamps/signs the
NSIS payload and restores the original build executable afterward, as shown in
the [pinned bundler source](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.12.2/crates/tauri-bundler/src/bundle.rs).
The script extracts the actual shipped executable to `release/payload` for its
runtime/signature checks and records hashes for the build executable, extracted
payload and installer. Extraction does not execute an installer or change app data.
It deliberately does
not create/run tests or fixtures: the explicit `AGENTS.md` policy takes precedence
over Prompt 15's automated acceptance requests. Direct installed qualification
remains required; script success is only candidate build success.

Unsigned personal/internal candidates are supported and labeled **NotSigned**.
When genuine credentials exist, an external Tauri JSON overlay can be passed via
`npm run release:windows -- -SigningConfig C:\secure\signing.json`. Keep publisher,
certificate/thumbprint or custom signing command, digest and timestamp settings
in that external configuration and credentials in the provider's environment or
Windows certificate store. No credentials or sample secrets are committed.
The overlay permits only `bundle.publisher` and `bundle.windows` fields
`signCommand`, `certificateThumbprint`, `digestAlgorithm`, `timestampUrl` and `tsp`;
it cannot override startup, installer, CSP or capability configuration.
Both artifacts must then have valid Authenticode signatures and timestamp
certificates. Follow the official
[Windows signing contract](https://v2.tauri.app/distribute/sign/windows/);
signing does not guarantee immediate SmartScreen reputation.

The candidate paths for the retained version are:

- `src-tauri/target/x86_64-pc-windows-msvc/release/shift-happens.exe`
- `src-tauri/target/x86_64-pc-windows-msvc/release/payload/shift-happens.exe`
- `src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/Shift Happens_0.1.0_x64-setup.exe`
- `src-tauri/target/x86_64-pc-windows-msvc/release/SHA256SUMS.txt`
- `src-tauri/target/x86_64-pc-windows-msvc/release/SOURCE-IDENTITY.json`
- `src-tauri/target/x86_64-pc-windows-msvc/release/SOURCE-INVENTORY.json` (explicit inventory mode)

The strict release advisory gate audits the supported Windows release graph,
including build dependencies, through `src-tauri/deny.toml` and
[cargo-deny's target filtering](https://embarkstudios.github.io/cargo-deny/checks/cfg.html).
It fetches current RustSec data and denies vulnerabilities, unmaintained/unsound
dependencies, yanked packages, and all warnings, with no ignored advisories or
excluded packages. The independently reconciled graph includes every Windows
normal/build dependency; it is not a scan of application dependencies alone.
The supported Windows graph passes this gate. A separate whole-lockfile scan
still reports Unix-only `proc-macro-error` 1.0.4
([unmaintained](https://rustsec.org/advisories/RUSTSEC-2024-0370.html)),
and
`glib` 0.18.5
([unsound iterator](https://rustsec.org/advisories/RUSTSEC-2024-0429.html)).
Both are outside the Windows compiled/build dependency graph. The direct PDF dependency
`ttf-parser`, flagged as
[unmaintained](https://rustsec.org/advisories/RUSTSEC-2026-0192.html), has been
replaced with the maintained Fontations parser. No findings are suppressed.
The Unix-only entries remain intact in Cargo.lock. Tauri's stable GTK dependencies
have not adopted the upstream replacement versions, so replacing them with
unreviewed forks would not improve this Windows artifact's security. This policy
does not qualify a Linux build; this application intentionally supports Windows only.

Direct verification on Windows 10.0.26200 used an unsigned x64 release with the
exact `tauri/custom-protocol` feature set. Default and alternate writable installs,
install-over retaining the alternate location, second-launch restore/focus
(including a launch with CLI arguments), new/current/pending startup, a preceding
schema-7 upgrade, schedule/current-setting preservation, finalization, XLSX/PDF
export and manual backup ran successfully. Independent SQLite inspection found
the validated pre-migration copy still at schema 7 with the finalized schedule;
the live database reached schema 8 and recorded the backup afterward. A boundary
check initially rejected SQLite's read-only creation of an empty WAL; that narrow
housekeeping case was corrected and the installed upgrade repeated successfully.

A physically valid foreign-key-blocked database with `backup_runs` absent saved
a `manual_preserve` copy through the actual blocked shell/native dialog, retaining
the same violation and remaining blocked without managed domain state. Native
dialog cancellation, explicit application overwrite confirmation/success and
failure after source file-set drift also ran. The manually changed disposable
row was restored afterward. An unwritable migration-backup folder returned
`file_write_failed` at `pre_migration_backup`, left schema 7 and the finalized
schedule/current setting intact, and created no history table. Restoring the
original inherited folder permissions allowed the validated backup and upgrade
to complete on the next launch. The patched uninstaller UI had no deletion control;
interactive, silent and passive uninstalls preserved the database and all three
app-owned migration backups. Full SHA-256 values were unchanged across the
interactive/passive removals. Reinstall retained the finalized plan/current setting.
Executable/installer version 0.1.0, both **NotSigned**, and both shortcuts were
inspected on Windows. The dual-binding diagnostic reported SQLite 3.51.3 with the
WAL-reset-fixed source ID and all eight exact source migration checksums.
Preservation retains the journal mode from read-only inspection, accepting empty
WAL/shared-memory housekeeping only for an inspected WAL database. SQLite
corruption result codes during health checks classify as integrity failures after
recognition. An unrecognized file, permission, locking or recovery failure retains
its access classification; unreadable health results stop further metadata reads.
An unrecognized file was directly opened through the rebuilt installed app:
startup reported `database_access_error`, left its contents unchanged and created
no sidecars. The original disposable database file set was restored with matching
hashes; current startup reopened the finalized plan and selected schedule normally.
Windows 7-Zip extraction produced an NSIS payload with the same SHA-256 as the
installed executable. Its only difference from the original build executable was
Tauri's three-byte `UNK` → `NSS` marker. Runtime/signature gates therefore inspect
the payload; the original build output remains separately identified in the
checksum file. The extracted executable and installer both reported **NotSigned**.

Follow-up direct Windows inspection covered dirty/checksum/unknown migration
precedence, each sole checksum/unknown failure, missing migration metadata, and
post-schema initializer rollback. An inserted missing setting remained absent
after a later retained-row invariant failed; the original source/settings rows
were restored before healthy startup. A genuine interrupted SQLite transaction
left a hot rollback journal. Read-only startup reported `database_access_error`,
did not offer unsafe preservation, and left main/journal SHA-256 values unchanged.
Native SQLite subsequently recovered that developer-owned transaction normally.
The preservation command returned cancellation from the trusted local shell;
the same real WebView navigated to a remote origin retained IPC internals but
explicitly denied the command through the local main capability. All 94 registered
commands, build-manifest entries, generated permissions, capability grants and
frontend call paths were reconciled without missing or duplicate entries.

Further direct checks used DELETE-mode CLI preservation, stale/reused overwrite
confirmations, a live database hard link, an active destination's WAL/shared-memory
sidecars, denied temporary-file cleanup, and concurrent PDF publication. Failures
preserved the original destination or reported the retained owned temporary path;
retrying after restoring permissions succeeded. Concurrent publication produced
one complete document and one destination-state rejection, without leftover
temporary files. The original development rows and migration metadata were
restored exactly, and healthy startup reopened the current finalized plan. The
actual schedule print button opened WebView's print UI and Microsoft Print to PDF
produced three sheets. Independently rendered pages contained all five assignments,
their details, next-day night-shift times, and the literal HTML-like employee name.
No application navigation or workflow controls appeared in those printed pages.

Another direct manager walkthrough covered changed PTO facts and stale warning
confirmations, three undo/redo steps across two assignments, error-blocked and
warning-backed finalization, clean finalization/reopening, reduced motion, current
selection/archive behavior, all themes, help keyboard focus and actual cheat-sheet
print cancellation. A 31-day month with exactly 15 active employees generated and
committed 187 assigned slots in 168 ms in the Windows debug build after readiness
inspection; independent validation found zero errors. This is one observed run on
an i7-11370H/NVMe system, not an optimized-release benchmark. A normal 370-day
generation produced 2,008 slots, and its PDF independently parsed as 1,051
landscape Letter pages containing all 2,008 detail entries. Retargeting a parent
junction during that PDF operation rejected publication without leftover files.

A real bounded filesystem ran out of space during PDF writing: new publication
failed cleanly, confirmed replacement preserved the original bytes, and freeing
space allowed retry. A real online SQLite copy to the Windows-to-WSL filesystem
hit the 60-second limit; its guidance now includes choosing another destination
folder. Moving its parent exposed an unreported temporary-file leak, corrected by
retaining the verified Windows directory handle. The rebuilt app cleaned the
moved temporary file on native NTFS while preserving the original destination;
short-held source contention then allowed a successful confirmed retry. The WSL
filesystem could not resolve a Linux-side directory rename through that handle
and returned an explicit cleanup warning. Its copy never wrote pages, so that
operation is evidence of a real copy timeout, not SQLite's native disk-full result.
Original development database/WAL/SHM snapshots were restored with exact hashes.

Frontend format/types/lint/build, Rust formatting and Windows debug/release Clippy
passed; the updated bundled debug executable built and npm audit reported no
vulnerabilities. SDK debug-PDB linker warnings occurred during WSL cross-linking.
The release script parsed in PowerShell 5.1 and a real source-inventory mismatch
stopped it with exit 1 before building. At that time, the workspace had no Git
history; inventory mode records that distinction explicitly. Build-gate success
still requires direct installed qualification; the observations above do not certify every filesystem
race, temporary-name collision or capacity boundary. Ordinary first installation
requires network access when WebView2 is absent. Genuine signing/timestamp
credentials were unavailable, so the internal candidate remains unsigned.
No automated tests, fixtures or harnesses were created or run.
