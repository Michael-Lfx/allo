//! Pure `llama-server` argument assembly.
//!
//! This is the single place that turns a [`ModelManifest`] + user launch
//! options into the exact command line. It mirrors the herdsman llama.cpp
//! runtime rules:
//!
//! - mmproj is mounted only when the multimodal projector exists and DFlash is
//!   **off**.
//! - DFlash (`Qwen3.6-35B-A3B` only) adds `--model-draft --spec-type
//!   draft-dflash --gpu-layers-draft --spec-draft-n-min --spec-draft-n-max` and
//!   suppresses mmproj.
//! - MTP (`Qwen3.8-27B`, `Qwen3.6-35B-A3B-MTP`) adds `-fa on --spec-type
//!   draft-mtp --spec-draft-n-max` and forces DFlash off.
//! - n-gram speculative decoding is available only when both are off.

use std::path::Path;

use nomifun_api_types::LocalModelLaunchOptions;

use crate::manifest::ModelManifest;

pub(crate) const DEFAULT_CACHE_RAM_MIB: i64 = 8192;
pub(crate) const DEFAULT_CACHE_TYPE: &str = "f16";

/// Build the dialog defaults for a model: manifest sampling/context values with
/// herdsman's per-model playback switches (MTP and mmproj on for MTP models,
/// mmproj on / DFlash off for the 35B-A3B model).
pub(crate) fn default_launch_options(model: &ModelManifest) -> LocalModelLaunchOptions {
    let mut options = LocalModelLaunchOptions {
        context_size: Some(model.context_size),
        cache_type_k: Some(DEFAULT_CACHE_TYPE.to_owned()),
        cache_type_v: Some(DEFAULT_CACHE_TYPE.to_owned()),
        cache_ram: Some(DEFAULT_CACHE_RAM_MIB),
        parallel: Some(1),
        no_mmap: Some(true),
        cont_batching: Some(false),
        no_kv_offload: Some(false),
        enable_dflash: Some(false),
        dflash_gpu_layers: Some(model.dflash_gpu_layers),
        dflash_draft_min: Some(model.dflash_draft_min),
        dflash_draft_max: Some(model.dflash_draft_max),
        mtp_speculative_decoding: Some(model.supports_mtp),
        mtp_draft_max: Some(2),
        ngram_speculative_decoding: Some(false),
        spec_type: Some("ngram-mod".to_owned()),
        spec_ngram_size_n: Some(24),
        draft_min: Some(48),
        draft_max: Some(64),
        ..Default::default()
    };
    match model.id {
        "Qwen3.8-27B" => {
            options.temperature = Some(1.0);
            options.top_p = Some(0.95);
            options.top_k = Some(20);
            options.min_p = Some(0.0);
            options.repeat_penalty = Some(1.0);
        }
        "Qwen3.6-35B-A3B" => {
            options.temperature = Some(0.7);
            options.top_p = Some(0.95);
            options.top_k = Some(40);
            options.repeat_penalty = Some(1.1);
        }
        "Qwen3.6-35B-A3B-MTP" => {
            options.temperature = Some(0.7);
            options.top_p = Some(0.95);
            options.top_k = Some(40);
            options.repeat_penalty = Some(1.1);
            options.gpu_layers = Some(99);
            options.gpu_layers_auto = Some(true);
        }
        _ => {}
    }
    options
}

