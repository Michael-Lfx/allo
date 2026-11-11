# Windows: the dev loop cannot write outside the repo (Low integrity label)

A machine-level failure mode that makes `bun run dev` look broken while the code
is fine. It was hit on a Windows 11 workstation in November 2026 and is recorded
here so the next person spends minutes instead of a day on it.

## Symptoms

The dev loop fails at three different layers, all with "access denied" style
errors, and the desktop window flashes and closes if it gets that far:

| Layer | Message |
| --- | --- |
| C/C++ build | `LINK : fatal error LNK1104: cannot open file "...\Temp\lnk{GUID}.tmp"` or `cl : error D8050: cannot execute c1.dll` |
| Backend bootstrap | `thread 'nomifun-backend' panicked at .../bootstrap/tracing_init.rs: failed to create backend log file appender: ... Os { code: 5, kind: PermissionDenied }` |
| Tauri window | `ERROR tauri_runtime_wry: failed to create webview: WebView2 error: WindowsError(Error { code: HRESULT(0x8000FFFF) })` (fresh profile) or `HRESULT(0x800700AA)` (existing profile) |

## Root cause

The checkout directory carries a Windows **Low mandatory integrity label**:

```powershell
icacls "D:\path\to\repo" | Select-String Mandatory
# Mandatory Label\Low Mandatory Level:(OI)(CI)(NW)
```

Every executable that lives inside a labelled tree inherits the label, and a
process started from a Low-labelled image runs at **Low integrity level** — this
is what was measured with a probe that printed its own token (`whoami /groups`):

| Same binary | Process integrity |
| --- | --- |
| inside the repo | `Low Mandatory Level` (S-1-16-4096) |
| copied outside the repo | `Medium Mandatory Level` (S-1-16-8192) |

Windows mandatory integrity control (MIC) blocks write-up from a lower to a
higher integrity object. A Low process therefore cannot write `%TEMP%`, the
per-user data directory, or the WebView2 profile under `%LOCALAPPDATA%` — while
writes **inside** the labelled tree still succeed. That single rule explains all
three symptoms above: the linker cannot create its temp file, the backend cannot
create its log file, and the webview runtime cannot initialise its profile.

Nothing in this repository causes it, and an unlabelled checkout is unaffected.

## Confirm it in one command

```powershell
icacls "D:\path\to\repo\build.noindex\debug\deps\Flowy.exe" | Select-String Mandatory
```

- `Medium` → the label is not the problem; keep looking.
- `Low` → confirmed. (`icacls` on the repository root shows where it was set.)

Ruled out during the original investigation, all of which left the failure in
place: stopping the `CodexSandboxService`, killing every desktop-AI client
process, and stopping the antivirus real-time drivers. The label is persistent
NTFS state, not a running process — which is why a reboot or closing the
offending application does not help.

## Fix

Run in an **administrator** PowerShell:

```powershell
icacls "<repo>" /setintegritylevel Medium /T /C /Q
```

`/T` walks the whole tree; on a checkout with a populated `build.noindex`
(~100k files) this took about two and a half minutes. The command is idempotent.

Verify:

```powershell
icacls "<repo>\build.noindex\debug\deps\Flowy.exe" | Select-String Mandatory   # Medium
icacls "<repo>" | Select-String Mandatory                                      # Medium
```

Then `bun run dev` starts normally, and the backend uses the default per-user dev
data directory again (`%LOCALAPPDATA%\Flowy\Nomi-dev` on Windows) with the
WebView2 profile under `%LOCALAPPDATA%\<identifier>`.

Rollback (not recommended, only if the labels were there on purpose):

```powershell
icacls "<repo>" /setintegritylevel Low /T /C /Q
```

## Prevention

The label was applied by a desktop AI/sandbox tool that treats the checkout as a
`workspace-write` workspace: it labels the directory tree Low so the tools it
runs can only write inside the workspace. The label survives closing the tool,
so the dev loop stays broken afterwards even though nothing is running.

- Do not open this checkout as a sandboxed workspace in such a tool. If it is
  already registered, remove it from that tool's workspace list (or use a
  checkout outside the labelled path).
- After updating or reinstalling such a tool, re-run the one-line `icacls`
  check above; if the label is back, repeat the fix.
- **Do not** work around it by moving `TEMP`, `NOMIFUN_DATA_DIR` or
  `WEBVIEW2_USER_DATA_FOLDER` into the repository. That only hides the symptom,
  moves every developer's dev data, and leaves the machine broken for anything
  else that runs from the checkout (tests, packaging, `nomicore`).
