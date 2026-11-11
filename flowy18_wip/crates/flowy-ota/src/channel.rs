//! Release channel endpoints and updater public key.

#[derive(Debug, Clone)]
pub struct ChannelConfig {
    pub github_repo: &'static str,
    pub modelscope_cn_repo: &'static str,
    pub modelscope_ai_repo: &'static str,
    pub prefix: &'static str,
    /// Base64-encoded minisign public key (Tauri updater format).
    pub pubkey: &'static str,
    pub current_version: &'static str,
}

impl ChannelConfig {
    /// Allo production mirrors already serving desktop OTA.
    pub const fn allo(current_version: &'static str) -> Self {
        Self {
            github_repo: "Michael-Lfx/allo",
            modelscope_cn_repo: "flowy2025/flowyaipc",
            modelscope_ai_repo: "flowy2025/flowy",
            prefix: "allo",
            pubkey: "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDZGRDA3NTMzQzQxODdCNjQKUldSa2V4akVNM1hRYjlrRmZLalZRdGpaM0FzeGxVd0s3VkdoY0VMY01lY2UrQ2dMdkVMTWF6SFUK",
            current_version,
        }
    }

    /// Build from compile-time brand injection (`FLOWY_OTA_*`).
    pub const fn from_brand(
        github_repo: &'static str,
        modelscope_cn_repo: &'static str,
        modelscope_ai_repo: &'static str,
        prefix: &'static str,
        pubkey: &'static str,
        current_version: &'static str,
    ) -> Self {
        Self {
            github_repo,
            modelscope_cn_repo,
            modelscope_ai_repo,
            prefix,
            pubkey,
            current_version,
        }
    }
}