/// The effective options after applying manifest defaults to a request:
/// request values win, then defaults fill the gaps.
pub(crate) fn normalize_options(
    model: &ModelManifest,
    requested: LocalModelLaunchOptions,
) -> LocalModelLaunchOptions {
    let defaults = default_launch_options(model);
    LocalModelLaunchOptions {
        context_size: requested.context_size.or(defaults.context_size),
        cache_type_k: requested.cache_type_k.or(defaults.cache_type_k),
        cache_type_v: requested.cache_type_v.or(defaults.cache_type_v),
        temperature: requested.temperature.or(defaults.temperature),
        top_p: requested.top_p.or(defaults.top_p),
        top_k: requested.top_k.or(defaults.top_k),
        repeat_penalty: requested.repeat_penalty.or(defaults.repeat_penalty),
        min_p: requested.min_p.or(defaults.min_p),
        parallel: requested.parallel.or(defaults.parallel),
        batch_size: requested.batch_size.or(defaults.batch_size),
        ubatch_size: requested.ubatch_size.or(defaults.ubatch_size),
        gpu_layers: requested.gpu_layers.or(defaults.gpu_layers),
        gpu_layers_auto: requested.gpu_layers_auto.or(defaults.gpu_layers_auto),
        threads: requested.threads.or(defaults.threads),
        threads_batch: requested.threads_batch.or(defaults.threads_batch),
        cache_ram: requested.cache_ram.or(defaults.cache_ram),
        no_mmap: requested.no_mmap.or(defaults.no_mmap),
        cont_batching: requested.cont_batching.or(defaults.cont_batching),
        no_kv_offload: requested.no_kv_offload.or(defaults.no_kv_offload),
        enable_dflash: requested.enable_dflash.or(defaults.enable_dflash),
        dflash_gpu_layers: requested.dflash_gpu_layers.or(defaults.dflash_gpu_layers),
        dflash_draft_min: requested.dflash_draft_min.or(defaults.dflash_draft_min),
        dflash_draft_max: requested.dflash_draft_max.or(defaults.dflash_draft_max),
        mtp_speculative_decoding: requested
            .mtp_speculative_decoding
            .or(defaults.mtp_speculative_decoding),
        mtp_draft_max: requested.mtp_draft_max.or(defaults.mtp_draft_max),
        ngram_speculative_decoding: requested
            .ngram_speculative_decoding
            .or(defaults.ngram_speculative_decoding),
        spec_type: requested.spec_type.or(defaults.spec_type),
        spec_ngram_size_n: requested.spec_ngram_size_n.or(defaults.spec_ngram_size_n),
        draft_min: requested.draft_min.or(defaults.draft_min),
        draft_max: requested.draft_max.or(defaults.draft_max),
    }
}

/// Arguments for the fixed Houmo `llama-server`.
///
/// The binary is the Houmo NPU build, not the downloaded Vulkan runtime, so
/// this stays on the flags that server accepts for a downloaded GGUF: the
/// model, its mmproj, the listen address, and the catalog context size.
///
/// `api_key` is optional: Linux opens the LAN WebUI and passes `None` so
/// `llama-server` starts without authentication, while other hosts keep the
/// random key. An empty key is treated the same as `None` so a blank value can
/// never produce a `--api-key ""` that locks the server out.
pub(crate) fn build_houmo_args(
    alias: &str,
    main_gguf: &str,
    mmproj: Option<&str>,
    context_size: u64,
    model_dir: &Path,
    host: &str,
    port: u16,
    api_key: Option<&str>,
) -> Vec<String> {
    let mut args = vec![
        "--jinja".to_owned(),
        "-m".to_owned(),
        model_dir.join(main_gguf).to_string_lossy().into_owned(),
        "--port".to_owned(),
        port.to_string(),
        "--host".to_owned(),
        host.to_owned(),
        "--alias".to_owned(),
        alias.to_owned(),
    ];
    if let Some(api_key) = api_key.filter(|key| !key.is_empty()) {
        args.push("--api-key".to_owned());
        args.push(api_key.to_owned());
    }
    args.push("--ctx-size".to_owned());
    args.push(context_size.max(1).to_string());
    if let Some(mmproj) = mmproj {
        args.push("--mmproj".to_owned());
        args.push(model_dir.join(mmproj).to_string_lossy().into_owned());
    }
    args
}

fn ctx_size(options: &LocalModelLaunchOptions, model: &ModelManifest) -> u64 {
    options.context_size.unwrap_or(model.context_size).max(1)
}

