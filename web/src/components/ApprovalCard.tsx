/**
 * W2 approval card (R8 · D3=B).
 *
 * Shown above the composer while the followed Run has an unanswered
 * `approval.requested`. The card is deliberately the *only* place the WebUI
 * answers a decision, and it answers through `run/answer-decision` with the CAS
 * tokens projected on the event — there is no approve-all / always-allow switch
 * anywhere in this surface (the desktop confirmation route's flag is not part of
 * the App Server protocol).
 */

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useAppStore } from "../store/appStore";
import { pendingDecision } from "../lib/approvals";
import { shortId } from "../ui/format";

export function ApprovalCard() {
  const { t } = useTranslation();
  const activeRunId = useAppStore((s) => s.activeRunId);
  const runEvents = useAppStore((s) => s.runEvents);
  const busy = useAppStore((s) => s.runDecisionBusy);
  const error = useAppStore((s) => s.runDecisionError);
  const answerDecision = useAppStore((s) => s.answerRunDecision);
  const [draft, setDraft] = useState("");

  const state = pendingDecision(runEvents);
  const decision = state.status === "pending" ? state.decision : null;
  const unanswerable = state.status === "unanswerable" ? state.reason : null;
  // One decision, one answer: a new request never inherits the previous text.
  const decisionKey = decision ? `${decision.stepId}:${decision.attemptId}:${decision.event.sequence}` : null;

  useEffect(() => {
    setDraft("");
  }, [decisionKey]);

  if (!activeRunId || (!decision && !unanswerable)) {
    return null;
  }

  const submit = () => {
    const text = draft.trim();
    if (!text || busy) return;
    void answerDecision(text);
  };

  return (
    <section className="approval-card" aria-live="polite">
      <header className="approval-card-head">
        <span className="approval-card-badge">{t("run.approvalBadge")}</span>
        <span className="approval-card-scope">
          {t("run.approvalScope", { run: shortId(activeRunId) })}
          {decision
            ? ` · ${t("run.approvalScopeStep", {
                step: shortId(decision.stepId),
                attempt: shortId(decision.attemptId),
              })}`
            : ""}
        </span>
      </header>
      {decision ? (
        <>
          <p className="approval-card-question">{decision.question}</p>
          <textarea
            className="approval-card-input"
            value={draft}
            onChange={(event) => setDraft(event.target.value)}
            placeholder={t("run.approvalPlaceholder")}
            rows={2}
            disabled={busy}
            onKeyDown={(event) => {
              if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
                event.preventDefault();
                submit();
              }
            }}
          />
          <div className="approval-card-actions">
            <button onClick={submit} disabled={busy || draft.trim().length === 0}>
              {busy ? t("run.approvalAnswering") : t("run.approvalAnswer")}
            </button>
            <span className="approval-card-hint">{t("run.approvalHint")}</span>
          </div>
        </>
      ) : (
        <p className="approval-card-question muted">{unanswerable}</p>
      )}
      {error && <p className="approval-card-error">{error}</p>}
    </section>
  );
}
