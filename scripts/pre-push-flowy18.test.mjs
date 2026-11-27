import { describe, expect, test } from "bun:test";
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  parsePushLines,
  runPrePushCheck,
  shouldTriggerFlowy18Check,
  touchesFlowy18Paths,
} from "./pre-push-flowy18.mjs";

describe("pre-push flowy18 trigger logic", () => {
  test("ignores branch deletions (zero local oid)", () => {
    const parsed = parsePushLines(
      `refs/heads/old 0000000000000000000000000000000000000000 refs/heads/old abcdef1234567890abcdef1234567890abcdef12\n` +
      `refs/heads/feat abcdef1234567890abcdef1234567890abcdef12 refs/heads/feat 0000000000000000000000000000000000000000\n`,
    );
    expect(parsed.length).toBe(1);
    expect(parsed[0].localRef).toBe("refs/heads/feat");
  });

  test("triggers only for flowy18_wip or flowy18-ui workflow changes", () => {
    expect(touchesFlowy18Paths(["scripts/prune-build.mjs", "README.md"])).toBe(false);
    expect(touchesFlowy18Paths(["flowy18_wip/crates/flowy-app/src/lib.rs"])).toBe(true);
    expect(touchesFlowy18Paths([".github/workflows/flowy18-ui.yml"])).toBe(true);
    expect(
      shouldTriggerFlowy18Check({
        updates: [{ localRef: "a", localOid: "111", remoteRef: "b", remoteOid: "000" }],
        listChangedFiles: () => ["crates/backend/foo.rs"],
      }),
    ).toBe(false);
  });

  test("respects FLOWY_SKIP_PREPUSH_CHECK=1 only for the flowy18 fast check", () => {
    const sandbox = mkdtempSync(join(tmpdir(), "pre-push-flowy18-"));
    try {
      const fastPath = join(sandbox, "flowy18_wip", "scripts", "check-fast.mjs");
      mkdirSync(join(sandbox, "flowy18_wip", "scripts"), { recursive: true });
      writeFileSync(fastPath, "console.log('ok')\n");

      let called = false;
      const code = runPrePushCheck({
        repoRoot: sandbox,
        remoteName: "origin",
        stdinText: "refs/heads/x 0000000000000000000000000000000000000000 refs/heads/x 1111111111111111111111111111111111111111\n",
        env: { FLOWY_SKIP_PREPUSH_CHECK: "1" },
        runner: () => {
          called = true;
          return 0;
        },
      });
      expect(code).toBe(0);
      expect(called).toBe(false);
    } finally {
      rmSync(sandbox, { recursive: true, force: true });
    }
  });
});
