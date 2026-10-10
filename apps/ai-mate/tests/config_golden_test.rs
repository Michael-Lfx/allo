use std::path::PathBuf;
use ai_mate::config::{AiMateConfig, Config, PreferMirror, ENV_AI_MATE_MODEL_DIR};

#[test]
fn test_default_config_golden() {
    let toml_str = include_str!("fixtures/default.toml");
    let config = Config::from_toml_str(toml_str).expect("default.toml must parse successfully");

    assert_eq!(
        config.models.models_dir,
        PathBuf::from("/var/lib/ai-mate/models")
    );
    assert_eq!(config.models.auto_download, true);
    assert_eq!(config.models.probe_timeout_ms, 1500);
    assert_eq!(config.models.prefer_mirror, PreferMirror::Auto);
}

#[test]
fn test_custom_config_golden() {
    let toml_str = include_str!("fixtures/custom.toml");
    let config = Config::from_toml_str(toml_str).expect("custom.toml must parse successfully");

    assert_eq!(
        config.models.models_dir,
        PathBuf::from("/custom/storage/models")
    );
    assert_eq!(config.models.auto_download, false);
    assert_eq!(config.models.probe_timeout_ms, 3000);
    assert_eq!(config.models.prefer_mirror, PreferMirror::ModelScope);
}

#[test]
fn test_github_mirror_golden() {
    let toml_str = include_str!("fixtures/github_mirror.toml");
    let config = Config::from_toml_str(toml_str).expect("github_mirror.toml must parse successfully");

    assert_eq!(
        config.models.models_dir,
        PathBuf::from("/opt/ai-mate/models")
    );
    assert_eq!(config.models.auto_download, true);
    assert_eq!(config.models.probe_timeout_ms, 2000);
    assert_eq!(config.models.prefer_mirror, PreferMirror::GitHub);
}

#[test]
fn test_empty_models_section_defaults() {
    let toml_str = include_str!("fixtures/empty_models.toml");
    let config = Config::from_toml_str(toml_str).expect("empty_models.toml must parse successfully");

    assert_eq!(
        config.models.models_dir,
        PathBuf::from("/var/lib/ai-mate/models")
    );
    assert_eq!(config.models.auto_download, true);
    assert_eq!(config.models.probe_timeout_ms, 1500);
    assert_eq!(config.models.prefer_mirror, PreferMirror::Auto);
}

#[test]
fn test_empty_toml_defaults() {
    let config = Config::from_toml_str("").expect("empty toml must parse successfully");
    let default_config = AiMateConfig::default();

    assert_eq!(config, default_config);
    assert_eq!(
        config.models.models_dir,
        PathBuf::from("/var/lib/ai-mate/models")
    );
    assert_eq!(config.models.auto_download, true);
    assert_eq!(config.models.probe_timeout_ms, 1500);
    assert_eq!(config.models.prefer_mirror, PreferMirror::Auto);
}

#[test]
fn test_env_var_override_via_helper() {
    let toml_str = include_str!("fixtures/default.toml");
    let mut config = Config::from_toml_str(toml_str).expect("default.toml must parse");

    assert_eq!(
        config.models.models_dir,
        PathBuf::from("/var/lib/ai-mate/models")
    );

    // Apply env override using mock env provider
    let override_path = "/custom/env/models/path";
    config.apply_env_overrides_from(|key| {
        if key == ENV_AI_MATE_MODEL_DIR {
            Some(override_path.to_string())
        } else {
            None
        }
    });

    assert_eq!(config.models.models_dir, PathBuf::from(override_path));
    // Other fields must remain intact
    assert_eq!(config.models.auto_download, true);
    assert_eq!(config.models.probe_timeout_ms, 1500);
    assert_eq!(config.models.prefer_mirror, PreferMirror::Auto);
}

#[test]
fn test_env_var_override_with_std_env() {
    let toml_str = include_str!("fixtures/default.toml");
    let override_path = if cfg!(windows) {
        r"C:\test\env_override\models"
    } else {
        "/tmp/test/env_override/models"
    };

    // Set environment variable
    unsafe {
        std::env::set_var(ENV_AI_MATE_MODEL_DIR, override_path);
    }

    let loaded = Config::load_from_str(toml_str).expect("load_from_str must succeed");
    assert_eq!(loaded.models.models_dir, PathBuf::from(override_path));

    // Clean up
    unsafe {
        std::env::remove_var(ENV_AI_MATE_MODEL_DIR);
    }
}

#[test]
fn test_invalid_prefer_mirror() {
    let invalid_toml = r#"
[models]
prefer_mirror = "invalid_mirror_name"
"#;
    let result = Config::from_toml_str(invalid_toml);
    assert!(result.is_err(), "Invalid prefer_mirror must fail to parse");
}

#[test]
fn test_roundtrip_toml() {
    let original = Config {
        models: ai_mate::config::ModelsConfig {
            models_dir: PathBuf::from("/test/models"),
            auto_download: false,
            probe_timeout_ms: 2500,
            prefer_mirror: PreferMirror::ModelScope,
        },
    };

    let serialized = toml::to_string_pretty(&original).expect("serialization must succeed");
    let deserialized: Config = toml::from_str(&serialized).expect("deserialization must succeed");

    assert_eq!(original, deserialized);
}
