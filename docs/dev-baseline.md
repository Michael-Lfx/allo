# Allo development baseline

Date: 2026-08-06  
Branch: `fix/startup-login-config-reset`  
Commit: `eab1ba641`  
Main baseline: `22c0b6510`

## Initial snapshot

The working tree was clean before this execution started. The repository-local
Cargo cache directories were not present at the time of the first read-only
snapshot:

| Path | State |
| --- | --- |
| `build.noindex` | missing |
| `target` | missing |
| `ui/node_modules` | 55.6 MiB, 466 files |

Because the Rust caches were absent, their previous size and compiler reuse
cannot be used as a current measurement. The historical cache incident is
tracked separately; this document records only measurements from this
execution.

## Required runtime measurements

These measurements are completed after the default entry no longer invokes a
destructive prune:

1. Run `bun run dev` twice and record startup duration, `Compiling` lines,
   `[prune-build]` output, Vite/Tauri/Desktop PIDs, and cache sizes.
2. Run the supervisor entry directly twice and record the same values for the
   no-prune A/B comparison.
3. Trigger a work-directory restart and verify that the Vite PID remains the
   same while the Tauri CLI and desktop process are replaced.
4. If a warm run still recompiles broadly, repeat once with
   `CARGO_LOG=cargo::core::compiler::fingerprint=info` and record the first
   dirty fingerprint reason.

The final acceptance section must append the measured values and the exact
commands used. No historical value is considered proof of the current build.
