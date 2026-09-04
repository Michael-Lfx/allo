# CLI usage

`flowy-agent-store` is the entry point of the packaged single-file runtime: the backend and the embedded Web UI are served on the **same port**. Most interaction happens in the browser workbench; the CLI only takes launch options — just run it, no subcommand:

## Launch the Web UI

```bash
flowy-agent-store
flowy-agent-store --port 8787
```

By default it listens on `http://127.0.0.1:8787/` and opens the workbench in your default browser.

| Flag | Description | Default |
| --- | --- | --- |
| `--port <n>` | Listen port (shared by the API and the embedded Web UI) | `8787` |
| `--host <ip>` | Bind address; keep `127.0.0.1` for local-only | `127.0.0.1` |
| `--data-dir <dir>` | Backend data directory (database + storage) | Per-user Flowy/Nomi directory |
| `--auth` | Require login; without it the local trusted mode needs no login | Off |
| `--no-open` | Don't open the browser after start | Opens |
| `--admin-user <name>` | Admin username provisioned on first run (authenticated mode) | `admin` |
| `--admin-password <pw>` | Admin password provisioned on first run; if omitted, the first workbench visitor creates it | None |
| `-h, --help` / `-V, --version` | Show help / version | — |

## Examples

```bash
# Port taken? Pick another (the UI follows the page origin, no manual change needed)
flowy-agent-store --port 8788

# Access from other machines on the LAN (pair with --auth)
flowy-agent-store --host 0.0.0.0 --auth

# Login-required setup with a pre-seeded admin
flowy-agent-store --auth --admin-user admin --admin-password <pw>

# Custom data directory / headless server without opening a browser
flowy-agent-store --data-dir /data/agent-store --no-open
```

## Environment variables

Every option also has an env equivalent: `AGENT_STORE_HOST`, `AGENT_STORE_PORT`, `AGENT_STORE_AUTH` (`1`/`true`/`yes`/`on` enables), `NOMIFUN_DATA_DIR` / `FLOWY_DATA_DIR` (data directory), `NOMIFUN_ADMIN_USERNAME` / `NOMIFUN_ADMIN_PASSWORD` (first-run admin in authenticated mode).

## Notes

- Fixed port by design: if it's taken, the process exits with an error — stop the other holder (desktop app or a previous instance) or pass another `--port`.
- Zero-config UI: the embedded workbench connects to the same origin it was served from, so changing `--port` / `--host` needs no manual address change; Settings can still override it.
- Exclusive data-directory lock: it shares state with the desktop app by default, so running both at once fails fast — that write-protection is intentional, not a bug.
- Binding a non-loopback address (`--host 0.0.0.0`) without `--auth` gives anyone who can reach the port full host access — use a trusted network or require login.
- Import, runs and status all live in the Web UI workbench. The CLI and Web UI share the same App Server protocol boundary — neither touches the internal database or credential storage directly.
