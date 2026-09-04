/**
 * Readiness protocol (docs/agent-store/12 P0-1).
 *
 * The runtime prints exactly one machine-readable line on stdout after the
 * socket is bound. Tracing logs share stdout, so the host scans lines for a
 * JSON object with `"agent_store": "listening"` and ignores everything else.
 */

export interface ReadinessInfo {
  host: string;
  port: number;
  url: string;
  protocol_version: string;
  version: string;
  auth: string;
}

/** Parse one stdout line; `null` when it is not the readiness line. */
export function parseReadinessLine(line: string): ReadinessInfo | null {
  let parsed: unknown;
  try {
    parsed = JSON.parse(line);
  } catch {
    return null;
  }
  if (typeof parsed !== "object" || parsed === null) return null;
  const record = parsed as Record<string, unknown>;
  if (record["agent_store"] !== "listening") return null;
  const { host, port, url, protocol_version, version, auth } = record;
  if (
    typeof host !== "string" ||
    typeof port !== "number" ||
    typeof url !== "string" ||
    typeof protocol_version !== "string" ||
    typeof version !== "string" ||
    typeof auth !== "string"
  ) {
    return null;
  }
  return { host, port, url, protocol_version, version, auth };
}
