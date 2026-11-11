pub const fn product() -> &'static str {
    env!("FLOWY_PRODUCT")
}

pub const fn bundle_id() -> &'static str {
    env!("FLOWY_BUNDLE_ID")
}

pub const fn qualifier_tld() -> &'static str {
    env!("FLOWY_QUALIFIER_TLD")
}

pub const fn qualifier_org() -> &'static str {
    env!("FLOWY_QUALIFIER_ORG")
}

pub const fn qualifier_app() -> &'static str {
    env!("FLOWY_QUALIFIER_APP")
}

pub const fn url_scheme() -> &'static str {
    env!("FLOWY_URL_SCHEME")
}

pub const fn logo_png() -> &'static [u8] {
    include_bytes!(env!("FLOWY_LOGO"))
}

pub const fn ota_github_repo() -> &'static str {
    env!("FLOWY_OTA_GITHUB_REPO")
}

pub const fn ota_modelscope_cn_repo() -> &'static str {
    env!("FLOWY_OTA_MS_CN_REPO")
}

pub const fn ota_modelscope_ai_repo() -> &'static str {
    env!("FLOWY_OTA_MS_AI_REPO")
}

pub const fn ota_prefix() -> &'static str {
    env!("FLOWY_OTA_PREFIX")
}

pub const fn ota_pubkey() -> &'static str {
    env!("FLOWY_OTA_PUBKEY")
}

pub const fn channel_config() -> flowy_ota::ChannelConfig {
    flowy_ota::ChannelConfig::from_brand(
        ota_github_repo(),
        ota_modelscope_cn_repo(),
        ota_modelscope_ai_repo(),
        ota_prefix(),
        ota_pubkey(),
        env!("CARGO_PKG_VERSION"),
    )
}
