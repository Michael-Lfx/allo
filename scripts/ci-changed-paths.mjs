/**
 * Decide whether a change can affect the Rust build, so CI can skip
 * `cargo check` / `cargo test` for documentation-only pull requests.
 *
 * Two rules keep that skip safe:
 *
 *   1. The allowlist is narrow and by prefix, never by file extension. Markdown
 *      is *compiled into Rust* in this repository:
 *      `crates/agent/nomi-agent/src/goal/templates/` (goal/runtime.rs,
 *      horizon/delta.rs), `crates/agent/nomi-vimax/skills/builtin/`
 *      (skills/builtin.rs) and
 *      `crates/backend/nomifun-learning/assets/tutorial/` (tutorial.rs) all
 *      reach the binary through `include_str!`. Treating every `.md` as
 *      documentation would skip `cargo test -p nomi-agent --lib` — the one
 *      suite that covers those templates — for a pull request that edits them.
 *   2. Every unknown is answered with "run the Rust jobs". An empty change set,
 *      an unrecognised event, a missing or all-zero base commit and a failing
 *      `git` all yield `docsOnly: false`. The one failure this file must never
 *      have is a silent skip.
 *
 * Paths are matched with forward slashes, the way `git diff --name-only` prints
 * them.
 *
 * Usage:
 *   bun scripts/ci-changed-paths.mjs              # decide; write $GITHUB_OUTPUT
 *   bun scripts/ci-changed-paths.mjs --self-test  # assert the contract above
 */

import { execFileSync } from "node:child_process";
import { appendFileSync, readFileSync } from "node:fs";

/**
 * Paths whose changes cannot affect the Rust build. Deliberately narrow: a
 * missing entry costs one unnecessary Rust run, an extra entry costs a gate.
 */
export const DOC_PATH_PATTERNS = [
  /^docs\//, // the docs tree: 255 tracked .md files plus their assets
  /^[^/]+\.md$/, // markdown at the repository root
  /^\.github\/[^/]+\.md$/, // .github markdown: PR template, copilot instructions
];

export function isDocPath(path) {
  return DOC_PATH_PATTERNS.some((pattern) => pattern.test(path));
}

/** An empty change set is *not* docs-only: "nothing changed" is never proven. */
export function isDocsOnly(paths) {
  return paths.length > 0 && paths.every(isDocPath);
}

/**
 * Derive the commits to diff from the event payload, or `null` when the event
 * cannot be reasoned about.
 */
export function diffRange({ eventName, event, headSha }) {
  if (eventName === "pull_request") {
    const base = event?.pull_request?.base?.sha;
    const head = event?.pull_request?.head?.sha ?? headSha;
    if (!base || !head) {
      return null;
    }
    // The merge base, not the recorded base tip: `base.sha` is where the base
    // branch pointed when the event fired, so diffing straight against it would
    // also report everything merged into `main` since, and a docs-only pull
    // request would stop being recognised as soon as `main` moved.
    return { base, head, mergeBase: true };
  }

  if (eventName === "push") {
    const before = event?.before;
    const head = event?.after ?? headSha;
    // A branch's first push reports an all-zero `before`: nothing to diff.
    if (!before || /^0+$/.test(before) || !head) {
      return null;
    }
    // A push *is* `before..after`, so no merge base: after a force push the two
    // commits need not share one, and the (large) path set that produces is the
    // safe answer.
    return { base: before, head, mergeBase: false };
  }

  return null;
}

/**
 * The changed paths, or `null` when they cannot be determined. Never throws.
 *
 * @param {(args: string[]) => string} run git runner returning stdout.
 */
export function changedPaths({ eventName, event, headSha, run }) {
  const range = diffRange({ eventName, event, headSha });
  if (!range) {
    return null;
  }
  try {
    const base = range.mergeBase ? run(["merge-base", range.base, range.head]).trim() : range.base;
    if (!base) {
      return null;
    }
    // `--no-renames` is load-bearing twice over. With rename detection a move
    // from `crates/.../templates/a.md` to `docs/a.md` reports only the new path,
    // and the deletion of a compiled template would be mistaken for a docs-only
    // change; and rename detection needs blob contents, which a blobless clone
    // does not carry.
    return run(["diff", "--name-only", "--no-renames", base, range.head])
      .split("\n")
      .map((line) => line.trim())
      .filter((line) => line.length > 0);
  } catch {
    return null;
  }
}

