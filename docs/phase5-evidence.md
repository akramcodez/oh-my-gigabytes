# Phase 5 development evidence

Status: **Linux development milestone implemented and verified; full Phase 5 acceptance is not claimed.** The service is scan-only, optional and never installed implicitly. Cleanup remains disabled (Phase 3 gates are unchanged).

## Commands

| Command | Behavior |
| --- | --- |
| `omg start` / `omg service start [--yes]` | If no registration exists, prints exactly what will be written and asks `y/N` (non-interactive use requires `--yes`). Starts the unit, then waits up to 10 s for the service to answer on its socket and **fails** if it does not. |
| `omg service install [--enable] [--yes]` | Writes `~/.config/systemd/user/omg.service` (atomically) and `daemon-reload`. `--enable` additionally starts it at login. Never touches system units, root or lingering. |
| `omg stop` / `omg service stop` | `systemctl --user stop` when registered, otherwise the IPC `stop` operation for a manually supervised `omg service run`. |
| `omg service uninstall` | Stops, disables and removes only a unit carrying omg's marker line. The index, history and ignores are kept. |
| `omg service status`, `omg status` | Registration facts, kernel-lock + heartbeat health (`running`, `degraded`, `unhealthy`, `stopped`) and, when reachable, live service state. |
| `omg service scan` | Asks the running service for a scan at its low priority. |
| `omg service run` | The foreground process the unit executes. Refuses to run as root. |
| `omg trends [--days N]` | Observed free-space and indexed-size change plus the largest-growing directories. |
| `omg doctor` | Read-only registration facts, including a registration that differs from the current install and an orphaned registration whose binary is gone. |

## Configuration

`$XDG_CONFIG_HOME/omg/config.toml` (or `--config PATH`). Strict TOML: unknown keys are rejected, the file is size-bounded (16 KiB), must be a regular file (no symlinks) and every value is range-checked. A missing file means defaults with **notifications off**. There is no key that can name a command, a path, an action or an authorization.

```toml
version = 1
[schedule]       # interval 15..=10080 min, default 360; reconcile 1..=8760 h, default 168
interval_minutes = 360
reconcile_hours = 168
jitter_percent = 10        # forward-only: never earlier than the interval
pause_on_battery = true    # scheduled scans only; an explicit request still runs
backoff_base_minutes = 15
backoff_max_minutes = 1440
[resources]
workers = 1                # 1..=2
nice = 10                  # 0..=19, never raises priority
volume_check_minutes = 5   # cheap statvfs sample for notifications, not a scan
[notifications]
enabled = false
desktop = false            # fixed /usr/bin/notify-send call if present
low_free_bytes = 0
recovery_free_bytes = 0    # must exceed low_free_bytes
dwell_minutes = 10
cooldown_hours = 24
[retention]
sample_days = 365
max_samples = 4000
```

## Design as implemented

- **Scan-only by construction.** The IPC protocol is a closed enum of four operations (`status`, `scan`, `yield`, `stop`); there is no way to express cleanup, plan approval or catalog changes. Messages are ≤512 bytes, versioned, unknown fields are rejected. A structural test also fails if the service sources reference deletion, rename, planning, Trash or authorization.
- **Authenticated IPC.** Unix socket in the private (0700) state directory, mode 0600, peer credentials checked against the effective uid on both sides. The socket is addressed through a no-follow directory descriptor (`/proc/self/fd/N`), so deep state paths do not hit the 108-byte socket limit and a symlinked state directory is refused.
- **Single owner, shared scan lock.** One service per profile via a kernel advisory lock (`service.lock`); health uses that lock plus the heartbeat, never a PID. The service and foreground scans use the same `scan.lock`. A foreground `omg scan` that finds the lock held asks the service to `yield`, waits (≤30 s) and then scans; the service's cancelled scan is not counted as a failure and scheduled work is held for 120 s.
- **Restart-gap / periodic recovery.** Every scan is a truthful full enumeration. A scan is labelled `full` when no complete scan exists, the last complete scan is older than twice the interval, or `reconcile_hours` has elapsed. Interrupted runs are recovered by the existing scan-ownership path.
- **Low priority and limits.** `setpriority` (nice) in-process; the unit adds `Nice=10`, `IOSchedulingClass=idle`, `CPUWeight=20`, `MemoryMax=512M`, `TasksMax=64`, `NoNewPrivileges`, `RestrictAddressFamilies=AF_UNIX`. Workers ≤2; backoff is exponential with forward-only jitter; failures ≥3 report `unhealthy`; a battery that reports `Discharging` pauses scheduled scans.
- **Notifications.** Pure hysteresis state machine (normal → pending → notified), persisted per volume. Dwell requires *continuously* low space, recovery needs `recovery_free_bytes`, cooldown bounds repeats, and a backwards clock restarts dwell/cooldown instead of firing early. Text is fixed wording with only a rounded byte figure and never a path or filename. Delivery is the service log plus, if enabled, a constant-argv `notify-send`.
- **Trends.** Schema 3→4 adds `volume_samples`, top-50 `growth_snapshots` (depth 1–2 directories, hard links counted once), `service_state` and `notification_state`. Samples are recorded after every complete/partial scan (foreground or service) and pruned by age and count. Directories absent from the earlier top-N list report an unknown prior size rather than an invented delta. Output states that attribution is uncertain.
- **Concurrency fixes found by testing.** Scan batches now take `BEGIN IMMEDIATE` (a deferred read-then-write batch failed with `database is locked` when the service wrote concurrently); `Index::open` waits briefly for the maintenance lock instead of failing a concurrent scan; scan cancellation no longer surfaces as a scanner fault; heartbeat writes are throttled during scans so IPC stays responsive.

