//! Host allow-list for OTA redirects (product policy, not transport).

const ALLOWED: &[&str] = &[
    "modelscope.cn",
    "www.modelscope.cn",
    "modelscope.ai",
    "www.modelscope.ai",
    "github.com",
    "www.github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
    "githubusercontent.com",
];

pub fn host_allowed(host: &str) -> bool {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    ALLOWED.iter().any(|allowed| {
        host == *allowed || host.ends_with(&format!(".{allowed}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_github_asset_hosts() {
        assert!(host_allowed("objects.githubusercontent.com"));
        assert!(host_allowed("github.com"));
        assert!(!host_allowed("evil.example"));
    }
}
