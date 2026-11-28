use super::shell_env_allowed;

#[test]
fn proxy_environment_requires_network_and_a_credential_free_endpoint() {
    for (name, value) in [
        ("HTTPS_PROXY", "http://127.0.0.1:8080"),
        ("http_proxy", "proxy.internal:3128"),
        ("ALL_PROXY", "socks5h://[::1]:1080"),
        ("NO_PROXY", "localhost,.internal,10.0.0.0/8"),
    ] {
        assert!(shell_env_allowed(name, value, true), "{name} fixture");
        assert!(!shell_env_allowed(name, value, false));
    }
    for value in [
        "http://user:password@proxy.internal:8080",
        "proxy.internal:3128?token=secret",
        "https://proxy.internal/private-token",
        "https://proxy.internal/private-token/../",
        "file:///etc/hosts",
        "https://proxy.internal/#token",
        "not a proxy",
    ] {
        assert!(!shell_env_allowed("HTTPS_PROXY", value, true));
    }
    assert!(!shell_env_allowed("FLOWY_LLM_API_KEY", "fixture", true));
    assert!(!shell_env_allowed("no_proxy", "u:p@proxy.internal", true));
}