/// Assemble the full `llama-server` argument vector.
pub(crate) fn build_args(
    model: &ModelManifest,
    options: &LocalModelLaunchOptions,
    model_dir: &Path,
    port: u16,
    api_key: &str,
) -> Vec<String> {
    let main_path = model_dir.join(model.main_gguf);
    let mut args: Vec<String> = vec![
        "--jinja".to_owned(),
        "-m".to_owned(),
        main_path.to_string_lossy().into_owned(),
        "--port".to_owned(),
        port.to_string(),
        "--host".to_owned(),
        "127.0.0.1".to_owned(),
        "--alias".to_owned(),
        model.id.to_owned(),
        "--api-key".to_owned(),
        api_key.to_owned(),
        "--ctx-size".to_owned(),
        ctx_size(options, model).to_string(),
        "--n-predict".to_owned(),
        model.max_tokens.to_string(),
        "--reasoning".to_owned(),
        "on".to_owned(),
    ];

    if let Some(value) = options.temperature {
        args.push("--temp".to_owned());
        args.push(format_float(value));
    }
    if let Some(value) = options.top_p {
        args.push("--top-p".to_owned());
        args.push(format_float(value));
    }
    if let Some(value) = options.top_k {
        args.push("--top-k".to_owned());
        args.push(value.to_string());
    }
    if let Some(value) = options.repeat_penalty {
        args.push("--repeat-penalty".to_owned());
        args.push(format_float(value));
    }
    if let Some(value) = options.min_p {
        args.push("--min-p".to_owned());
        args.push(format_float(value));
    }

    append_advanced_args(&mut args, options);
    append_speculative_args(&mut args, model, options);

    let dflash_on = dflash_enabled(model, options);
    if dflash_on {
        let dflash = model.dflash.expect("guarded by dflash_enabled");
        args.push("--model-draft".to_owned());
        args.push(model_dir.join(dflash).to_string_lossy().into_owned());
        args.push("--spec-type".to_owned());
        args.push("draft-dflash".to_owned());
        args.push("--gpu-layers-draft".to_owned());
        args.push(
            options
                .dflash_gpu_layers
                .unwrap_or(model.dflash_gpu_layers)
                .to_string(),
        );
        args.push("--spec-draft-n-min".to_owned());
        args.push(
            options
                .dflash_draft_min
                .unwrap_or(model.dflash_draft_min)
                .to_string(),
        );
        args.push("--spec-draft-n-max".to_owned());
        args.push(
            options
                .dflash_draft_max
                .unwrap_or(model.dflash_draft_max)
                .to_string(),
        );
    } else if let Some(mmproj) = model.mmproj {
        args.push("--mmproj".to_owned());
        args.push(model_dir.join(mmproj).to_string_lossy().into_owned());
    }

    if mtp_enabled(model, options) {
        args.push("-fa".to_owned());
        args.push("on".to_owned());
        args.push("--spec-type".to_owned());
        args.push("draft-mtp".to_owned());
        args.push("--spec-draft-n-max".to_owned());
        args.push(mtp_draft_max(options).to_string());
    }

    args
}

fn append_advanced_args(args: &mut Vec<String>, options: &LocalModelLaunchOptions) {
    let cache_ram = options.cache_ram.unwrap_or(DEFAULT_CACHE_RAM_MIB);
    args.push("--cache-ram".to_owned());
    args.push(cache_ram.to_string());

    if options.no_mmap.unwrap_or(true) {
        args.push("--no-mmap".to_owned());
    }

    if ngram_enabled(options) && !dflash_enabled_raw(options) {
        append_ngram_args(args, options);
    }

    let parallel = options.parallel.unwrap_or(1).max(1);
    args.push("-np".to_owned());
    args.push(parallel.to_string());

    if let Some(layers) = options.gpu_layers
        && !options.gpu_layers_auto.unwrap_or(false)
    {
        args.push("-ngl".to_owned());
        args.push(layers.to_string());
    }
    if let Some(value) = options.batch_size {
        args.push("--batch-size".to_owned());
        args.push(value.to_string());
    }
    if let Some(value) = options.ubatch_size {
        args.push("--ubatch-size".to_owned());
        args.push(value.to_string());
    }
    if let Some(value) = options.threads {
        args.push("--threads".to_owned());
        args.push(value.to_string());
    }
    if let Some(value) = options.threads_batch {
        args.push("--threads-batch".to_owned());
        args.push(value.to_string());
    }
    if let Some(value) = options.cache_type_k.as_deref().filter(|v| !v.is_empty()) {
        args.push("--cache-type-k".to_owned());
        args.push(value.to_owned());
    }
    if let Some(value) = options.cache_type_v.as_deref().filter(|v| !v.is_empty()) {
        args.push("--cache-type-v".to_owned());
        args.push(value.to_owned());
    }
    if options.cont_batching.unwrap_or(false) {
        args.push("--cont-batching".to_owned());
    }
    if options.no_kv_offload.unwrap_or(false) {
        args.push("--no-kv-offload".to_owned());
    }
    args.push("--cache-reuse".to_owned());
    args.push("256".to_owned());
}

