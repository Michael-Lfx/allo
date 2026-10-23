/// How a product route relates to a paired phone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkClass {
    /// Paired phone may invoke it. The effect happens in the PC process.
    LinkReady,
    /// Phone may request it. The desktop must confirm before it runs.
    StepUp,
    /// No owner session. Health pages and the one-time browser QR exchange.
    PublicAsset,
    /// Companion-token, MCP, or OAuth front door. Not a Link session.
    ExternalChannel,
    /// Physical robot channel.
    RobotDevice,
}

/// Longest-prefix classification. `None` means the route is not on the product surface yet.
pub fn classify_route(path: &str) -> Option<LinkClass> {
    let path = path.split('?').next().unwrap_or(path);
    RULES
        .iter()
        .filter(|(prefix, _)| path == *prefix || path.starts_with((*prefix).trim_end_matches('/')) && boundary(path, prefix))
        .max_by_key(|(prefix, _)| prefix.len())
        .map(|(_, class)| *class)
}

fn boundary(path: &str, prefix: &str) -> bool {
    path == prefix || path.starts_with(&format!("{prefix}/"))
}

const RULES: &[(&str, LinkClass)] = &[
    ("/api/webui/companions", LinkClass::StepUp),
    ("/api/webui/change-password", LinkClass::StepUp),
    ("/api/webui/change-username", LinkClass::StepUp),
    ("/api/webui/reset-password", LinkClass::StepUp),
    ("/api/webui/generate-qr-token", LinkClass::StepUp),
    ("/api/terminals/register-knowledge-global", LinkClass::StepUp),
    ("/api/terminals/unregister-knowledge-global", LinkClass::StepUp),
    ("/api/terminals/register-knowledge", LinkClass::StepUp),
    ("/api/auth/qr-login", LinkClass::PublicAsset),
    ("/api/auth/setup", LinkClass::PublicAsset),
    ("/health", LinkClass::PublicAsset),
    ("/login", LinkClass::PublicAsset),
    ("/logout", LinkClass::PublicAsset),
    ("/qr-login", LinkClass::PublicAsset),
    ("/mcp", LinkClass::ExternalChannel),
    ("/v1", LinkClass::ExternalChannel),
    ("/oauth", LinkClass::ExternalChannel),
    ("/.well-known", LinkClass::ExternalChannel),
    ("/robot", LinkClass::RobotDevice),
    ("/robot-mcp", LinkClass::RobotDevice),
    ("/ota", LinkClass::RobotDevice),
    ("/ws", LinkClass::LinkReady),
    ("/api/agent-execution-templates", LinkClass::LinkReady),
    ("/api/agent-executions", LinkClass::LinkReady),
    ("/api/agent", LinkClass::LinkReady),
    ("/api/agents", LinkClass::LinkReady),
    ("/api/app-server", LinkClass::LinkReady),
    ("/api/appearance", LinkClass::LinkReady),
    ("/api/assets", LinkClass::LinkReady),
    ("/api/auth", LinkClass::LinkReady),
    ("/api/bedrock", LinkClass::LinkReady),
    ("/api/briefing", LinkClass::LinkReady),
    ("/api/browser-secrets", LinkClass::LinkReady),
    ("/api/browser", LinkClass::LinkReady),
    ("/api/channel", LinkClass::LinkReady),
    ("/api/cloud", LinkClass::LinkReady),
    ("/api/companion", LinkClass::LinkReady),
    ("/api/computer", LinkClass::LinkReady),
    ("/api/conversations", LinkClass::LinkReady),
    ("/api/creation", LinkClass::LinkReady),
    ("/api/cron", LinkClass::LinkReady),
    ("/api/customer-service", LinkClass::LinkReady),
    ("/api/debug", LinkClass::LinkReady),
    ("/api/excel-preview", LinkClass::LinkReady),
    ("/api/extensions", LinkClass::LinkReady),
    ("/api/fs", LinkClass::LinkReady),
    ("/api/hub", LinkClass::LinkReady),
    ("/api/idmm", LinkClass::LinkReady),
    ("/api/insights", LinkClass::LinkReady),
    ("/api/knowledge", LinkClass::LinkReady),
    ("/api/learning", LinkClass::LinkReady),
    ("/api/link", LinkClass::LinkReady),
    ("/api/mcp", LinkClass::LinkReady),
    ("/api/media", LinkClass::LinkReady),
    ("/api/meetings", LinkClass::LinkReady),
    ("/api/messages", LinkClass::LinkReady),
    ("/api/model-profiles", LinkClass::LinkReady),
    ("/api/model-services", LinkClass::LinkReady),
    ("/api/office-watch-proxy", LinkClass::LinkReady),
    ("/api/poi", LinkClass::LinkReady),
    ("/api/ppt-preview", LinkClass::LinkReady),
    ("/api/ppt-proxy", LinkClass::LinkReady),
    ("/api/preset-tags", LinkClass::LinkReady),
    ("/api/presets", LinkClass::LinkReady),
    ("/api/preview-history", LinkClass::LinkReady),
    ("/api/provider-models", LinkClass::LinkReady),
    ("/api/providers", LinkClass::LinkReady),
    ("/api/remote-agents", LinkClass::LinkReady),
    ("/api/requirements", LinkClass::LinkReady),
    ("/api/robots", LinkClass::LinkReady),
    ("/api/settings", LinkClass::LinkReady),
    ("/api/shell", LinkClass::LinkReady),
    ("/api/skills", LinkClass::LinkReady),
    ("/api/ssh-hosts", LinkClass::LinkReady),
    ("/api/star-office", LinkClass::LinkReady),
    ("/api/stt", LinkClass::LinkReady),
    ("/api/system", LinkClass::LinkReady),
    ("/api/tags", LinkClass::LinkReady),
    ("/api/terminals", LinkClass::LinkReady),
    ("/api/tts", LinkClass::LinkReady),
    ("/api/video-canvas", LinkClass::LinkReady),
    ("/api/vimax", LinkClass::LinkReady),
    ("/api/webhooks", LinkClass::LinkReady),
    ("/api/webui", LinkClass::LinkReady),
    ("/api/word-preview", LinkClass::LinkReady),
    ("/api/workshop", LinkClass::LinkReady),
    ("/api/ws-token", LinkClass::LinkReady),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_up_does_not_swallow_the_shorter_sibling() {
        assert_eq!(
            classify_route("/api/terminals/register-knowledge"),
            Some(LinkClass::StepUp)
        );
        assert_eq!(
            classify_route("/api/terminals/list"),
            Some(LinkClass::LinkReady)
        );
        assert_eq!(
            classify_route("/api/terminals/register-knowledge-global"),
            Some(LinkClass::StepUp)
        );
    }

    #[test]
    fn product_routes_are_classified() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../backend");
        let mut missing = Vec::new();
        for entry in walk(&root) {
            let text = std::fs::read_to_string(&entry).unwrap_or_default();
            for path in route_paths(&text) {
                if is_fixture(&path) || !is_product_path(&path) {
                    continue;
                }
                if classify_route(&path).is_none() {
                    missing.push(format!("{} ({path})", entry.display()));
                }
            }
        }
        assert!(
            missing.is_empty(),
            "unclassified product routes:\n{}",
            missing.join("\n")
        );
    }

    fn is_product_path(path: &str) -> bool {
        path.starts_with("/api/")
            || matches!(
                path,
                "/health" | "/login" | "/logout" | "/qr-login" | "/ws" | "/mcp" | "/v1"
            )
            || path.starts_with("/oauth")
            || path.starts_with("/.well-known")
            || path.starts_with("/robot")
            || path.starts_with("/ota")
            || path.starts_with("/v1/")
            || path.starts_with("/mcp/")
    }

    fn is_fixture(path: &str) -> bool {
        matches!(
            path,
            "/api/thing"
                | "/api/test"
                | "/api/protected"
                | "/api/{prefix}"
                | "/api/{prefix}/{capability}/{*path}"
                | "/__test__/ingress-gate"
        )
    }

    fn route_paths(text: &str) -> Vec<String> {
        let mut paths = Vec::new();
        let marker = ".route(";
        let mut rest = text;
        while let Some(index) = rest.find(marker) {
            rest = &rest[index + marker.len()..];
            let trimmed = rest.trim_start();
            if let Some(path) = trimmed.strip_prefix('"') {
                if let Some(end) = path.find('"') {
                    paths.push(path[..end].to_string());
                }
            }
        }
        paths
    }

    fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut files = Vec::new();
        let Ok(read) = std::fs::read_dir(dir) else {
            return files;
        };
        for entry in read.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.ends_with("tests") {
                    continue;
                }
                files.extend(walk(&path));
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
                files.push(path);
            }
        }
        files
    }
}
