/**
 * The one credential form (doc `34` §6.3) — what a marketplace connector asks
 * the user to fill in, rendered from the host's own `credential.fields`.
 *
 * Everything is server-driven: labels, placeholders, descriptions and the
 * marketplace's own "where do I get a key" link arrive in both languages and are
 * resolved here, so connector-authored copy (including a market author's own
 * security note) ships with the schema while this component's chrome goes
 * through i18n. Nothing here invents a field, decides which ones are required,
 * or knows what a key is called.
 *
 * Two rules it must not lose:
 *
 * - a `secret` row is masked and starts **empty**: the host never sends a secret
 *   back, so there is nothing to prefill, and an empty box is also how "leave
 *   the stored value alone" is expressed on save;
 * - a `plain` row is the connector's own setting (`HOST`, `PORT`) and **does**
 *   arrive with its value, because prefilling it is the whole point (`34` §5.3).
 */

import { useState } from "react";
import { ExternalLink } from "lucide-react";
import { useTranslation } from "react-i18next";

import { pickLocalized, useLocalizedLang } from "../../ui/localize";
import type { ConnectorCredential, CredentialField } from "../../lib/protocol";

/**
 * What a save actually sends: the boxes with something in them.
 *
 * Empty is never a value, it is the absence of one — the same rule the importer
 * applies to an empty `env` value (`34` §5.4 ②). Without it, re-saving the form
 * to change one field would overwrite every stored secret with `""`, because a
 * secret row is empty unless the user retypes it.
 */
export function submittedValues(values: Record<string, string>): Record<string, string> {
  return Object.fromEntries(
    Object.entries(values).filter(([, value]) => value.trim() !== ""),
  );
}

/**
 * Whether a body says anything the host does not already hold.
 *
 * A `plain` box is prefilled, so it has to be compared; a `secret` box is empty
 * unless the user typed in it, so whatever it contributes to the body is new by
 * construction — and what is stored is deliberately unknowable here, which is why
 * `credential/set` cannot be turned into a "did it change?" question.
 */
export function hasChanges(
  fields: CredentialField[],
  body: Record<string, string>,
): boolean {
  return Object.entries(body).some(([key, value]) => {
    const field = fields.find((candidate) => candidate.key === key);
    return field?.kind === "secret" || value !== (field?.value ?? "");
  });
}

export function ConnectorCredentialForm({
  credential,
  busy,
  onSave,
  onClear,
}: {
  credential: ConnectorCredential;
  busy: boolean;
  onSave: (values: Record<string, string>) => void;
  onClear: () => void;
}) {
  const { t } = useTranslation();
  const lang = useLocalizedLang();
  const [values, setValues] = useState<Record<string, string>>(() => {
    const seeded: Record<string, string> = {};
    for (const field of credential.fields) {
      seeded[field.key] = field.kind === "plain" ? (field.value ?? "") : "";
    }
    return seeded;
  });

  const title = pickLocalized(credential.title, lang);
  const description = pickLocalized(credential.description, lang);
  const docUrl = pickLocalized(credential.doc_url, lang);
  // `doc_label` is missing on 9 of the market's 55 documented schemas (`34`
  // §5.2), so the fallback chain ends at our own chrome rather than an empty link.
  const docLabel = pickLocalized(credential.doc_label, lang) || t("catalog.credentialGetKey");
  const body = submittedValues(values);

  /**
   * The save button is offered only when the body says something new: a form
   * whose every box already matches the host would otherwise carry a control
   * that writes back what is already there.
   */
  const changed = hasChanges(credential.fields, body);

  /**
   * Whether the host reports this row as already in place.
   *
   * `missing` only tracks **required** keys, so an optional secret gets no
   * verdict rather than a guess — it renders without a state chip. (Only 4 of
   * the market's fields are optional, `34` §5.2.)
   */
  const satisfied = (field: CredentialField): boolean | null =>
    field.kind === "secret"
      ? field.required
        ? !credential.missing.includes(field.key)
        : null
      : Boolean(field.value);

  /** Offer to forget only what the host says it holds. */
  const clearable = credential.fields.some(
    (field) => field.kind === "secret" && field.required && !credential.missing.includes(field.key),
  );

  return (
    <div className="credential-form">
      {title && <h3 className="credential-form-title">{title}</h3>}
      {description && <p className="credential-form-desc">{description}</p>}
      {/* The marketplace's own "where do I get a key" page, once for the form
          (`34` §5.2/§6.3). It belongs to the schema, not to a field: the same
          link under 「端口」 is how this read before the projection stopped
          copying it onto every row. */}
      {docUrl && (
        <a className="credential-form-doc" href={docUrl} target="_blank" rel="noreferrer">
          <ExternalLink size={12} strokeWidth={1.8} />
          <span>{docLabel}</span>
        </a>
      )}
      {credential.missing.length > 0 && (
        <p className="credential-form-missing">
          {t("catalog.credentialMissingCount", { count: credential.missing.length })}
        </p>
      )}

      {credential.fields.map((field) => {
        const label = pickLocalized(field.label, lang) || field.key;
        const placeholder = pickLocalized(field.placeholder, lang);
        const hint = pickLocalized(field.description, lang);
        const state = satisfied(field);
        return (
          <div className="credential-field" key={field.key}>
            <div className="credential-field-head">
              <span className="credential-field-label">{label}</span>
              {field.required && (
                <span className="market-tag is-muted">{t("catalog.credentialRequired")}</span>
              )}
              {state !== null && (
                <span className={`market-tag is-status ${state ? "is-success" : "is-warn"}`}>
                  {state ? t("catalog.credentialFieldSet") : t("catalog.credentialFieldUnset")}
                </span>
              )}
            </div>
            {/* The wire key stays visible: it is what the connector's own template
                names and what the host log would name, and a mismatch between the
                label and the key is otherwise invisible. */}
            <code className="credential-field-key">{field.key}</code>
            <input
              className="credential-input"
              type={field.kind === "secret" ? "password" : "text"}
              value={values[field.key] ?? ""}
              placeholder={placeholder || undefined}
              // A manager must not offer to fill the box with a saved login for a
              // service it has never heard of; `new-password` is the browser's own
              // way of saying "this is not a login to remember".
              autoComplete={field.kind === "secret" ? "new-password" : "off"}
              spellCheck={false}
              disabled={busy}
              onChange={(event) =>
                setValues((current) => ({ ...current, [field.key]: event.target.value }))
              }
            />
            {hint && <p className="credential-field-desc">{hint}</p>}
          </div>
        );
      })}

      <div className="credential-form-actions">
        <button
          className="primary-button"
          type="button"
          disabled={busy || !changed}
          onClick={() => onSave(body)}
        >
          {busy ? t("catalog.credentialSaving") : t("catalog.credentialSave")}
        </button>
        {clearable && (
          <button className="quiet-button is-destructive" type="button" disabled={busy} onClick={onClear}>
            {t("catalog.credentialClear")}
          </button>
        )}
      </div>
    </div>
  );
}
