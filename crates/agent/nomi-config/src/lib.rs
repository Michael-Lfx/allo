// Configuration layer: runtime Config, ProviderCompat, auth, hooks, provider-specific configs.

pub mod auth;
pub mod compact;
pub mod compat;
pub mod config;
pub mod dep_check;
pub mod dep_gate;
pub mod features;
pub mod file_cache;
pub mod gateway;
pub mod hooks;
pub mod insights;
pub mod interest;
pub mod logging;
pub mod media;
pub mod plan;
pub mod runtime_dep_install;
pub mod server;
pub mod shell;

pub use gateway::{
    GatewayConfig, config_yaml_path, data_dir, default_data_dir, env_var_enabled,
    env_var_enabled_default_true, load_config, load_user_config_file, save_config_yaml,
};
pub use dep_check::{RuntimeDep, description as dep_description, is_available as dep_is_available, resolve_ffmpeg_executable};
pub use dep_gate::{await_tool_deps, spawn_background_install};
pub use runtime_dep_install::{
    auto_ensure_enabled, ensure_ffmpeg, ensure_missing_runtime_deps, ensure_runtime_dep,
    register_dep_gate_hooks,
};
pub use insights::{InsightsConfig, InsightsContributionConfig};
pub use interest::InterestConfig;
pub use media::{
    ImageGenSettings, MediaGenConfig, MediaWorkflowSettings, MediaWorkflowTemplateMap,
    VideoGenSettings, flowy_media_exposed, flowy_media_exposed_from_disk,
};
pub use server::{
    DEFAULT_SERVER_LLM_MODEL, DEFAULT_WECHAT_FLOWY_SERVER_BASE, ServerAuthConfig, ServerConfig,
    ServerLlmConfig, ServerLoginMethod, default_wechat_app_id_for_channel,
    is_valid_wechat_open_app_id,
};
