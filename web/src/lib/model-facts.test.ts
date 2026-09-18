import { describe, expect, it } from "vitest";

import {
  capabilityLabels,
  contextWindowTokens,
  costRateText,
  costText,
  modelFactsFor,
  modelKey,
  rateText,
  turnCostUsd,
  turnModelKey,
  validateModelSelection,
} from "./model-facts";
import type { ConversationModelOptions } from "./protocol";

describe("rateText", () => {
  it("formats whole and fractional rates without trailing zeros", () => {
    expect(rateText(3)).toBe("3");
    expect(rateText(0.15)).toBe("0.15");
    expect(rateText(2.5)).toBe("2.5");
    expect(rateText(12.345)).toBe("12.35");
  });

  it("treats missing, zero and non-finite values as unknown", () => {
    expect(rateText(null)).toBeNull();
    expect(rateText(undefined)).toBeNull();
    expect(rateText(0)).toBeNull();
    expect(rateText(Number.NaN)).toBeNull();
    expect(rateText(Number.POSITIVE_INFINITY)).toBeNull();
  });
});

describe("costRateText", () => {
  it("joins both directions when the catalog knows them", () => {
    expect(costRateText({ costInput: 3, costOutput: 15 })).toBe("$3/M in · $15/M out");
  });

  it("shows the single known direction instead of inventing the other", () => {
    expect(costRateText({ costInput: 1 })).toBe("$1/M in");
    expect(costRateText({ costOutput: 2 })).toBe("$2/M out");
  });

  it("returns null when the catalog has no price at all (token-only fallback, D12=A)", () => {
    expect(costRateText({})).toBeNull();
    expect(costRateText({ costInput: null, costOutput: null })).toBeNull();
    expect(costRateText({ costInput: 0, costOutput: 0 })).toBeNull();
  });
});

describe("contextWindowTokens", () => {
  it("prefers the catalog window and falls back to the configured limit", () => {
    expect(contextWindowTokens({ catalogContextWindow: 200000, contextLimit: 128000 })).toBe(200000);
    expect(contextWindowTokens({ contextLimit: 128000 })).toBe(128000);
    expect(contextWindowTokens({ catalogContextWindow: 0, contextLimit: 64000 })).toBe(64000);
  });

  it("is null when neither source knows (never 0)", () => {
    expect(contextWindowTokens({})).toBeNull();
    expect(contextWindowTokens({ catalogContextWindow: null, contextLimit: null })).toBeNull();
    expect(contextWindowTokens({ catalogContextWindow: 0, contextLimit: 0 })).toBeNull();
  });
});

describe("capabilityLabels", () => {
  it("labels only a catalog-declared capability", () => {
    expect(capabilityLabels({ supportsVision: true })).toEqual(["vision"]);
  });

  it("stays empty for false and for unknown — absence is not a denial", () => {
    expect(capabilityLabels({ supportsVision: false })).toEqual([]);
    expect(capabilityLabels({})).toEqual([]);
    expect(capabilityLabels({ supportsVision: null })).toEqual([]);
  });
});

describe("validateModelSelection", () => {
  it("passes an unknown selection when the directory has not loaded yet", () => {
    expect(
      validateModelSelection({ selectedKey: "openai/gpt-5", directoryKeys: [], optionKeys: [] }),
    ).toBeNull();
  });

  it("passes a selection the directory or the options know", () => {
    const input = { directoryKeys: ["openai/gpt-5"], optionKeys: ["anthropic/claude-sonnet-4-5"] };
    expect(validateModelSelection({ ...input, selectedKey: "openai/gpt-5" })).toBeNull();
    expect(validateModelSelection({ ...input, selectedKey: "anthropic/claude-sonnet-4-5" })).toBeNull();
  });

  it("blocks a selection that neither source knows", () => {
    expect(
      validateModelSelection({
        selectedKey: "openai/gpt-typo",
        directoryKeys: ["openai/gpt-5"],
        optionKeys: [],
      }),
    ).toBe("composer.modelUnknown");
  });

  it("ignores an empty selection (the default model path)", () => {
    expect(validateModelSelection({ selectedKey: null, directoryKeys: ["openai/gpt-5"], optionKeys: [] })).toBeNull();
    expect(validateModelSelection({ selectedKey: "  ", directoryKeys: ["openai/gpt-5"], optionKeys: [] })).toBeNull();
  });
});

describe("modelKey", () => {
  it("joins provider and model the way the picker does", () => {
    expect(modelKey("openai", "gpt-5")).toBe("openai/gpt-5");
  });
});

/**
 * W9（R14）：按 turn 的费用。口径与上一批一致——**任一输入缺席就不给数字**：
 * 没费率不算金额、没 token 不算金额、单向费率不算金额（半张账单不是账单）。
 */
