import { describe, expect, it, vi } from "vitest";

import type { ConnectorCredential } from "../lib/protocol";

/**
 * Render tests for the schema-driven credential form (doc `34` §6.3).
 *
 * The form is a pure function of the host's `credential.fields`, so a server
 * render pins the whole contract without a client, a socket or an effect:
 *
 * - a `secret` row is masked and **starts empty** — the host never sends a secret
 *   back, and echoing one would put it in the DOM of a page that did not need it;
 * - a `plain` row is the connector's own setting and **is** prefilled, which is
 *   the only reason it is on the wire at all (`34` §5.3);
 * - connector-authored copy arrives in both languages and resolves by UI
 *   language, falling back to the other language and then to the wire key;
 * - the chrome (buttons, chips, the "how do I get a key" fallback) is ours and
 *   goes through i18n.
 */

vi.stubGlobal("localStorage", { getItem: () => null, setItem: () => {}, removeItem: () => {} });
vi.stubGlobal("navigator", { language: "zh-CN" });
vi.stubGlobal("window", {
  location: { protocol: "http:", host: "localhost:5174" },
  setTimeout: () => 0,
  focus: () => {},
  matchMedia: () => ({ matches: false }),
});
vi.stubGlobal("document", { hidden: false, hasFocus: () => true, documentElement: { dataset: {} } });

const [
  { renderToStaticMarkup },
  { createElement },
  { ConnectorCredentialForm, hasChanges, submittedValues },
  { default: i18n },
] = await Promise.all([
  import("react-dom/server"),
  import("react"),
  import("./catalog/ConnectorCredentialForm"),
  import("../i18n"),
]);

/**
 * `tdengine`, the market's mixed case (`34` §5.2): three plain selectors with
 * defaults and one secret, with the URL template built out of all four.
 */
const TDENGINE: ConnectorCredential = {
  connector_id: "tdengine",
  mode: "token",
  status: "requires_input",
  missing: ["TDENGINE_API_KEY"],
  title: { zh: "TDengine 配置", en: "TDengine configuration" },
  description: { zh: "填入连接 TDengine 所需的参数。", en: "Parameters for connecting to TDengine." },
  // The schema declares its "where do I get a key" page once, for the form
  // (`34` §5.2) — never per field.
  doc_url: { zh: "https://docs.example.com/tdengine", en: "https://docs.example.com/tdengine/en" },
  // `docLabel_en` is missing on 9 of the market's 55 documented schemas, so the
  // fallback chain has to end at our own chrome.
  doc_label: { zh: "", en: "" },
  fields: [
    {
      key: "TDENGINE_API_SCHEMA",
      kind: "plain",
      required: false,
      label: { zh: "协议", en: "Scheme" },
      placeholder: { zh: "", en: "" },
      description: { zh: "http 或 https", en: "" },
      value: "http",
    },
    {
      key: "TDENGINE_API_HOST",
      kind: "plain",
      required: false,
      label: { zh: "主机", en: "Host" },
      placeholder: { zh: "", en: "" },
      description: { zh: "", en: "" },
      value: "localhost",
    },
    {
      key: "TDENGINE_API_PORT",
      kind: "plain",
      required: false,
      label: { zh: "端口", en: "Port" },
      placeholder: { zh: "", en: "" },
      description: { zh: "", en: "" },
      value: "6042",
    },
    {
      key: "TDENGINE_API_KEY",
      kind: "secret",
      required: true,
      // Only the Chinese slot is filled: the English reader must fall back to it
      // rather than see an empty label.
      label: { zh: "密钥", en: "" },
      placeholder: { zh: "粘贴密钥", en: "" },
      description: { zh: "在控制台创建后复制。", en: "" },
    },
  ],
};

function render(credential: ConnectorCredential): string {
  return renderToStaticMarkup(
    createElement(ConnectorCredentialForm, {
      credential,
      busy: false,
      onSave: () => {},
      onClear: () => {},
    }),
  );
}

const inputTags = (html: string): string[] => html.match(/<input[^>]*>/g) ?? [];

