# AnyLeap Two-PC E2E Automation Tooling

Windows PowerShell 5.1 scripts for remote NSIS installation and KVM return-cursor soak testing on a two-PC setup (Main PC = controller with low-level hooks, Linked PC = receiver).

Because SSH sessions land in non-interactive Session 0, desktop-facing steps run in the logged-on user's interactive session via Windows Task Scheduler (`InteractiveToken`, equivalent to `/IT`).

## Scripts

| File | Purpose |
| --- | --- |
| `common.ps1` | Shared Win32 P/Invoke layer (`PerMonitorV2` DPI awareness, `GetLastInputInfo`, `SendInput` relative mouse moves, `GetCursorPos`, `GetCursorInfo`, `GetClipCursor`, `EnumDisplayMonitors`), the shared 120 s idle guard, and user-input tick tracking. |
| `register-tasks.ps1` | One-time interactive setup that registers `AnyLeapTestInstall` and `AnyLeapKvmSoak` under the current user with `InteractiveToken` (`/IT`). Uses `LeastPrivilege` unless the registering shell is already elevated (`HighestAvailable`). |
| `unregister-tasks.ps1` | Removes the `AnyLeapTestInstall` and `AnyLeapKvmSoak` scheduled tasks. |
| `run-pending.ps1` | Task Scheduler entry point that reads arguments for the requested task from `scripts/e2e/pending.json` and invokes the target script. |
| `install-anyleap.ps1` | Waits for idle guard, stops `anyleap.exe`, stray `adb.exe`/`scrcpy.exe` from the install directory, and any `WerFault.exe -p <pid>` targeting them, runs the NSIS installer with `/S`, verifies the installed `anyleap.exe` timestamp/version/hash changed, optionally relaunches AnyLeap (`-Launch`), and writes `scripts/e2e/install-result.json`. |
| `kvm-soak.ps1` | Runs `-Cycles` (default `300`) leap-and-return cycles across `-SeamEdge` (default `left`), varying speed and cadence (`slow`, `fast_flick`, `immediate_releap` within 100–200 ms), samples `GetCursorPos` at `+0 ms`, `+50 ms`, and `+250 ms` after `RETURN`, classifies each cycle (`edge_ok`, `center`, `other`), and writes `scripts/e2e/kvm-soak-result.json`. |

## Tagged Test Input, Flag File, Early Abort, Progress Log

- **Why:** AnyLeap's low-level mouse hook drops injected (`SendInput`) mouse events while not trapped, so an untagged soak never engages a leap.
- **Tag:** `kvm-soak.ps1` sets `dwExtraInfo = 0x414E594532454531` on every move (`ANYLEAP_E2E_INPUT_TAG` in `src-tauri/src/kvm/hooks.rs`, `$script:AnyLeapE2EInputTag` / `E2E_INPUT_TAG` in `common.ps1`; keep all three in sync).
- **Flag file:** AnyLeap accepts tagged injected moves as physical only while `%LOCALAPPDATA%id.co.dak.anyleape2e-accept-injected` exists. The soak creates it at start and deletes it in a `finally` block (also on abort or Ctrl+C). AnyLeap re-checks it every ~250 ms on its hook thread. Requires an AnyLeap build that contains this hook. Without the file, behavior is unchanged. See SPEC-23 §7.
- **Fail fast:** the soak exits with `"status": "anyleap_not_running"` (exit code `4`) if `anyleap.exe` is not running.
- **Early abort:** if the first 5 cycles produce no engage (no ENGAGE log event, cursor confinement, or relocation within `-EngageTimeoutMs`), the soak stops with `"status": "no_engage"` (exit code `3`) instead of running all cycles.
- **Progress log:** one line per cycle (`<utc> cycle=<n>/<total> scenario=<name> outcome=<class>`) is written to `kvm-soak-progress.log` next to the result JSON; tail it to monitor a run.

## Safety Rules

1. **Idle Guard (120 s)**: Both `install-anyleap.ps1` and `kvm-soak.ps1` check `GetLastInputInfo` before doing anything. If user input occurred within the last 120 seconds, they poll every 15 seconds up to `-MaxIdleWaitMinutes` (default `10`). If the user stays active, they abort cleanly with `"status": "user_active"` and exit code `2`.
2. **Relative Cursor Moves Only**: `kvm-soak.ps1` injects `SendInput` `MOUSEEVENTF_MOVE` relative deltas only — never mouse clicks, never absolute jumps, and never keyboard events.
3. **Immediate Abort on Real User Input**: `kvm-soak.ps1` records the `GetTickCount()` window around every `SendInput` call it injects. If `GetLastInputInfo` changes outside those script-owned tick windows, the script aborts immediately with `"status": "aborted_user_input"` and exit code `3`.
4. **Redacted Log Excerpts**: `kvm-soak.ps1` reads `%LOCALAPPDATA%\id.co.dak.anyleap\logs\kvm-debug.log` (when KVM debug logging is enabled in AnyLeap) and strips window titles, process names, and `hwnd` values from output excerpts, keeping only timestamps and event fields.

## Coordinator Workflow Over SSH

1. **One-time setup (run once in the interactive desktop session on the Main PC):**
   ```powershell
   powershell -NoProfile -ExecutionPolicy Bypass -File scripts/e2e/register-tasks.ps1
   ```
2. **Optional pre-step for soak tests:** Ensure AnyLeap is running on both PCs, linked with the seam on the Main PC's left edge (default), and enable KVM debug logging in AnyLeap so `kvm-debug.log` records `ENGAGE phase=perform` and `RETURN`.
3. **Write `scripts/e2e/pending.json` over SSH:**
   ```json
   {
     "AnyLeapTestInstall": {
       "Installer": "C:\\Users\\<user>\\Downloads\\AnyLeap_1.1.1_x64-setup.exe",
       "Launch": true
     },
     "AnyLeapKvmSoak": {
       "Cycles": 300,
       "SeamEdge": "left"
     }
   }
   ```
4. **Trigger a task over SSH:**
   ```cmd
   schtasks /Run /TN AnyLeapTestInstall
   ```
   or:
   ```cmd
   schtasks /Run /TN AnyLeapKvmSoak
   ```
5. **Poll status and inspect JSON results:**
   ```cmd
   schtasks /Query /TN AnyLeapKvmSoak /FO LIST
   ```
   Read `scripts/e2e/install-result.json` or `scripts/e2e/kvm-soak-result.json`.
6. **Cleanup when done:**
   ```powershell
   powershell -NoProfile -ExecutionPolicy Bypass -File scripts/e2e/unregister-tasks.ps1
   ```

## Self-Checks (`-DryRun`)

Every script supports `-DryRun` (Pester-free, no desktop or Task Scheduler side effects) to validate argument parsing, idle-guard state transitions, process filtering, Task Scheduler XML generation, log sanitization, and cursor classification math:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/e2e/common.ps1 -DryRun
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/e2e/install-anyleap.ps1 -DryRun
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/e2e/register-tasks.ps1 -DryRun
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/e2e/unregister-tasks.ps1 -DryRun
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/e2e/run-pending.ps1 -DryRun
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/e2e/kvm-soak.ps1 -DryRun
```
