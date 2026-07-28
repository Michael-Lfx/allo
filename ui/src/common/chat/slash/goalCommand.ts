/**
 * Parsing for the host-resolved `/goal` + `/subgoal` slash-command families.
 *
 * The backend advertises these commands via the slash-commands endpoint
 * ("goal" / "goal status" / "goal pause" / "goal resume" / "goal clear" and
 * "subgoal" / "subgoal list" / "subgoal remove" / "subgoal clear").
 * They are NOT sent to the agent as normal messages — the SendBox intercepts
 * them at submit time and maps each onto one POST
 * `/api/conversations/{id}/goal` action:
 * - `/goal <text>`  → set (text is the objective)
 * - `/goal status` / `/goal pause` / `/goal resume` / `/goal clear` → verbatim action
 * - `/goal` (no argument) → status
 * - `/subgoal <text>` → add_subgoal
 * - `/subgoal list` (or bare `/subgoal`) → list_subgoals
 * - `/subgoal remove <n>` → remove_subgoal (n is 1-based, matching the list numbering)
 * - `/subgoal clear` → clear_subgoals
 */

export type GoalSlashInvocation =
  | { action: 'status' | 'pause' | 'resume' | 'clear' }
  | { action: 'set'; objective: string }
  | { action: 'add_subgoal'; subgoal: string }
  | { action: 'remove_subgoal'; index_1based: number }
  | { action: 'clear_subgoals' }
  | { action: 'list_subgoals' }
  /** `/subgoal remove` with a missing/non-numeric index — surfaced as a user
   *  hint instead of a request. */
  | { action: 'invalid_subgoal_index' };

const GOAL_COMMAND_RE = /^\/goal(?:\s+([\s\S]+))?$/i;

const SUBGOAL_COMMAND_RE = /^\/subgoal(?:\s+([\s\S]+))?$/i;

const GOAL_SUBCOMMANDS = new Set(['status', 'pause', 'resume', 'clear']);

export function parseGoalSlashCommand(input: string): GoalSlashInvocation | null {
  const match = input.trim().match(GOAL_COMMAND_RE);
  if (!match) {
    return parseSubgoalSlashCommand(input);
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

function parseSubgoalSlashCommand(input: string): GoalSlashInvocation | null {
  const match = input.trim().match(SUBGOAL_COMMAND_RE);
  if (!match) {
    return null;
  }
  const arg = (match[1] ?? '').trim();
  if (!arg) {
    return { action: 'list_subgoals' };
  }
  const keyword = arg.toLowerCase();
  if (keyword === 'list') {
    return { action: 'list_subgoals' };
  }
  if (keyword === 'clear') {
    return { action: 'clear_subgoals' };
  }
  if (keyword === 'remove' || keyword.startsWith('remove ')) {
    const rawIndex = arg.slice('remove'.length).trim();
    const index = Number(rawIndex);
    if (!rawIndex || !Number.isInteger(index) || index < 1) {
      return { action: 'invalid_subgoal_index' };
    }
    return { action: 'remove_subgoal', index_1based: index };
  }
  return { action: 'add_subgoal', subgoal: arg };
}