fn append_speculative_args(
    args: &mut Vec<String>,
    model: &ModelManifest,
    options: &LocalModelLaunchOptions,
) {
    if mtp_enabled(model, options) {
        // MTP handles speculative decoding; nothing else may.
        return;
    }
    if dflash_enabled_raw(options) {
        // DFlash args are appended by the caller after this point.
        return;
    }
    if ngram_enabled(options) {
        append_ngram_args(args, options);
    }
}

fn append_ngram_args(args: &mut Vec<String>, options: &LocalModelLaunchOptions) {
    let spec_type = options
        .spec_type
        .clone()
        .unwrap_or_else(|| "ngram-mod".to_owned());
    args.push("--spec-type".to_owned());
    args.push(spec_type.clone());
    if spec_type == "ngram-mod" {
        if let Some(value) = options.spec_ngram_size_n {
            args.push("--spec-ngram-mod-n-match".to_owned());
            args.push(value.to_string());
        }
        if let Some(value) = options.draft_min {
            args.push("--spec-ngram-mod-n-min".to_owned());
            args.push(value.to_string());
        }
        if let Some(value) = options.draft_max {
            args.push("--spec-ngram-mod-n-max".to_owned());
            args.push(value.to_string());
        }
    }
}

fn mtp_enabled(model: &ModelManifest, options: &LocalModelLaunchOptions) -> bool {
    model.supports_mtp && options.mtp_speculative_decoding.unwrap_or(true)
}

fn mtp_draft_max(options: &LocalModelLaunchOptions) -> u32 {
    options
        .mtp_draft_max
        .filter(|value| (1..=16).contains(value))
        .unwrap_or(2)
}

fn dflash_enabled_raw(options: &LocalModelLaunchOptions) -> bool {
    options.enable_dflash.unwrap_or(false)
}

/// DFlash is only real when the model ships a draft file and MTP is off.
fn dflash_enabled(model: &ModelManifest, options: &LocalModelLaunchOptions) -> bool {
    model.supports_dflash && model.dflash.is_some() && dflash_enabled_raw(options) && !mtp_enabled(model, options)
}

fn ngram_enabled(options: &LocalModelLaunchOptions) -> bool {
    options.ngram_speculative_decoding.unwrap_or(false)
}

