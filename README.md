# usage-report

**Linux user quotas and disk usage, in one terminal.**

See who is using your root filesystem, check quota limits, and explore a user's
files in an ncdu-style browser—even when they are scattered outside their home
directory. A background service builds a shared SQLite index so that opening the
report does not start another filesystem scan.

[Quick start](#quick-start) · [Usage](#usage) · [Scan settings](#scan-settings) · [Validation notes](VALIDATION.md) · **English** / [简体中文](README.zh-CN.md)

## Is usage-report for you?

Built for shared Linux servers, usage-report helps administrators understand
storage allocation and users understand their own usage. Its primary deployment
target is a local ext4 root filesystem with user quotas enabled and systemd.

- **Quota overview.** Used space, soft and hard limits, remaining allowance,
  utilization bars, and color-coded warnings, sorted by usage.
- **Per-UID usage tree.** Browse a user's allocated space across the root
  filesystem, including paths under directories owned by other users.
- **Cached browsing.** One scan indexes all UIDs. Clients use the published index
  while the next generation is being built.
- **Visible progress.** Entries, elapsed time, average speed, errors, and skipped
  paths, without starting a new scan.
- **Administrator controls.** Adjust the scan schedule from the command line or TUI.
- **English and Chinese.** English by default; press `l` to switch instantly.

The file browser is read-only: it does not delete files or change quotas.
Administrators can explicitly request scans and change scheduling settings.

## Quick start

You need Linux, a Rust toolchain supporting edition 2024, a C compiler for bundled
SQLite, and systemd for system installation. Enable user quotas separately; the
installer does not configure quotas or edit `/etc/fstab`.

```bash
git clone https://github.com/leeechsh/usage-report-rs.git
cd usage-report-rs
bash scripts/build-dist.sh
sudo bash install-system.sh
usage-report
```

Repository access is required to clone. The build script creates
`dist/usage-report` and a SHA256 manifest, which the installer verifies.

Installation starts the query service and an initial background scan of `/`.
The first tree becomes available when that scan finishes; check it with
`usage-report --progress`. Normal queries do not need `sudo`.

To try the browser without installing system services, run this after building:

```bash
bash demo.sh
```

The demo indexes your home directory without root privileges, reuses its local
cache on subsequent launches, and stops its query service on exit. It is not a
full-root scan and cannot modify the system scan schedule.

## Usage

```bash
usage-report                        # Interactive quota overview
usage-report alice                  # Usage tree for a username
usage-report 1004                   # Usage tree for a numeric UID
usage-report --plain                # Plain-text quota report
usage-report alice --json           # First page of tree data as JSON
usage-report --status               # Quota update, ready index, active scan
usage-report --progress             # Interactive scan progress
usage-report --progress --plain     # One progress snapshot
usage-report --watch                # Append progress every second
usage-report --watch --json         # One JSON status record per second
usage-report --refresh              # Admin: request an asynchronous scan
```

Non-terminal output defaults to plain text. Refresh requests do not wait for
completion or launch a second concurrent scan.

### Keyboard controls

| Key | Action |
| --- | --- |
| `↑` / `↓`, `j` / `k` | Select a row |
| `Enter` / `→` | Open a user's tree or enter a directory |
| `←` / `Backspace` | Go to the parent directory |
| `PgUp` / `PgDn` | Change tree query pages (200 entries per page) |
| `r` | Load the latest published index; does not start a scan |
| `p` | Open progress; press again to return |
| `c` | Open scan settings |
| `l` | Switch between English and Chinese |
| `Esc` | Return to the overview or close a dialog |
| `q` / `Ctrl-C` | Quit; `q` closes an open settings dialog first |

The interface adapts to terminal width, with compact status information,
right-aligned sizes, usage bars, alternating rows, and a highlighted selection.

### Language

```bash
usage-report --lang zh              # Start in Chinese
usage-report alice --lang en        # Start in English
usage-report --plain --lang zh      # Chinese text report
usage-report --lang zh --help       # Chinese command help
```

Each launch defaults to English, independently of `LANG`. Pressing `l` preserves
your selection, current directory, and unsaved settings input. Language is not
persisted. Usernames, filenames, paths, and JSON fields and values are unchanged;
unrecognized low-level diagnostics retain their original text.

## Scan settings

Kernel quotas refresh every **30 seconds** by default. The full filesystem index
uses a separate systemd timer with these distributed defaults:

| Setting | Default |
| --- | --- |
| Delay after boot | 5 minutes |
| Interval after the previous scan finishes | 3 hours |
| Randomized delay | Up to 1 minute |
| Timer accuracy window | 1 minute |

These are scheduling windows, not exact execution times. Inspect the effective
settings and next trigger on your machine:

```bash
usage-report --config
usage-report --config --json
```

Root and configured administrators can change settings without a separate
`sudo` prompt for each request:

```bash
usage-report --scan-interval 1h
usage-report --boot-delay 10m --scan-jitter 30s
usage-report --auto-scan off
usage-report --auto-scan on
```

In the settings dialog (`c`), use `↑` / `↓` / `Tab` to select fields,
`Backspace` to edit durations, `←` / `→` / `Space` to toggle, `Enter` to save,
and `Esc` to cancel. Ordinary users can view settings but cannot save changes.

Durations accept `30m`, `3h`, `1d`, and `1h30m`. The interval must be
1 minute–30 days, boot delay 0–1 day, and jitter at most 1 hour and no longer
than the interval. Settings are written atomically to
`/etc/systemd/system/usage-report-scan.timer.d/50-usage-report.conf`, applied,
and checked against systemd. Failures trigger an attempt to restore old settings.

Saving unchanged settings leaves the timer alone. Changes recalculate the next
trigger; an elapsed boot delay can cause an immediate scan. Disabling automatic
scans does not stop an active scan or disable manual `--refresh`.

## How the index works

A root scan traverses the filesystem once for all UIDs, reading metadata rather
than file contents. Known mountpoints are pruned before inspecting their contents,
including NFS, separate disks, virtual filesystems, and same-device bind mounts.
Symlinks are not followed.

Allocated space uses `st_blocks × 512`; apparent size is stored separately.
Directory blocks belonging to the selected UID count toward that user's usage.
Hard links are deduplicated by device and inode: the first encountered path gets
the allocation, and other paths appear as link references. That first path can
change between scans. Ancestors owned by other UIDs remain as navigation paths
without adding their own space to the selected user's totals.

A new index is published only after completion; the last two completed
generations are retained. Interrupted scans do not replace the ready index.
Browsing stays on one generation until you press `r`.

Progress is committed every 2,000 inserted entries or approximately one second;
blocked I/O can delay updates. The total entry count is unknown, so the tool does
not display a completion percentage or ETA. Only administrators see the active
scan's current path; ordinary users see aggregate progress.

**An index is a live traversal, not a filesystem snapshot.** Deleted-but-open
files, files hidden beneath mounts, concurrent changes, and scan errors can make
cached totals differ from live kernel quotas. Quota and index timestamps are
shown separately.

## Permissions and storage

| Component | Default location |
| --- | --- |
| Client | `/usr/local/bin/usage-report` |
| Service binary | `/usr/local/libexec/usage-report` |
| SQLite index | `/var/lib/usage-report/index.db` |
| Unix socket | `/run/usage-report/report.sock` |
| Administrator configuration | `/etc/default/usage-report` |

The SQLite WAL database is private to the service and must reside on a local
filesystem; NFS database storage is rejected. Clients use a Unix socket, with
peer UIDs authenticated through `SO_PEERCRED`. Ordinary users can browse only
their own UID's tree. Root and configured administrator UIDs can browse any UID.
The quota overview is visible to all users.

On first installation, the installer sets the invoking `sudo` user's UID as an
administrator. Existing configuration is preserved. To add administrators, use
repeated `--admin-uid` arguments in `USAGE_REPORT_ADMIN_ARGS` in
`/etc/default/usage-report`, then restart `usage-report.service`.
No setuid binary, sudoers rule, or remote network API is installed.

## Upgrading and troubleshooting

```bash
bash scripts/build-dist.sh
sudo bash install-system.sh --upgrade
usage-report --status
```

Wait for an active scan to finish before upgrading; the installer refuses an
upgrade during a scan before changing files. It preserves the database,
administrator configuration, timer overrides, and timer enabled/active state.
It restarts the query service without deliberately starting a full reindex.

Existing binaries are backed up under `/var/backups/usage-report/<timestamp>/`.
An existing entry command is preserved as `usage-report-legacy`; the original
`/usr/local/sbin/quota_report` is not overwritten. Restoring an old binary also
requires checking database schema compatibility; see the
[validation and upgrade notes](VALIDATION.md) (Chinese).

```bash
systemctl status usage-report.service usage-report-scan.service usage-report-scan.timer
journalctl -u usage-report-scan.service -n 50 --no-pager
```

Large filesystems take time to scan and require database space proportional to
metadata volume. The distributed scan service uses low CPU/I/O priority and a
2 GiB memory limit. Measure resource requirements on your workload. For database
backups, use SQLite's backup API or stop writers and checkpoint first; copying
only the main database while WAL writes are active is insufficient.

## Development

```bash
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
cargo fmt --check
```

| Path | Purpose |
| --- | --- |
| `src/scan.rs`, `src/db.rs`, `src/model.rs` | Traversal, SQLite storage, data model |
| `src/quota.rs`, `src/service.rs` | Kernel quota queries and authenticated service |
| `src/tui.rs`, `src/ui.rs` | Interaction and terminal rendering |
| `src/i18n.rs`, `src/text.rs` | Localization and text reports |
| `src/schedule.rs`, `systemd/` | Scan scheduling and service units |
| `tests/`, `examples/` | Integration tests and UI preview generator |
| `scripts/build-dist.sh`, `install-system.sh` | Build and system installation |
| [VALIDATION.md](VALIDATION.md) | Version-specific validation and operational notes (Chinese) |

Build output (`target/`, `dist/`), local previews (`var/`), and database files are
excluded from Git. Run `cargo clean` to remove compilation artifacts; the local
`dist/` binary is retained.

## License

[MIT](LICENSE) · Created by [leeechsh](https://github.com/leeechsh).