/** `{ docsOnly, paths, reason }` — `docsOnly` is false whenever anything is unknown. */
export function decide({ eventName, event, headSha, run }) {
  const paths = changedPaths({ eventName, event, headSha, run });
  if (paths === null) {
    return {
      docsOnly: false,
      paths: [],
      reason: `cannot determine the changed paths of a \`${eventName || "unknown"}\` event`,
    };
  }
  if (paths.length === 0) {
    return { docsOnly: false, paths, reason: "the change set is empty" };
  }
  if (isDocsOnly(paths)) {
    return { docsOnly: true, paths, reason: `all ${paths.length} changed paths are documentation` };
  }
  const offenders = paths.filter((path) => !isDocPath(path));
  return {
    docsOnly: false,
    paths,
    reason: `not documentation: ${offenders.slice(0, 5).join(", ")}${offenders.length > 5 ? ", …" : ""}`,
  };
}

function gitRun(args) {
  return execFileSync("git", args, { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] });
}

function readEventPayload() {
  const path = process.env.GITHUB_EVENT_PATH;
  if (!path) {
    return null;
  }
  try {
    return JSON.parse(readFileSync(path, "utf8"));
  } catch {
    return null; // decide() then reports that it cannot tell, i.e. run the jobs
  }
}

function selfTest() {
  let assertions = 0;
  const fail = (what, actual, expected) => {
    throw new Error(`${what}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
  };
  const eq = (what, actual, expected) => {
    assertions++;
    if (JSON.stringify(actual) !== JSON.stringify(expected)) {
      fail(what, actual, expected);
    }
  };

  const PR_BASE = "1".repeat(40);
  const PR_HEAD = "2".repeat(40);
  const MERGE_BASE = "3".repeat(40);
  const PUSH_BEFORE = "4".repeat(40);
  const PUSH_AFTER = "5".repeat(40);
  const pullRequestEvent = { pull_request: { base: { sha: PR_BASE }, head: { sha: PR_HEAD } } };
  const pushEvent = { before: PUSH_BEFORE, after: PUSH_AFTER };

  // The allowlist itself.
  eq("docs/", isDocsOnly(["docs/agent-store/32-expert-pack-export.zh.md"]), true);
  eq("root markdown", isDocsOnly(["README.md", "AGENTS.md", "docs/images/a.png"]), true);
  eq(".github markdown", isDocsOnly([".github/pull_request_template.md"]), true);
  eq("empty set", isDocsOnly([]), false);
  eq("mixed", isDocsOnly(["docs/a.md", "crates/backend/nomifun-db/migrations/060_x.sql"]), false);
  // The `include_str!` hazard: these are Rust sources in markdown clothing.
  eq(
    "nomi-agent goal template",
    isDocsOnly(["crates/agent/nomi-agent/src/goal/templates/continuation.md"]),
    false,
  );
  eq(
    "nomi-agent horizon template",
    isDocsOnly(["crates/agent/nomi-agent/src/goal/templates/continuation_delta.md"]),
    false,
  );
  eq(
    "nomi-vimax builtin skill",
    isDocsOnly(["crates/agent/nomi-vimax/skills/builtin/luxury-tvc/SKILL.md"]),
    false,
  );
  eq(
    "nomifun-learning tutorial",
    isDocsOnly(["crates/backend/nomifun-learning/assets/tutorial/README.md"]),
    false,
  );
  // Everything else stays out of the allowlist on purpose.
  eq("this workflow", isDocsOnly([".github/workflows/ci.yml"]), false);
  eq("ui markdown", isDocsOnly(["ui/README.md"]), false);
  eq("web markdown", isDocsOnly(["web/README.md"]), false);

  // Range derivation.
  eq(
    "pull_request range",
    diffRange({ eventName: "pull_request", event: pullRequestEvent, headSha: PR_HEAD }),
    { base: PR_BASE, head: PR_HEAD, mergeBase: true },
  );
  eq(
    "push range",
    diffRange({ eventName: "push", event: pushEvent, headSha: PUSH_AFTER }),
    { base: PUSH_BEFORE, head: PUSH_AFTER, mergeBase: false },
  );
  eq("branch creation", diffRange({ eventName: "push", event: { before: "0".repeat(40), after: PUSH_AFTER }, headSha: PUSH_AFTER }), null);
  eq("unknown event", diffRange({ eventName: "workflow_dispatch", event: {}, headSha: PR_HEAD }), null);
  eq("payload missing", diffRange({ eventName: "pull_request", event: null, headSha: PR_HEAD }), null);

  // The exact git calls, and the argv that closes the rename hole.
  const recording = (outputs) => {
    const calls = [];
    return {
      calls,
      run: (args) => {
        calls.push(args);
        return outputs[calls.length - 1] ?? "";
      },
    };
  };
  const pr = recording([`${MERGE_BASE}\n`, "docs/a.md\nREADME.md\n"]);
  eq(
    "pull_request calls",
    changedPaths({ eventName: "pull_request", event: pullRequestEvent, headSha: PR_HEAD, run: pr.run }),
    ["docs/a.md", "README.md"],
  );
  eq("pull_request argv", pr.calls, [
    ["merge-base", PR_BASE, PR_HEAD],
    ["diff", "--name-only", "--no-renames", MERGE_BASE, PR_HEAD],
  ]);

  const push = recording(["crates/agent/nomi-agent/src/goal/templates/goal_context.md\n"]);
  eq(
    "push calls",
    changedPaths({ eventName: "push", event: pushEvent, headSha: PUSH_AFTER, run: push.run }),
    ["crates/agent/nomi-agent/src/goal/templates/goal_context.md"],
  );
  eq("push argv has no merge-base", push.calls, [
    ["diff", "--name-only", "--no-renames", PUSH_BEFORE, PUSH_AFTER],
  ]);

  // A failing git never throws out of the decision, it runs the Rust jobs.
  const boom = () => {
    throw new Error("git: command not found");
  };
  eq("git failure is not docs-only", decide({ eventName: "push", event: pushEvent, headSha: PUSH_AFTER, run: boom }).docsOnly, false);
  eq("empty diff is not docs-only", decide({ eventName: "push", event: pushEvent, headSha: PUSH_AFTER, run: () => "\n" }).docsOnly, false);
  eq(
    "docs-only decision",
    decide({ eventName: "pull_request", event: pullRequestEvent, headSha: PR_HEAD, run: recording([`${MERGE_BASE}\n`, "docs/a.md\n"]).run }).docsOnly,
    true,
  );
  eq(
    "code decision names the offender",
    decide({ eventName: "pull_request", event: pullRequestEvent, headSha: PR_HEAD, run: recording([`${MERGE_BASE}\n`, "docs/a.md\nsrc/main.rs\n"]).run }).reason,
    "not documentation: src/main.rs",
  );

  console.log(`ci-changed-paths: ${assertions} assertions passed`);
}

if (import.meta.main) {
  if (process.argv.slice(2).includes("--self-test")) {
    selfTest();
  } else {
    const result = decide({
      eventName: process.env.GITHUB_EVENT_NAME ?? "",
      event: readEventPayload(),
      headSha: process.env.GITHUB_SHA,
      run: gitRun,
    });

    const output = process.env.GITHUB_OUTPUT;
    if (output) {
      appendFileSync(output, `docs_only=${result.docsOnly}\n`);
    } else {
      // Not fatal: a job that receives no output sees an empty string, which is
      // still `!= 'true'` in the workflow's condition, so the Rust jobs run.
      console.warn("GITHUB_OUTPUT is unset; printing the decision instead of exporting it");
    }

    console.log(`docs_only=${result.docsOnly} — ${result.reason}`);
    for (const path of result.paths) {
      console.log(`  ${path}`);
    }
  }
}
