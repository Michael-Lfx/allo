import { describe, expect, it, vi } from "vitest";

import type { SkillSummary } from "@flowy-agent-store/protocol";

/**
 * W12 skill management surface (`16` R17) — render tests.
 *
 * The store half is covered by `store/skillAdmin.test.ts`; this file pins what
 * the *components* show, rendering the prop-driven halves (same pattern as the
 * other render tests here). The rules asserted:
 *
 * - a writable skill offers 编辑 / 复制 / 删除;
 * - a read-only skill offers **no** write button and instead says why, using the
 *   origin the host reported (`16` R17: 只读来源不给写按钮并给出原因);
 * - an unknown origin is treated as not-writable, so a host that predates the
 *   field never makes the UI offer a write it cannot justify;
 * - the edit form cannot smuggle a rename, and a form where nothing changed
 *   cannot be submitted at all (the server refuses an empty patch anyway).
 */

vi.stubGlobal("localStorage", { getItem: () => null, setItem: () => {}, removeItem: () => {} });
vi.stubGlobal("navigator", { language: "zh-CN" });
vi.stubGlobal("window", {
  location: { protocol: "http:", host: "localhost:5174" },
  setTimeout: () => 0,
  focus: () => {},
  matchMedia: () => ({ matches: false }),
});
vi.stubGlobal("document", {
  hidden: false,
  hasFocus: () => true,
  documentElement: { dataset: {} },
  addEventListener: () => {},
  removeEventListener: () => {},
});

const [
  { renderToStaticMarkup },
  { createElement },
  surface,
  { default: i18n },
] = await Promise.all([
  import("react-dom/server"),
  import("react"),
  import("./SkillWriteSurface"),
  import("../../i18n"),
]);
await i18n.changeLanguage("zh-CN");

const {
  EMPTY_SKILL_DRAFT,
  SkillCopyView,
  SkillDeleteView,
  SkillFormView,
  SkillWriteActionsView,
  buildSkillCreateInput,
  buildSkillUpdateInput,
  originLabelI18nKey,
  readOnlyReasonI18nKey,
  skillUpdateIsEmpty,
} = surface;

type SkillLike = Pick<SkillSummary, "id" | "name" | "writable" | "origin">;

const noop = () => {};

const userSkill: SkillLike = { id: "mine", name: "mine", writable: true, origin: "user" };
const builtinSkill: SkillLike = {
  id: "builtin",
  name: "builtin",
  writable: false,
  origin: "builtin",
};
const marketSkill: SkillLike = {
  id: "market",
  name: "market",
  writable: false,
  origin: "marketplace",
};
const legacySkill: SkillLike = { id: "old", name: "old" } as SkillLike;

function renderActions(skill: SkillLike, busy = false): string {
  return renderToStaticMarkup(
    createElement(SkillWriteActionsView, {
      skill,
      busy,
      onEdit: noop,
      onCopy: noop,
      onDelete: noop,
    }),
  );
}

describe("SkillWriteActionsView (W12 / R17)", () => {
  it("offers the three write actions for a writable user skill", () => {
    const html = renderActions(userSkill);

    expect(html).toContain("编辑");
    expect(html).toContain("复制");
    expect(html).toContain("删除");
    // No read-only note on a writable row.
    expect(html).not.toContain("只读：");
  });

  it("offers no write action on a read-only origin and says why", () => {
    const builtin = renderActions(builtinSkill);
    expect(builtin).toContain("只读：内置技能，随应用发布");
    expect(builtin).not.toContain(">编辑<");
    expect(builtin).not.toContain(">删除<");

    // The marketplace case points at the installer/market chain instead of
    // pretending the skill is editable from here.
    expect(renderActions(marketSkill)).toContain("请在应用商店里卸载");
  });

  it("treats a host that never reported the origin as read-only", () => {
    const html = renderActions(legacySkill);

    expect(html).toContain("只读：");
    expect(html).not.toContain(">删除<");
    expect(readOnlyReasonI18nKey(undefined)).toBe("skills.reasonUnknown");
  });

  it("labels every origin it knows, and none it does not", () => {
    expect(originLabelI18nKey("user")).toBe("skills.originUser");
    expect(originLabelI18nKey("marketplace")).toBe("skills.originMarketplace");
    expect(originLabelI18nKey(undefined)).toBe("skills.originUnknown");
  });

  it("disables the actions while a write is in flight", () => {
    const html = renderActions(userSkill, true);

    expect(html).toContain("disabled");
  });
});

