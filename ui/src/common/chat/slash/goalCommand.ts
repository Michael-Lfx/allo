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
 * - `/goal draft [text]` → draft (text is an optional objective override)
 * - `/goal show` → show (status alias; response carries the contract)
 * - `/goal wait <pid>` → wait (pid barrier)
 * - `/goal unwait` → unwait
 * - `/subgoal <text>` (or `/subgoal add <text>`) → add_subgoal
 * - `/subgoal list` (or bare `/subgoal`) → list_subgoals
 * - `/subgoal remove <n>` → remove_subgoal (n is 1-based, matching the list numbering)
 * - `/subgoal clear` → clear_subgoals
 */

import type { GoalContractDto } from '@/common/adapter/ipcBridge';

export type GoalSlashInvocation =
  | { action: 'status' | 'pause' | 'resume' | 'clear' }
  | { action: 'set'; objective: string }
  | { action: 'add_subgoal'; subgoal: string }
  | { action: 'remove_subgoal'; index_1based: number }
  | { action: 'clear_subgoals' }
  | { action: 'list_subgoals' }
  | { action: 'draft'; objective?: string }
  | { action: 'show' }
  | { action: 'wait'; pid: number }
  | { action: 'unwait' }
  /** No slash command maps here — programmatic entry for contract editing
   *  UIs (an all-empty contract clears the current one). */
  | { action: 'set_contract'; contract: GoalContractDto }
  /** `/subgoal remove` with a missing/non-numeric index — surfaced as a user
   *  hint instead of a request. */
  | { action: 'invalid_subgoal_index' }
  /** `/subgoal add` with no text — surfaced as a user hint instead of a
   *  request. */
  | { action: 'invalid_subgoal_text' }
  /** `/goal wait` with a missing/non-numeric pid — surfaced as a user hint
   *  instead of a request. */
  | { action: 'invalid_wait_pid' };

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
  if (keyword === 'show') {
    return { action: 'show' };
  }
  if (keyword === 'unwait') {
    return { action: 'unwait' };
  }
  if (keyword === 'draft' || keyword.startsWith('draft ')) {
    const objective = arg.slice('draft'.length).trim();
    return objective ? { action: 'draft', objective } : { action: 'draft' };
  }
  if (keyword === 'wait' || keyword.startsWith('wait ')) {
    const rawPid = arg.slice('wait'.length).trim();
    const pid = Number(rawPid);
    if (!rawPid || !Number.isInteger(pid) || pid < 1) {
      return { action: 'invalid_wait_pid' };
    }
    return { action: 'wait', pid };
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
  // `/subgoal add <text>` alias — strip the keyword so it does not leak into
  // the subgoal text; a bare `add` is an empty subgoal, hinted locally.
  if (keyword === 'add' || keyword.startsWith('add ')) {
    const text = arg.slice('add'.length).trim();
    if (!text) {
      return { action: 'invalid_subgoal_text' };
    }
    return { action: 'add_subgoal', subgoal: text };
  }
  return { action: 'add_subgoal', subgoal: arg };
}
