use super::parser::strip_code_fences;
use super::*;


// ── Per-stage output budgets ───────────────────────────────────────────────
// The learning pipeline's calls differ wildly in output size, so every stage
// passes its own `max_tokens` budget instead of a one-size-fits-all cap:
//
// | stage                     | budget | why                                            |
// |---------------------------|--------|------------------------------------------------|
// | blueprint                 |  4096  | structure-only JSON (title/concepts/modules)   |
// | activities                |  4096  | 3-5 small questions as JSON                    |
// | single activity           |  4096  | exactly one question                           |
// | figure repair             |  4096  | corrected figure code                          |
// | reflection grading        |  2048  | tiny {score, feedback} JSON                    |
// | concept graph             | 16384  | whole 60-200-concept graph in one reply; a  |
// |                           |        | regeneration round rewrites EVERYTHING again |
// | concept graph scope       |  4096  | coverage/backbone analysis before the graph  |
// | concept graph repair      |  4096  | additive 1-10 concepts only                  |
//
// All budgets are output ceilings, not targets: a budget too small truncates
// a reply mid-JSON (guaranteed-unparseable), so heavy stages get headroom;
// a budget too large risks exceeding the provider context window on the
// input side, so tiny stages stay tiny.

/// Course blueprint: structure-only JSON, medium-sized.
pub(crate) const BLUEPRINT_MAX_TOKENS: u32 = 4096;
/// Section outline JSON: tier + manifest of ≤8 sections.
pub(crate) const SECTION_OUTLINE_MAX_TOKENS: u32 = 2048;
/// One section body (400-700 chars + optional figures).
pub(crate) const SECTION_BODY_MAX_TOKENS: u32 = 4096;
/// Per-lesson activities JSON (3-5 questions).
pub(crate) const ACTIVITIES_MAX_TOKENS: u32 = 4096;
/// Single additional activity.
pub(crate) const SINGLE_ACTIVITY_MAX_TOKENS: u32 = 4096;
/// Corrected figure code only.
pub(crate) const FIGURE_REPAIR_MAX_TOKENS: u32 = 4096;
/// Reflection grading: tiny `{score, feedback}` JSON.
pub(crate) const REFLECTION_GRADING_MAX_TOKENS: u32 = 2048;
/// Pre-generation scope analysis: a coarse block checklist JSON. Generous
/// budget so a complex goal can enumerate a long, strictly-complete
/// checklist without the reply being cut off mid-array.
pub(crate) const OUTLINE_SCOPE_MAX_TOKENS: u32 = 8192;
/// Coach growth batch: ≤7 nodes with concept references + mints + notes.
/// Coach calls run with thinking ON (see [`complete_json_coach`]) and reasoning
/// tokens share this output ceiling with the JSON reply — learnhub's coach
/// turns on the same model class emit 13k+ tokens of reasoning before the
/// batch (and a truncated batch surfaces as MaxTokens → BadGateway), so the
/// budget is generous on purpose.
pub(crate) const COACH_MAX_TOKENS: u32 = 32768;
/// Concept review verdict: pass marker or ≤8 problem entries. The review runs
/// with thinking ON (see [`complete_json_coach`]) and reasoning shares this ceiling
/// with the JSON verdict — a truncated verdict parses as garbage and the
/// pipeline degrades to PASS, so give the deliberation room.
pub(crate) const CONCEPT_REVIEW_MAX_TOKENS: u32 = 16384;
/// Structure artifact generation (structure + terrain, gd.* contract): the
/// reply carries 40–80 block entries plus the terrain doors, with thinking ON
/// sharing this ceiling (see [`complete_json_coach`]) — the same 13k+
/// reasoning share applies here, and this is the pipeline's largest reply,
/// so the budget sits at 2× the coach turn.
pub(crate) const STRUCTURE_MAX_TOKENS: u32 = 65536;


/// Figure-repair stage: one model call that receives the broken figure code
/// plus the runtime error it produced and returns only corrected code. The
/// rules mirror the lesson standard so a repair slots back into the renderer
/// unchanged.
const FIGURE_REPAIR_SYSTEM: &str = r#"You fix one broken lesson figure. You receive the figure language (svg or jsxgraph),
its source code, and the error it produced at render time. Reply with ONLY the corrected
figure body — no Markdown fences, no commentary. Keep the original intent and layout,
fix the error, and follow the figure rules:
- svg: ONE self-contained <svg> element with viewBox, labels via <text>, no scripts or
  external references.