describe("ConnectorCredentialForm", () => {
  it("prefills a plain field and never a secret one", () => {
    const html = render(TDENGINE);
    const tags = inputTags(html);
    expect(tags).toHaveLength(4);

    // The connector's own settings carry the value in effect.
    expect(tags[0]).toContain('value="http"');
    expect(tags[1]).toContain('value="localhost"');
    expect(tags[2]).toContain('value="6042"');

    // The secret does not, and its box is masked.
    const secretBox = tags.find((tag) => tag.includes('type="password"'));
    expect(secretBox).toBeDefined();
    expect(secretBox).toContain('value=""');
    expect(secretBox).toContain('placeholder="粘贴密钥"');
  });

  it("carries the connector's own copy, in the reader's language", async () => {
    await i18n.changeLanguage("zh-CN");
    const zh = render(TDENGINE);
    expect(zh).toContain("TDengine 配置");
    expect(zh).toContain("填入连接 TDengine 所需的参数。");
    // `label.en` is empty for the secret, so the Chinese slot is used.
    expect(zh).toContain("密钥");
    // `docLabel` is empty in both slots → our own chrome, never a raw key.
    expect(zh).toContain("如何获取密钥");

    await i18n.changeLanguage("en-US");
    const en = render(TDENGINE);
    expect(en).toContain("TDengine configuration");
    expect(en).toContain("Scheme");
    expect(en).toContain("How to get a key");
    // The scheme slot is the one that differs per language.
    expect(en).toContain("https://docs.example.com/tdengine/en");
    // The secret's label has no English slot: fall back, do not render nothing.
    expect(en).toContain("密钥");

    await i18n.changeLanguage("zh-CN");
  });

  it("puts the 'where do I get a key' link once, for the form", () => {
    const html = render(TDENGINE);
    // One link, not one per field: the market declares it on the schema, so a
    // link under 「端口」 would be an invention (`34` §5.2).
    expect(html.match(/class="credential-form-doc"/g) ?? []).toHaveLength(1);
    expect(html).toContain("https://docs.example.com/tdengine");
    // …and it sits with the form's own text, before the first field.
    expect(html.indexOf("credential-form-doc")).toBeLessThan(html.indexOf("credential-field"));
  });

  it("marks which rows the host reports as settled and which are still missing", () => {
    const html = render(TDENGINE);

    // A plain field with a value in effect, and the one required secret the host
    // says it does not hold. The **count** lives on the drawer's badge, not here:
    // a third statement of it, directly under that badge, is noise.
    expect(html).toContain("已保存");
    expect(html).toContain("待填写");
    expect(html).toContain("必填");
    expect(html).not.toContain("缺 1 项");
    // The wire key stays on screen: it is what the connector's template and the
    // host log name.
    expect(html).toContain("TDENGINE_API_KEY");
  });

  it("offers to forget only what the host says it holds", () => {
    // Nothing required is stored, so there is nothing to forget — and a control
    // that would do nothing when pressed is exactly what this avoids.
    expect(render(TDENGINE)).not.toContain("清除凭据");

    const stored: ConnectorCredential = { ...TDENGINE, status: "configured", missing: [] };
    expect(render(stored)).toContain("清除凭据");
    // …and the row the host now holds reads as settled.
    expect(render(stored)).not.toContain("待填写");
  });

  it("offers nothing to save until something the host does not hold is typed", () => {
    // The prefilled `plain` rows are the value in effect, and the secret box is
    // empty: pressing save here would write back exactly what is already stored.
    const saveButton = (html: string) =>
      (html.match(/<button[^>]*class="primary-button"[^>]*>/) ?? [""])[0];
    expect(saveButton(render(TDENGINE))).toContain("disabled");

    // An empty form (a connector whose settings have no default) likewise.
    const empty: ConnectorCredential = {
      ...TDENGINE,
      fields: TDENGINE.fields.map((field) => ({ ...field, value: field.kind === "plain" ? "" : undefined })),
    };
    expect(saveButton(render(empty))).toContain("disabled");
    expect(inputTags(render(empty)).every((tag) => tag.includes('value=""'))).toBe(true);
  });
});

describe("submittedValues", () => {
  it("drops the empty boxes instead of overwriting stored secrets with nothing", () => {
    // This is the whole reason the form tracks blanks: a secret row is empty
    // unless the user retypes it, so sending it back would erase what is stored.
    expect(
      submittedValues({ TDENGINE_API_KEY: "", TDENGINE_API_HOST: "localhost", TDENGINE_API_PORT: "  " }),
    ).toEqual({ TDENGINE_API_HOST: "localhost" });
  });

  it("keeps a value that merely starts or ends with a space", () => {
    expect(submittedValues({ TOKEN: " abc " })).toEqual({ TOKEN: " abc " });
  });

  it("returns an empty body when nothing was typed", () => {
    expect(submittedValues({ A: "", B: "   " })).toEqual({});
  });
});

describe("hasChanges", () => {
  // Static markup cannot type into a box, so the rule the save button is gated
  // on is asserted where it lives rather than through the DOM.
  it("sees no change when every box still holds what the host sent", () => {
    expect(hasChanges(TDENGINE.fields, {})).toBe(false);
    expect(hasChanges(TDENGINE.fields, { TDENGINE_API_HOST: "localhost" })).toBe(false);
  });

  it("sees a change when a plain setting is edited", () => {
    expect(hasChanges(TDENGINE.fields, { TDENGINE_API_HOST: "db.internal" })).toBe(true);
  });

  it("treats anything typed into a secret box as new", () => {
    // There is nothing to compare against: the host never sends a secret back.
    expect(hasChanges(TDENGINE.fields, { TDENGINE_API_KEY: "k" })).toBe(true);
  });
});
