# @flowy-agent-store/runtime-win32-x64

Vendored `flowy-agent-store.exe` (Agent Store App Server runtime, win32-x64) for
[`@flowy-agent-store/sdk`](https://www.npmjs.com/package/@flowy-agent-store/sdk).

`@flowy-agent-store/sdk` declares the `runtime-<platform>-<arch>` packages as
`optionalDependencies`; npm installs only the one matching the host platform
and `resolveAppServerBin` finds the binary via `require.resolve`.

Version-locked with the SDK: `runtime@X` pairs with `sdk@X` (protocol
`2026-08-26`). Install both explicitly if you use `--no-optional`:

```sh
npm i @flowy-agent-store/sdk @flowy-agent-store/runtime-win32-x64
```