- jsxgraph: code that draws on the provided `board` variable (the `JXG` namespace is
  available too). Never call JXG.JSXGraph.initBoard. Element constructors take element
  parents, not raw numbers: board.create('line', [pointA, pointB]) needs two point
  elements or two [x, y] coordinate pairs; board.create('segment', ...) likewise; check
  every parent type the error message lists as allowed."#;


/// Repair a figure that failed to render. Returns the corrected figure body
/// with any wrapping fences stripped.
pub(crate) async fn repair_figure(
    completer: &dyn LearningCompleter,
    model_override: Option<(&nomifun_common::ProviderId, &str)>,
    language: &str,
    code: &str,
    error: &str,
) -> Result<String, String> {
    let user = format!(
        "Language: {language}\nError produced at render time:\n{error}\n\n\
         Broken figure code:\n{code}\n\nReturn the corrected figure body now."
    );
    let raw = complete(
        completer,
        model_override,
        FIGURE_REPAIR_SYSTEM,
        &user,
        FIGURE_REPAIR_MAX_TOKENS,
    )
    .await
    .map_err(|error| error.to_string())?;
    Ok(strip_code_fences(&raw))
}


/// Ceiling for a single model call during course generation. LLM endpoints
/// can stall (busy free tier, hung proxy); without a bound the job would sit
/// in `lessons` forever with no error, looking stuck to the user.
const COMPLETE_CALL_TIMEOUT_SECS: u64 = 180;

/// Coach calls deliberate with thinking ON (domain survey, prerequisite
/// descent) before drafting — the observed single attempt runs ~2-3 minutes
/// against a 32k budget, and the bound covers one transient-error retry on
/// top (the retry lives inside the completer, so it shares this ceiling).
const COACH_CALL_TIMEOUT_SECS: u64 = 480;


/// JSON-mode sibling of other json-mode stages: artifact-contract generation
/// (structure + terrain) runs the full prompt as the user message on the
/// json seam with thinking ON, under the same timeout arithmetic — reasoning
/// tokens share the ceiling with the JSON reply, and a truncated artifact
/// surfaces as a contract violation instead of a parse hang.
pub(crate) async fn complete_json_coach(
    completer: &dyn LearningCompleter,
    model_override: Option<(&nomifun_common::ProviderId, &str)>,
    system: &str,
    user: &str,
    max_tokens: u32,
    temperature: Option<f32>,
) -> Result<String, AppError> {
    let call = async {
        completer
            .complete_json_with_thinking(
                model_override.map(|(id, model)| (id.as_str(), model)),
                system,
                user,
                max_tokens,
                temperature,
            )
            .await
    };
    tokio::time::timeout(std::time::Duration::from_secs(COACH_CALL_TIMEOUT_SECS), call)
        .await
        .map_err(|_| {
            tracing::warn!(
                timeout_secs = COACH_CALL_TIMEOUT_SECS,
                "json coach call timed out"
            );
            AppError::Timeout(format!(
                "json coach call exceeded {COACH_CALL_TIMEOUT_SECS}s"
            ))
        })?
}


pub(crate) async fn complete(
    completer: &dyn LearningCompleter,
    model_override: Option<(&nomifun_common::ProviderId, &str)>,
    system: &str,
    user: &str,
    max_tokens: u32,
) -> Result<String, AppError> {
    complete_with_timeout(
        completer,
        model_override,
        system,
        user,
        max_tokens,
        std::time::Duration::from_secs(COMPLETE_CALL_TIMEOUT_SECS),
    )
    .await
}


/// [`complete`] with an explicit timeout so callers with heavier outputs
/// (e.g. a whole concept graph in one reply) can request a longer bound
/// than the course-generation ceiling.
pub(crate) async fn complete_with_timeout(
    completer: &dyn LearningCompleter,
    model_override: Option<(&nomifun_common::ProviderId, &str)>,
    system: &str,
    user: &str,
    max_tokens: u32,
    timeout: std::time::Duration,
) -> Result<String, AppError> {
    let call = async {
        completer
            .complete(model_override.map(|(id, model)| (id.as_str(), model)), system, user, max_tokens)
            .await
    };
    tokio::time::timeout(timeout, call)
        .await
        .map_err(|_| {
            tracing::warn!(
                timeout_secs = timeout.as_secs(),
                model = ?model_override.map(|(id, model)| (id.as_str(), model)),
                "model call timed out (course generation or concept graph)"
            );
            AppError::Timeout(format!(
                "model call exceeded {}s (course generation or concept graph)",
                timeout.as_secs()
            ))
        })?
}

