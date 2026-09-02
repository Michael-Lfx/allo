/**
 * UI-side view of the App Server link lifecycle.
 *
 * Distinct from `TransportPhase` in `src/lib/errors.ts`, which classifies
 * *where* a failure happened inside one request. This type is what the shell
 * renders (`status-light`, composer dot, connect button) and what gates
 * every control that requires a live link.
 */
export type ConnectionPhase = "offline" | "connecting" | "online";
