# Goals

**Goals** turn a Nomi conversation into a self-driving loop: you state an
objective once, and after every agent turn a lightweight **judge** checks the
latest response against that objective. If the goal is not done yet, the loop
automatically feeds the agent a continuation prompt — up to a bounded turn
budget — so long-running work keeps moving without you re-poking it.

Goals compose with [AutoWork](autowork-requirements.md) and
[IDMM](intelligent-decision.md): AutoWork decides *what* to work on, IDMM keeps
each turn *alive*, and a goal keeps the session pointed at *one outcome* until
it is verifiably reached.

> Goals are per-conversation and currently available in **Nomi** conversations
> only. Everything below is driven from the chat input with slash commands; the
> same operations are exposed over the HTTP API
> (`POST /api/conversations/{id}/goal`).

## Quick start

```text
/goal Ship the CSV export feature and make `bun test` pass
```

That is all. The agent works normally; after each of its turns the judge reads
the response and returns one of three verdicts:

- **done** — the objective is satisfied (or provably blocked); the loop stops.
- **continue** — not done, there is a concrete next step; the loop injects a
  continuation prompt and the agent takes another turn.
- **wait** — not done, but the next step is to wait for something asynchronous
  (a build, a background session, a rate-limit cooldown); the loop parks on a
  [wait barrier](#wait-barriers) without burning a turn.

A status notice above the input shows the objective, a turn-budget progress
bar, pause/resume/clear controls, and — when present — subgoals, the
completion contract, and the active wait barrier.

## `/goal` commands

| Command | Effect |
| --- | --- |
| `/goal <objective>` | Set (or replace) the goal and start the loop. |
| `/goal status` | Show the current goal, verdict, budget, and barrier. |
| `/goal show` | Alias of `status`; the toast additionally lists the contract fields. |
| `/goal pause` | Pause the loop. Drops any wait barrier; subgoals are kept. |
| `/goal resume` | Resume a paused goal. The turn budget and failure counters start fresh; any stale barrier is dropped. |
| `/goal clear` | End the goal entirely — objective, subgoals, contract, and barrier are all cleared. |
| `/goal draft [objective]` | Ask the model to draft a [completion contract](#completion-contracts) for the given (or current) objective and apply it immediately. |
| `/goal wait <pid>` | Manually park the goal until process `pid` exits. Overrides any judge-set barrier. |
| `/goal unwait` | Drop the current wait barrier and go back to active judging. |

Notes:

- `/goal draft` is an LLM call and takes a few seconds; the UI shows a loading
  toast and then the drafted contract, field by field.
- `/goal wait` rejects pid `0` and non-numeric input locally; the backend also
  rejects waits on a goal that is already complete or cleared.
- `/goal unwait` on a goal that is not waiting is an error ("The goal is not
  waiting").

## `/subgoal` commands

Subgoals are extra acceptance criteria you can add mid-loop. The judge must
find concrete evidence for **every** subgoal before it may return *done*.

| Command | Effect |
| --- | --- |
| `/subgoal <text>` or `/subgoal add <text>` | Append a criterion. |
| `/subgoal list` | List the current criteria. |
| `/subgoal remove <n>` | Remove criterion number `n` (1-based). |
| `/subgoal clear` | Remove all criteria. |

Subgoals survive `pause`/`resume`; only `/goal clear` removes them together
with the goal.

## Turn budget and the judge

Every automatic continuation consumes one unit of the goal's **turn budget**
(default **8**, configurable per request via the API's `max_turns` field,
clamped to 1..100). When the budget runs out the loop simply stops continuing —
the goal stays visible and `/goal resume` restarts it with a fresh budget.
Waiting on a barrier does **not** burn budget, and the budget window restarts
with a new backend process.

The judge itself is deliberately cheap and safe:

- It is a **one-shot side request** on the session's main model — no tools, no
  thinking, and it never touches the conversation history or system prompt, so
  the main prompt cache stays intact.
- It sees the objective, subgoals/contract, a bounded tail of the agent's last
  response, and a snapshot of live background processes, and must answer with
  a strict one-line JSON verdict.
- It is **fail-open, never falsely done**: an unparseable or failed judge call
  degrades to *continue*. Repeated parse or transport failures trip separate
  circuit breakers that pause the goal instead of looping forever.

A message typed by *you* always takes priority: the loop yields to user input
rather than racing it.

## Wait barriers

A *wait* verdict (or a manual `/goal wait`) parks the goal on one of three
barrier types:

| Barrier | Set by | Releases when |
| --- | --- | --- |
| **Session** (`wait_on_session`) | judge | the background session exits or its watch trigger fires |
| **Process** (`wait_on_pid`) | judge or `/goal wait <pid>` | the process exits |
| **Time** (`wait_for_seconds`) | judge | the deadline passes |

When several are conceptually relevant, the effective priority is
**session > pid > time**. The UI shows the barrier line(s) with an inline
**Unwait** button.

Release is **lazy**: nothing wakes the loop the instant the barrier clears.
Instead, at the next evaluation point (the next time the loop would judge) the
runtime checks the barrier — if the deadline has passed, the pid is dead, or
the session is done, it silently clears the barrier and resumes normal judging
in that same call. This is also why the countdown reaching zero reads "will
resume on the next turn" rather than pretending the loop is already running.

Pid/session liveness is checked through a host-injected probe and is
**fail-open**: if liveness cannot be positively confirmed, the barrier
releases. A stale barrier can never wedge the loop; the worst case is resuming
one turn early, which is safe.

## Completion contracts

A **contract** replaces "the judge eyeballs the response" with an explicit,
reviewable definition of done. It has five fields, all optional strings:

| Field | Meaning |
| --- | --- |
| `outcome` | The single end state that must be true when done. |
| `verification` | The concrete test / command / artifact that **proves** the outcome. With a contract present, this is the judge's sole authoritative definition of done. |
| `constraints` | Rules the work must respect along the way. |
| `boundaries` | What is explicitly out of scope. |
| `stop_when` | Conditions under which the agent should stop and report instead of pushing on (hitting one is treated as *done, blocked*). |

Set one with `/goal draft` (the model drafts it from the objective and it is
applied immediately — review it in the status notice's contract section) or
programmatically via the API's `set_contract` action. Sending a contract with
all five fields empty clears it. Subgoals added alongside a contract fold into
it as extra criteria.

## Configuration

There is no config-file surface for goals. The only knob is the turn budget:
pass `max_turns` with the API `set` action (default **8**, clamped to 1..100).
Slash commands always use the default.

## Persistence

Goal state (objective, status, budget, subgoals, contract, verdicts) is
persisted per conversation and survives restarts. Counters that describe a
*live* run — the auto-continuation window and breaker counts — intentionally
start fresh with a new process.

## Origin and differences from hermes

The goal loop is a port of the hermes agent's goals feature (itself inspired
by the "Ralph loop" pattern). Behavior matches hermes' semantics — the judge
prompts, three-verdict contract, lazy barrier release, and fail-open probes
are direct ports — with these deliberate differences:

- Default turn budget is **8** (hermes: 20), and it is set per request via the
  API instead of a `config.yaml`.
- Contracts are set via `/goal draft` or the API only — there is no inline
  `field: value` contract syntax in the objective text.
- `/goal wait <pid>` takes no free-text reason argument.
- The judge runs on the session's main model; there is no separate auxiliary
  judge-model setting.
- Goals are available in Nomi conversations only.