fn format_float(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.1}")
    } else {
        let mut text = format!("{value}");
        if text.contains('.') {
            while text.ends_with('0') {
                text.pop();
            }
            if text.ends_with('.') {
                text.pop();
            }
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::find_model;

    fn args_for(id: &str, options: LocalModelLaunchOptions) -> Vec<String> {
        let model = find_model(id).unwrap();
        let normalized = normalize_options(model, options);
        build_args(model, &normalized, Path::new("C:\\models"), 8080, "secret")
    }

    #[test]
    fn houmo_args_load_the_gguf_with_the_fixed_server_flags() {
        let alias = "Qwen3.6_35b-a3b_256k_b1_2core_1chip(dynamic)";
        let args = build_houmo_args(
            alias,
            "HiModel.gguf",
            Some("mmproj.gguf"),
            262_144,
            Path::new(r"D:\models\houmo-108"),
            "0.0.0.0",
            11_888,
            Some("secret"),
        );
        assert!(args.windows(2).any(|pair| {
            pair[0] == "-m" && pair[1].ends_with("HiModel.gguf")
        }));
        assert!(args.windows(2).any(|pair| {
            pair[0] == "--mmproj" && pair[1].ends_with("mmproj.gguf")
        }));
        assert!(args.windows(2).any(|pair| pair[0] == "--host" && pair[1] == "0.0.0.0"));
        assert!(args.windows(2).any(|pair| pair[0] == "--port" && pair[1] == "11888"));
        assert!(args.windows(2).any(|pair| pair[0] == "--alias" && pair[1] == alias));
        assert!(args.windows(2).any(|pair| pair[0] == "--api-key" && pair[1] == "secret"));
        assert!(args.windows(2).any(|pair| pair[0] == "--ctx-size" && pair[1] == "262144"));
        assert!(!args.iter().any(|arg| arg == "--model-draft" || arg == "draft-mtp"));
    }

    #[test]
    fn houmo_args_omit_the_api_key_when_absent() {
        let args = build_houmo_args(
            "houmo-108",
            "HiModel.gguf",
            None,
            4_096,
            Path::new("/models/houmo-108"),
            "0.0.0.0",
            11_888,
            None,
        );
        assert!(!args.iter().any(|arg| arg == "--api-key"));
    }

    #[test]
    fn houmo_args_treat_an_empty_api_key_as_absent() {
        let args = build_houmo_args(
            "houmo-108",
            "HiModel.gguf",
            None,
            4_096,
            Path::new("/models/houmo-108"),
            "0.0.0.0",
            11_888,
            Some(""),
        );
        assert!(!args.iter().any(|arg| arg == "--api-key"));
    }

    #[test]
    fn houmo_args_keep_loopback_when_asked_for_it() {
        let args = build_houmo_args(
            "houmo-108",
            "HiModel.gguf",
            None,
            4_096,
            Path::new("/models/houmo-108"),
            "127.0.0.1",
            18080,
            Some("secret"),
        );
        assert!(args.windows(2).any(|pair| pair[0] == "--host" && pair[1] == "127.0.0.1"));
        assert!(args.windows(2).any(|pair| pair[0] == "--port" && pair[1] == "18080"));
    }

    #[test]
    fn mmproj_is_mounted_when_dflash_is_off() {
        let args = args_for("Qwen3.6-35B-A3B", LocalModelLaunchOptions::default());
        assert!(args.iter().any(|a| a == "--mmproj"));
        assert!(!args.iter().any(|a| a == "--model-draft"));
    }

    #[test]
    fn dflash_adds_draft_args_and_drops_mmproj() {
        let args = args_for(
            "Qwen3.6-35B-A3B",
            LocalModelLaunchOptions {
                enable_dflash: Some(true),
                ..Default::default()
            },
        );
        assert!(args.iter().any(|a| a == "--model-draft"));
        assert!(args.windows(2).any(|w| w[0] == "--spec-type" && w[1] == "draft-dflash"));
        assert!(!args.iter().any(|a| a == "--mmproj"));
    }

    #[test]
    fn mtp_adds_draft_mtp_and_forces_dflash_off() {
        let args = args_for(
            "Qwen3.8-27B",
            LocalModelLaunchOptions {
                enable_dflash: Some(true),
                ..Default::default()
            },
        );
        assert!(args.windows(2).any(|w| w[0] == "--spec-type" && w[1] == "draft-mtp"));
        assert!(args.windows(2).any(|w| w[0] == "-fa" && w[1] == "on"));
        assert!(!args.iter().any(|a| a == "--model-draft"));
        // MTP models still mount mmproj.
        assert!(args.iter().any(|a| a == "--mmproj"));
    }

    #[test]
    fn mtp_model_defaults_to_mtp_on() {
        let args = args_for("Qwen3.6-35B-A3B-MTP", LocalModelLaunchOptions::default());
        assert!(args.windows(2).any(|w| w[0] == "--spec-type" && w[1] == "draft-mtp"));
    }

    #[test]
    fn ngram_is_available_only_when_others_are_off() {
        let args = args_for(
            "Qwen3.6-35B-A3B",
            LocalModelLaunchOptions {
                ngram_speculative_decoding: Some(true),
                ..Default::default()
            },
        );
        assert!(args.windows(2).any(|w| w[0] == "--spec-type" && w[1] == "ngram-mod"));
        assert!(args.iter().any(|a| a == "--mmproj"));
    }
}
