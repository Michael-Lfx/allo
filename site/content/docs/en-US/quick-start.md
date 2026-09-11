# Quick start

Flowy Agent Store packages a local-first agent runtime as a **single executable** with a full Web UI embedded. No database, container, or background service to install — download, launch, and open your browser.

## 1. Download and launch

Get the binary for your platform from the [download center](http://111.170.173.22:10014/downloads/) and run it:

```bash
flowy-agent-store
```

The process starts the local App Server (default `http://localhost:8787`) and opens the workbench in your browser.

> Local-first: execution, credentials and run state live only on your machine. The cloud is used solely for definitions, versions and distribution.

## 2. Import an Agent

The workbench imports experts (Agents), Skills and Connectors from CodeBuddy / WorkBuddy directories. Imported content becomes an **immutable snapshot** you can query and run from the catalog.

## 3. Start a run

Pick an Agent from the catalog and choose **Run**. The workbench submits a Run; events, the plan (DAG) and Artifacts stream into the timeline and artifact panels.

## Next steps

- Read [CLI usage](/en-US/docs/cli) for the full command set.
- Read [Architecture](/en-US/docs/architecture) to understand the Runtime / App Server layering.
- Read the [Compatibility matrix](/en-US/docs/compatibility) for platform and source support.
- Writing your own integration (Node / Electron / browser): read the [TypeScript SDK guide](/en-US/docs/typescript-sdk). **End users take the installer, developers take the npm packages** — two distinct paths.
