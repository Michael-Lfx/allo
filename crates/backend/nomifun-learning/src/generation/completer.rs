use super::parser::strip_code_fences;
use super::*;


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
    completer: &dyn KnowledgeCompleter,
    model_override: Option<(&nomifun_common::ProviderId, &str)>,
    language: &str,
    code: &str,
    error: &str,
) -> Result<String, String> {
    let user = format!(
        "Language: {language}\nError produced at render time:\n{error}\n\n\
         Broken figure code:\n{code}\n\nReturn the corrected figure body now."
    );
    let raw = complete(completer, model_override, FIGURE_REPAIR_SYSTEM, &user)
        .await
        .map_err(|error| error.to_string())?;
    Ok(strip_code_fences(&raw))
}


/// Ceiling for a single model call during course generation. LLM endpoints
/// can stall (busy free tier, hung proxy); without a bound the job would sit
/// in `lessons` forever with no error, looking stuck to the user.
const COMPLETE_CALL_TIMEOUT_SECS: u64 = 180;


pub(crate) async fn complete(
    completer: &dyn KnowledgeCompleter,
    model_override: Option<(&nomifun_common::ProviderId, &str)>,
    system: &str,
    user: &str,
) -> Result<String, AppError> {
    complete_with_timeout(
        completer,
        model_override,
        system,
        user,
        std::time::Duration::from_secs(COMPLETE_CALL_TIMEOUT_SECS),
    )
    .await
}


/// [`complete`] with an explicit timeout so tests can bound a hung call.
pub(super) async fn complete_with_timeout(
    completer: &dyn KnowledgeCompleter,
    model_override: Option<(&nomifun_common::ProviderId, &str)>,
    system: &str,
    user: &str,
    timeout: std::time::Duration,
) -> Result<String, AppError> {
    let call = async {
        match model_override {
            Some((provider_id, model)) => {
                completer
                    .complete_with(system, user, provider_id.as_str(), model)
                    .await
            }
            None => completer.complete(system, user).await,
        }
    };
    tokio::time::timeout(timeout, call)
        .await
        .map_err(|_| {
            AppError::Timeout(format!(
                "model call exceeded {}s during course generation",
                timeout.as_secs()
            ))
        })?
}