describe("modelFactsFor", () => {
  const options = {
    default: null,
    reasoning_efforts: [],
    providers: [
      {
        name: "openai",
        models: [
          {
            name: "gpt-5",
            context_limit: 128_000,
            catalog_context_window: 400_000,
            cost_input: 1.25,
            cost_output: 10,
            supports_vision: true,
          },
          // 目录未映射的模型：整组事实缺席，不是 0。
          { name: "bare-model" },
        ],
      },
    ],
  } as unknown as ConversationModelOptions;

  it("reads the directory facts for a known provider/model key", () => {
    expect(modelFactsFor(options, "openai/gpt-5")).toEqual({
      contextLimit: 128_000,
      catalogContextWindow: 400_000,
      costInput: 1.25,
      costOutput: 10,
      supportsVision: true,
    });
  });

  it("returns null instead of another model's facts when the key is unknown", () => {
    expect(modelFactsFor(options, "openai/gpt-typo")).toBeNull();
    expect(modelFactsFor(options, "other/gpt-5")).toBeNull();
    expect(modelFactsFor(options, "gpt-5")).toBeNull(); // 没有 provider 段的键不算键
    expect(modelFactsFor(options, "")).toBeNull();
    expect(modelFactsFor(options, null)).toBeNull();
    expect(modelFactsFor(null, "openai/gpt-5")).toBeNull();
  });

  it("keeps an unmapped model's missing facts absent (never zeros)", () => {
    expect(modelFactsFor(options, "openai/bare-model")).toEqual({
      contextLimit: undefined,
      catalogContextWindow: undefined,
      costInput: undefined,
      costOutput: undefined,
      supportsVision: undefined,
    });
    expect(costRateText(modelFactsFor(options, "openai/bare-model") ?? {})).toBeNull();
  });
});

describe("turnModelKey", () => {
  it("prefers the explicit selection, then the conversation's own model", () => {
    expect(turnModelKey("openai/gpt-5", { provider_id: "anthropic", model: "claude" })).toBe("openai/gpt-5");
    expect(turnModelKey(null, { provider_id: "anthropic", model: "claude" })).toBe("anthropic/claude");
    expect(turnModelKey("  ", { provider_id: "anthropic", model: "claude" })).toBe("anthropic/claude");
  });

  it("returns null when neither source names a model (no rate attribution)", () => {
    expect(turnModelKey(null, null)).toBeNull();
    expect(turnModelKey(undefined, { provider_id: "", model: "claude" })).toBeNull();
    expect(turnModelKey(null, { provider_id: "anthropic", model: "" })).toBeNull();
  });
});

describe("turnCostUsd", () => {
  const facts = { costInput: 1.25, costOutput: 10 };

  it("prices both directions from the catalog rate when rate and tokens are both there", () => {
    // 1200 input @ $1.25/M + 340 output @ $10/M
    expect(turnCostUsd(facts, { input_tokens: 1_200, output_tokens: 340 })).toBeCloseTo(0.0049, 10);
  });

  it("returns null when either rate is missing (half a bill is not a bill)", () => {
    expect(turnCostUsd({ costInput: 1.25 }, { input_tokens: 1_200, output_tokens: 340 })).toBeNull();
    expect(turnCostUsd({ costOutput: 10 }, { input_tokens: 1_200, output_tokens: 340 })).toBeNull();
    expect(turnCostUsd({ costInput: 0, costOutput: 0 }, { input_tokens: 1_200, output_tokens: 340 })).toBeNull();
    expect(turnCostUsd(null, { input_tokens: 1_200, output_tokens: 340 })).toBeNull();
  });

  it("returns null when the tokens are unknown (never inferred from context occupancy)", () => {
    expect(turnCostUsd(facts, null)).toBeNull();
    expect(turnCostUsd(facts, { input_tokens: 0, output_tokens: 0 })).toBeNull();
    expect(turnCostUsd(facts, { input_tokens: Number.NaN, output_tokens: 10 })).toBeNull();
    expect(turnCostUsd(facts, { input_tokens: -1, output_tokens: 10 })).toBeNull();
  });
});

describe("costText", () => {
  it("scales precision to the magnitude and trims trailing zeros", () => {
    expect(costText(0.0049)).toBe("$0.0049");
    expect(costText(0.5)).toBe("$0.5");
    expect(costText(1.25)).toBe("$1.25");
    expect(costText(12)).toBe("$12");
    expect(costText(0.00004)).toBe("$0.00004");
  });

  it("returns null for a missing or non-positive amount (no `$0` standing in)", () => {
    expect(costText(null)).toBeNull();
    expect(costText(undefined)).toBeNull();
    expect(costText(0)).toBeNull();
    expect(costText(-1)).toBeNull();
    expect(costText(Number.NaN)).toBeNull();
    expect(costText(Number.POSITIVE_INFINITY)).toBeNull();
  });
});
