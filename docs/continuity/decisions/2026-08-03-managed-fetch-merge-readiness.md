# Managed Fetch rebase and merge-readiness decision

> Superseded snapshot notice (2026-08-04): the historical local-main fast-forward
> described below is not the current delivery state. The live work remains on
> `feat/fetch-optimization`. Its `origin/main=81b199fbc8fa` base is historical:
> the live branch state must be queried before delivery. The cold-start budget,
> endpoint-health, and Desktop model-routing acceptance are recorded in the
> 2026-08-04 follow-ups below. This historical snapshot is not itself the final
> merge approval.

Date: 2026-08-03
Branch: `feat/fetch-optimization` (PR source branch)
Latest main at current rebase: `origin/main=81b199fbc8fa`
Canary code checkpoint before the final documentation amendment: `d11b012ff02e0e846ab5744210fd417bc353bfc3`
Final documentation tip: query with `git rev-parse HEAD` after this record is committed.

## History closure

The original 70 commits were rebuilt as five continuous topic commits:

1. `53a69d3e` — `test(web): establish managed fetch evaluation foundation`
2. `f2c19111` — `test(web): qualify managed fetch admission evidence`
3. `989d3e56` — `feat(web): enable evidence-backed managed fetch on desktop`
4. `4fe63f49` — `fix(web): harden managed fetch safety and campaign boundaries`
5. final tip — `refactor(web): finalize managed fetch maintenance and readiness` (query with `git rev-parse HEAD`)

The pre-rewrite history is recoverable from local branch
`backup/feat-fetch-optimization-pre-compact-aeb9a285`; the pre-final-rebase tip is also
recoverable from `backup/feat-fetch-optimization-pre-final-rebase-24a8cf4`. The tracked-file tree before
and after compression was identical: `e4175ed4feb09308c7e62bed120f9d160dbe51b5`.
Older ADR and Run references remain historical `pre-rewrite checkpoint` evidence; they
are not replaced with new provenance.

The compact history was rebased onto the latest main. The only rebase conflicts were
`CHANGELOG.md` and `docs/reference/configuration.md`; main's current content was kept,
and the Managed Fetch release/configuration entries were added once. No Provider,
Coordinator, Safety, Host, or runtime conflict occurred.

## Behaviour and safety decision

Fetch remains Local-first. Only PDF, JavaScript Shell, and Empty Content are allowed to
enter the EvidenceBacked Parallel MCP Fetch fallback. Ordinary HTML returns from Local
without Remote. Unsupported Document, DNS, TLS, Network, and Timeout remain deferred or
Local-only. HTTP access-control failures, challenge/login/paywall pages, private hosts,
and sensitive URLs never leave the process.

The production call order remains:

```text
ParallelMcpCallPolicy::authorize
-> ManagedMcpCallControl::reserve
-> tools/call
-> observe_result
```

Fetch arguments remain exactly `{urls, full_content=false}`. Requested URL mapping is
canonical and exact; missing, extra, malformed, unmatched, or dropped results fail the
whole Remote batch and restore each Local error. `off` remains the immediate rollback:

```text
NOMIFUN_MANAGED_FETCH_MODE=off
```

## Rebase gates

All targeted gates passed after rebase:

- `cargo test -p flowy-web`: 173 passed;
- `cargo test -p flowy-web --features fetch-eval --all-targets`: 226 library tests + 4 example tests passed;
- `cargo test -p nomifun-ai-agent --features managed-search --test managed_web_handle`: 4 passed;
- `cargo test -p nomifun-app --features managed-search --lib managed_web`: 3 passed;
- `cargo check -p flowy-web --no-default-features`: passed;
- `cargo check -p flowy-web --features fetch-eval`: passed;
- `cargo check --workspace`: passed with existing warnings only;
- `cargo fmt --all -- --check`: passed;
- `git diff --check`: passed;
- `bun run check`: passed when run with the approved elevated process; the restricted
  invocation reports the environment-only `Operation not permitted` error;
- `.github/workflows` contains zero `.yml`/`.yaml` files.

The repository's Cargo config references an unavailable `sccache.exe`; all Rust commands
used the child-process-only override `--config build.rustc-wrapper=""`. No user or repo
configuration was changed.

## Rebase post-Canary

Evidence is under the ignored directory `fetch-evaluation-raw/post-rebase-d11b012f/`.
All five status/safety sidecars are complete, clean, and use the same code/corpus/profile
provenance. Actual calls: 6 Fetch, 0 Search, 0 recovery, 0 429/cooldown.

