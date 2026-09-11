import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const guide = (locale: "zh-CN" | "en-US") =>
  readFileSync(resolve(here, "../../../../site/content/docs", locale, "typescript-sdk.md"), "utf8");

/**
 * §7.3 quotes a method count in prose and §7.4 documents the approval answer
 * path. Neither is machine-readable in the guide itself, so both are pinned
 * here: mapping a new method (or dropping §7.4) fails this test and forces the
 * guide to be updated in the same change. `scripts/check-docs-sync.mjs` only
 * checks zh/en structural parity — it cannot see content drift like this.
 */
describe("developer guide ↔ code drift guard", () => {
  it("quotes the same 45 / 64 split the route table has", () => {
    for (const locale of ["zh-CN", "en-US"] as const) {
      expect(guide(locale), `${locale} must quote the current split`).toContain("45 / 64");
    }
  });

  it("documents run/answer-decision, its sub-client method and the CAS contract", () => {
    for (const locale of ["zh-CN", "en-US"] as const) {
      const text = guide(locale);
      expect(text, `${locale} §7.2 must list answerDecision`).toContain("answerDecision(input)");
      expect(text, `${locale} must name the protocol method`).toContain("run/answer-decision");
      expect(text, `${locale} §7.4 must exist`).toContain("### 7.4");
      for (const token of [
        "expectedExecutionVersion",
        "expectedStepVersion",
        "expectedAttemptVersion",
      ]) {
        expect(text, `${locale} must document ${token}`).toContain(token);
      }
      expect(text, `${locale} must state the always_allow red line`).toContain("always_allow");
    }
  });
});
