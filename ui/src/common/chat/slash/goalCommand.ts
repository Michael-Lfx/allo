/**
 * Parsing for the host-resolved `/goal` slash-command family.
 *
 * The backend advertises five commands via the slash-commands endpoint
 * ("goal" / "goal status" / "goal pause" / "goal resume" / "goal clear").
 * They are NOT sent to the agent as normal messages — the SendBox intercepts
 * them at submit time and maps each onto one POST
 * `/api/conversations/{id}/goal` action:
 * - `/goal <text>`  → set (text is the objective)
 * - `/goal status` / `/goal pause` / `/goal resume` / `/goal clear` → verbatim action
 * - `/goal` (no argument) → status
 */

export type GoalSlashInvocation =
  | { action: 'status' | 'pause' | 'resume' | 'clear' }
  | { action: 'set'; objective: string };

const GOAL_COMMAND_RE = /^\/goal(?:\s+([\s\S]+))?$/i;

const GOAL_SUBCOMMANDS = new Set(['status', 'pause', 'resume', 'clear']);

export function parseGoalSlashCommand(input: string): GoalSlashInvocation | null {
  const match = input.trim().match(GOAL_COMMAND_RE);
  if (!match) {
    return null;
  }
  const arg = (match[1] ?? '').trim();
  if (!arg) {
    return { action: 'status' };
  }
  const keyword = arg.toLowerCase();
  if (GOAL_SUBCOMMANDS.has(keyword)) {
    return { action: keyword as 'status' | 'pause' | 'resume' | 'clear' };
  }
  return { action: 'set', objective: arg };
}