| Case | Attempts | Remote calls | Effective success | Quality | Warm elapsed |
| --- | ---: | ---: | ---: | --- | --- |
| W3C public PDF | 1 cold + 2 warm | 3 | 3/3 | Q3 | 1.908s, 3.201s |
| ESLint JavaScript Shell | 1 cold + 2 warm | 3 | 3/3 | Q3 | 1.398s, 1.504s |
| Static HTML control | 1 Local | 0 | Local success | Q4 | n/a |

Warm elapsed P95 across the four E2E attempts was 3.201s, below the 8s gate. Safety
counts were zero for sensitive egress, source mismatch, dropped items, retry-limit
violations, and cancellation-late results. Run IDs:

- PDF cold: `019fc711-e022-7fe2-8e05-32e3375fa450`;
- PDF warm: `019fc712-0435-7013-9515-be2061aef146`;
- JS cold: `019fc712-6675-7c33-9ee3-febc0ba53e89`;
- JS warm: `019fc712-8cb1-7293-8b8c-3668849760c3`;
- HTML: `019fc712-c2c0-77f2-9f7b-d83b402f1046`.

The final code checkpoint propagates Managed Search shutdown errors during
startup-failure cleanup, continues other cleanup stages while aggregating failures,
removes stale acceptance claims from the authoritative architecture index, and keeps
the Unreleased changelog at a high-level summary.

This is a bounded preflight/merge regression check, not the formal 15+ URL Admission
Campaign. It cannot produce `candidate_for_enablement` or authorize category expansion.

## Review and delivery boundary

The compression equivalence review passed: five commits, identical tracked tree, and no
unintended file removal. The first rebase integration had only the two documented text
conflicts; the refresh rebase onto `796fa2bf0` was conflict-free. The post-refresh frozen
review ran against production tip `10629dcfa`; the final main tip differs only by this
documentation-only amendment. Any future production-code change requires rerunning the
final review and the bounded Canary.

The previous frozen review at `125877af8` and the post-refresh frozen review at
`10629dcfa` found zero P0/P1 findings on both Standards and Spec axes.
It recorded one non-blocking P2 follow-up: `FileQuotaControl::record_rate_limit` currently
keeps the in-memory rate-limit state when persisting cooldown evidence fails. The current
Runner still stops the batch, but a process crash could lose the durable cooldown. The
follow-up owner is the `flowy-web` Evaluation maintainer; it must add an injected ledger
failure test and surface `quota_ledger_failed` before any future multi-day Admission run.
This does not affect the production MCP path or the bounded Canary decision.

Historical decision: `local_main_fast_forward_complete` (not current). The 2026-08-04
follow-up closed the requested cold Desktop acceptance and separately recorded the model
tool-routing acceptance. The feature branch remains local and has not been merged into
`main`. A subsequent advance of `origin/main` requires a rebase and revalidation before
any PR. No push, GitHub Actions workflow, private URL, Browser-to-MCP loop, or formal
Admission Campaign was run.

Formal 15+ URL Admission, private PDF validation, unsupported/network category expansion,
and any further rollout policy change remain separate future work.

## 2026-08-04 cold-start fix addendum

The live feature tip is `f70a81416586f0abb923836e251b3550e41970d7`, rebased onto
`origin/main=81b199fbc8fa` with ahead/behind `10/0` at the snapshot. The fix replaces
the shared 10-second cold/tool-unknown gate with readiness-specific thresholds of
8/6/4 seconds (Cold/ToolUnknown/Ready) without changing the 12-second absolute
deadline, and protects endpoint recovery with epoch tokens. `application/*+json` and
`application/*+xml` remain Local structured content instead of being misclassified as
Unsupported Document.

Final bounded preflight evidence is in the ignored directory
`fetch-evaluation-raw/post-budget-fix-f70a8141/`: one Cold E2E plus two Warm E2E for
each of the PDF and JavaScript cases, and one Local HTML control. All six Fetch calls
were attempted and successful; HTML made zero Remote calls. Five Safety reports are
complete with all five zero gates; the Summary is explicitly `preflight_never_candidate`.

Review status: budget/readiness review found no P0/P1; the Safety/Lifecycle review found
and fixed stale Session recovery clearing a newer Unauthorized epoch, then passed at the
then-live tip. The fresh Desktop cold-session check subsequently passed. This does not
replace a new frozen review after rebasing to a newer `origin/main`.