describe("skill form inputs (W12 / R17)", () => {
  it("sends only the filled optional fields when creating", () => {
    const input = buildSkillCreateInput({
      ...EMPTY_SKILL_DRAFT,
      name: "  mine  ",
      description: " d ",
      when_to_use: "",
      allowed_tools: "Read, Grep",
      body: "body\n",
    });

    expect(input).toEqual({
      name: "mine",
      description: "d",
      allowed_tools: "Read, Grep",
      body: "body\n",
    });
    expect(Object.keys(input)).not.toContain("paths");
  });

  it("sends only the touched fields when editing, and detects an empty patch", () => {
    const baseline = { ...EMPTY_SKILL_DRAFT, name: "mine", description: "old" };

    expect(skillUpdateIsEmpty(buildSkillUpdateInput("mine", baseline, baseline))).toBe(true);

    const changed = buildSkillUpdateInput(
      "mine",
      { ...baseline, description: "new" },
      baseline,
    );
    expect(changed).toEqual({ skill_id: "mine", description: "new" });

    // Clearing an optional key is expressed as an empty string, not as absence.
    const cleared = buildSkillUpdateInput(
      "mine",
      { ...baseline, when_to_use: "" },
      { ...baseline, when_to_use: "was set" },
    );
    expect(cleared).toEqual({ skill_id: "mine", when_to_use: "" });
  });
});

describe("skill dialogs (W12 / R17)", () => {
  it("requires a name and a description before create can submit", () => {
    const empty = renderToStaticMarkup(
      createElement(SkillFormView, {
        mode: "create",
        draft: EMPTY_SKILL_DRAFT,
        busy: false,
        error: null,
        onChange: noop,
        onSubmit: noop,
        onCancel: noop,
      }),
    );
    expect(empty).toContain("disabled");

    const filled = renderToStaticMarkup(
      createElement(SkillFormView, {
        mode: "create",
        draft: { ...EMPTY_SKILL_DRAFT, name: "mine", description: "d" },
        busy: false,
        error: null,
        onChange: noop,
        onSubmit: noop,
        onCancel: noop,
      }),
    );
    expect(filled).toContain("创建");
  });

  it("locks the name when editing and says the body is replaced, not shown", () => {
    const baseline = { ...EMPTY_SKILL_DRAFT, name: "mine", description: "d" };
    const html = renderToStaticMarkup(
      createElement(SkillFormView, {
        mode: "edit",
        draft: baseline,
        baseline,
        busy: false,
        error: null,
        onChange: noop,
        onSubmit: noop,
        onCancel: noop,
      }),
    );

    expect(html).toContain("readOnly");
    expect(html).toContain("名称即 id");
    expect(html).toContain("替换正文");
    // Nothing changed → the server's own "names no field" refusal is unreachable.
    expect(html).toContain("disabled");
  });

  it("shows a write failure in place instead of a success", () => {
    const html = renderToStaticMarkup(
      createElement(SkillFormView, {
        mode: "create",
        draft: { ...EMPTY_SKILL_DRAFT, name: "mine", description: "d" },
        busy: false,
        error: "conflict: skill mine already exists (origin=builtin)",
        onChange: noop,
        onSubmit: noop,
        onCancel: noop,
      }),
    );

    expect(html).toContain("already exists");
  });

  it("asks for a new name before a copy can submit", () => {
    const html = renderToStaticMarkup(
      createElement(SkillCopyView, {
        sourceName: "builtin",
        value: "",
        busy: false,
        error: null,
        onChange: noop,
        onSubmit: noop,
        onCancel: noop,
      }),
    );

    expect(html).toContain("builtin");
    expect(html).toContain("disabled");
  });

  it("states what a delete removes, and warns about dependents", () => {
    const html = renderToStaticMarkup(
      createElement(SkillDeleteView, {
        target: { id: "mine", name: "mine", dependents: 2 },
        busy: false,
        error: null,
        onConfirm: noop,
        onCancel: noop,
      }),
    );

    expect(html).toContain("不可撤销");
    expect(html).toContain("2");
  });
});
