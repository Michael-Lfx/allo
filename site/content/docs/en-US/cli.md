# CLI usage

`flowy-agent-store` is the entry point of the packaged single-file runtime. Most interaction happens in the browser workbench; the CLI launches the runtime and handles a few local operations.

## Launch the Web UI

```bash
flowy-agent-store serve
flowy-agent-store serve --port 8787 --open
```

| Flag | Description |
| --- | --- |
| `--port` | App Server port, default `8787` |
| `--open` | Open the workbench in the default browser after launch |
| `--host` | Bind address, default `127.0.0.1` (local only) |

## Import

```bash
flowy-agent-store import ./my-plugin.codebuddy-plugin
```

The Importer validates paths, computes a content digest, and produces a compatibility report (compatible / manual-review / unsupported).

## Run (optionally from the CLI)

```bash
flowy-agent-store run --agent software-company.architect
flowy-agent-store run --team software-company
```

Run requests go through the local App Server's versioned protocol; actual execution happens in the `allo` Runtime. Workflows, retries and replan are driven by the Runtime's internal planner.

## Status

```bash
flowy-agent-store status
flowy-agent-store list agents
```

> The CLI and Web UI share the same App Server protocol boundary — neither touches the internal database or credential storage directly.