## Verification (WSL2 Linux x86_64, Rust 1.95.0, Node 22.20.0, systemd user manager running)

- `cargo fmt --check`, `cargo clippy --workspace --all-targets --locked -D warnings` clean.
- **41 Rust tests pass** (was 16): 19 core service, 8 CLI (5 new end-to-end including a real `omg service run` child process), plus the earlier suites. Node package tests: 10 pass (a forged `service: enabled` capability is now rejected). Linux PTY smoke test passes.
- Repeated runs: 15 consecutive clean workspace runs, then 8 more concurrent runs with two CPU-burning processes. Two flaky races surfaced during this stress (cancel-as-failure, IPC timeout under SQLite contention) and were fixed before the final runs.
- **Forged input:** nine malformed or destructive-looking IPC messages (`cleanup`, `delete`, `approve`, `trash`, `plan`, extra fields, wrong version, oversize, non-JSON) never reach the handler; sentinel user data is untouched. Forged configs (`cleanup = true`, `[actions]`, `command = ...`) are rejected before any state is created.
- **Concurrency:** a service scan of 30 000 files hands off to a foreground scan; the resulting index passes `integrity_check`, has no `running` scans and the service reports 0 failures.
- **Backoff:** a scan failure sets `degraded` and schedules the retry 14–17 minutes out (15-minute base plus jitter).
- **Real systemd user manager** (this machine): approval refused without `--yes` and nothing written; `install`, `start` (verified responding), `status`, requested scan (trend sample recorded), `kill -9` reported truthfully as `stopped / ended without a clean stop` while systemd showed `activating`, `stop`, `uninstall` (unit removed, state kept). systemd reported `Nice=10`, `IOSchedulingClass=idle`, `MemoryMax=512M`; the process ran at nice 10 with idle I/O class. This run also found and fixed two real bugs the fake-`systemctl` tests could not: deep state paths exceeding the Unix socket limit, and `start` reporting success for a process that exited immediately.
- **Unsupported environments** (fake `systemctl` reporting `offline`, or no `systemctl`) report "unsupported" and write nothing.

### Measured resource use (release build, this machine only)

| Condition | Result |
| --- | --- |
| Idle service, 60 s | 0.08 % of one core, 7.9 MB RSS |
| Requested scan of 30 000 files, 1 worker, nice 10 | 2.8 s wall, 1.7 s CPU, 13.4 MB peak RSS |
| Debug build, idle 30 s (earlier loop version) | 0.37 % CPU — reduced after moving the sysfs battery read off the per-tick path |

These are single-machine WSL2 measurements, not the representative-machine budgets the exit gate asks for, and battery/I/O-contention behavior was not measured on real laptop hardware beyond the sysfs parsing tests.

## Hosted CI

The `Checks` workflow (fmt, clippy `-D warnings`, tests, PTY smoke, Node tests) passed on hosted ubuntu, macOS and Windows runners at commit 922ec40, after fixing non-Linux unused-import/dead-code errors that had failed the earlier Phase 4 push. This verifies that the non-Linux builds compile with native adapters disabled; it is **not** native macOS/Windows certification. The `artifacts.yml` native-artifact workflow has still not been run.

## Not delivered / unpassed gates

- **macOS LaunchAgent and Windows Task Scheduler** integrations: not implemented; those platforms report the service as unsupported. No native cross-platform service evidence exists.
- **Advisory filesystem watchers / dirty-scope invalidation:** not implemented. Freshness relies on periodic full enumeration (the documented truthful baseline).
- **Developer catalog expansion:** not delivered; it is defined as feedback-driven and no volunteered feedback exists.
- **Duplicate content analysis and Git-aware analysis:** deliberately not delivered; each remains separately gated and neither is needed for a reliable service.
- **Notification delivery** is limited to the service log and a fixed `notify-send`; behavior in real desktop sessions was not tested. Peer-credential rejection of a *different* uid is implemented but could not be exercised without a second account.
- **Resource budgets** on representative machines, IPC peer ACLs on other platforms, journald log-size policy, upgrade of a registered binary path (detected by `doctor`, repaired only by re-running `omg service install`) and independent usability review remain pending.
- Phase 1–4 acceptance blockers recorded in their evidence files still apply.
