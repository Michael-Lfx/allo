//! Baked-in manifests for the three supported local models and the single
//! `llama.cpp` `standard` runtime variant they launch with.
//!
//! These are verbatim projections of the herdsman `embed/manifest/*.json`
//! files for `Qwen3.8-27B`, `Qwen3.6-35B-A3B` and `Qwen3.6-35B-A3B-MTP`.
//! Any other catalog id is rejected by the service. The constants below also
//! pin the separate Houmo `HLIELLama` runtime the remote `houmo-*` catalog uses.

/// One downloadable file with an alternate mirror.
pub(crate) struct FileSpec {
    pub file_name: &'static str,
    pub file_size: u64,
    pub sha256: Option<&'static str>,
    /// Primary (ModelScope) URL.
    pub modelscope: &'static str,
    /// Fallback (Hugging Face) URL.
    pub huggingface: &'static str,
}

/// Static metadata for a supported model.
pub(crate) struct ModelManifest {
    pub id: &'static str,
    pub display_name: (&'static str, &'static str),
    pub description: (&'static str, &'static str),
    pub model_type: &'static str,
    pub capabilities: &'static [&'static str],
    pub tags: &'static [&'static str],
    pub scenarios: &'static [&'static str],
    pub parameter_count: f64,
    pub size_bytes: u64,
    pub author: &'static str,
    pub license: &'static str,
    pub icon: &'static str,
    pub main_gguf: &'static str,
    pub mmproj: Option<&'static str>,
    pub dflash: Option<&'static str>,
    pub supports_mtp: bool,
    pub supports_dflash: bool,
    pub context_size: u64,
    pub max_tokens: u64,
    pub dflash_gpu_layers: u32,
    pub dflash_draft_min: u32,
    pub dflash_draft_max: u32,
    pub files: &'static [FileSpec],
}

/// The supported model catalog, in store display order.
pub(crate) const MODELS: &[ModelManifest] = &[
    ModelManifest {
        id: "Qwen3.8-27B",
        display_name: ("Qwen3.8-27B", "Qwen3.8-27B"),
        description: (
            "Qwen3.8-27B MTP 是 Qwen3.8 系列支持 MTP 的 GGUF 模型，支持视觉理解、智能体编程、多语言交互，以及思考和非思考模式。",
            "Qwen3.8-27B MTP is the MTP-enabled GGUF model from the Qwen3.8 series. It supports visual understanding, agentic coding, multilingual interaction, and thinking and non-thinking modes.",
        ),
        model_type: "multimodal",
        capabilities: &["agent-work"],
        tags: &["text-generation", "qwen3.8", "llama", "open-source", "mtp"],
        scenarios: &[
            "multimodal-chat",
            "image-understanding",
            "vision-qa",
            "document-ocr",
            "visual-reasoning",
            "long-context",
            "summary",
            "work",
            "chinese",
            "multilingual",
        ],
        parameter_count: 27.0,
        size_bytes: 17_395_586_656,
        author: "Qwen",
        license: "Apache 2.0",
        icon: "/model_logo/qwen.png",
        main_gguf: "Qwen3.8-27B-UD-Q4_K_M.gguf",
        mmproj: Some("mmproj-BF16.gguf"),
        dflash: None,
        supports_mtp: true,
        supports_dflash: false,
        context_size: 262_144,
        max_tokens: 262_144,
        dflash_gpu_layers: 99,
        dflash_draft_min: 0,
        dflash_draft_max: 2,
        files: &[
            FileSpec {
                file_name: "Qwen3.8-27B-UD-Q4_K_M.gguf",
                file_size: 16_464_440_224,
                sha256: Some(
                    "322e194ff79741c7baa497c240f677f54b201b0efab44ca8e50f122b39123482",
                ),
                modelscope: "https://modelscope.cn/models/flowy2025/herdsman_models/resolve/master/qwen3.8_27b_mtp/Qwen3.8-27B-UD-Q4_K_M.gguf",
                huggingface: "https://huggingface.co/datasets/flowyrepo/herdsman_models/resolve/main/qwen3.8_27b_mtp/Qwen3.8-27B-UD-Q4_K_M.gguf",
            },
            FileSpec {
                file_name: "mmproj-BF16.gguf",
                file_size: 931_146_432,
                sha256: Some(
                    "83ee4f4f205fa514161778c41df1ea14144faa0f713510893b63c2395f5c2d53",
                ),
                modelscope: "https://modelscope.cn/models/flowy2025/herdsman_models/resolve/master/qwen3.8_27b_mtp/mmproj-BF16.gguf",
                huggingface: "https://huggingface.co/datasets/flowyrepo/herdsman_models/resolve/main/qwen3.8_27b_mtp/mmproj-BF16.gguf",
            },
        ],
    },
    ModelManifest {
        id: "Qwen3.6-35B-A3B",
        display_name: ("Qwen3.6-35B-A3B", "Qwen3.6-35B-A3B"),
        description: (
            "Qwen3.6-35B-A3B 是一个稀疏但能力出色的混合专家（MoE）模型，总参数量 350 亿，激活参数仅 30 亿，支持多模态思考与非思考模式。",
            "Qwen3.6-35B-A3B is a sparse yet capable mixture-of-experts (MoE) model with 35B total parameters and only 3B active parameters, supporting multimodal thinking and non-thinking modes.",
        ),
        model_type: "multimodal",
        capabilities: &["agent-work"],
        tags: &["text-generation", "llama", "open-source"],
        scenarios: &[
            "multimodal-chat",
            "image-understanding",
            "vision-qa",
            "document-ocr",
            "visual-reasoning",
            "long-context",
            "summary",
            "work",
            "chinese",
            "multilingual",
        ],
        parameter_count: 35.0,
        size_bytes: 23_273_038_016,
        author: "Qwen",
        license: "Apache 2.0",
        icon: "/model_logo/qwen.png",
        main_gguf: "Qwen3.6-35B-A3B-UD-Q4_K_M.gguf",
        mmproj: Some("mmproj-F16.gguf"),
        dflash: Some("Qwen3.6-35B-A3B-DFlash-Q4_K_M.gguf"),
        supports_mtp: false,
        supports_dflash: true,
        context_size: 256_144,
        max_tokens: 81_920,
        dflash_gpu_layers: 99,
        dflash_draft_min: 0,
        dflash_draft_max: 2,
        files: &[
            FileSpec {
                file_name: "Qwen3.6-35B-A3B-UD-Q4_K_M.gguf",
                file_size: 22_134_528_992,
                sha256: Some(
                    "ac0e2c1189e055faa36eff361580e79c5bd6f8e76bffb4ce547f167d53e31a61",
                ),
                modelscope: "https://modelscope.cn/models/flowy2025/herdsman_models/resolve/master/qwen3.6_35b_a3b/Qwen3.6-35B-A3B-UD-Q4_K_M.gguf",
                huggingface: "https://huggingface.co/datasets/flowyrepo/herdsman_models/resolve/main/qwen3.6_35b_a3b/Qwen3.6-35B-A3B-UD-Q4_K_M.gguf",
            },
            FileSpec {
                file_name: "mmproj-F16.gguf",
                file_size: 902_822_624,
                sha256: Some(
                    "356dfaa3111376a4f7165e32e8749713378d1700b37cf52e0c50d9f23322334d",
                ),
                modelscope: "https://modelscope.cn/models/flowy2025/herdsman_models/resolve/master/qwen3.6_35b_a3b/mmproj-F16.gguf",
                huggingface: "https://huggingface.co/datasets/flowyrepo/herdsman_models/resolve/main/qwen3.6_35b_a3b/mmproj-F16.gguf",
            },
            FileSpec {
                file_name: "Qwen3.6-35B-A3B-DFlash-Q4_K_M.gguf",
                file_size: 235_692_224,
                sha256: Some(
                    "107b7a433cbbdb4656a52b7845b35f6eed94eb0efb0019c8af7faed16d2d52f7",
                ),
                modelscope: "https://modelscope.cn/models/flowy2025/herdsman_models/resolve/master/qwen3.6_35b_a3b/Qwen3.6-35B-A3B-DFlash-Q4_K_M.gguf",
                huggingface: "https://huggingface.co/datasets/flowyrepo/herdsman_models/resolve/main/qwen3.6_35b_a3b/Qwen3.6-35B-A3B-DFlash-Q4_K_M.gguf",
            },
        ],
    },
    ModelManifest {
        id: "Qwen3.6-35B-A3B-MTP",
        display_name: ("Qwen3.6-35B-A3B-MTP", "Qwen3.6-35B-A3B-MTP"),
        description: (
            "Qwen3.6-35B-A3B-MTP 是 Qwen3.6-35B-A3B 的 MTP GGUF 版本，需要支持 MTP 的 llama.cpp 运行时，并默认使用 draft-mtp 推测解码参数。",
            "Qwen3.6-35B-A3B-MTP is the MTP-enabled GGUF variant of Qwen3.6-35B-A3B. It requires a llama.cpp runtime with MTP support and is configured for draft-mtp speculative decoding.",
        ),
        model_type: "multimodal",
        capabilities: &["agent-work"],
        tags: &["text-generation", "llama", "open-source", "mtp"],
        scenarios: &[
            "multimodal-chat",
            "image-understanding",
            "vision-qa",
            "document-ocr",
            "visual-reasoning",
            "long-context",
            "summary",
            "work",
            "chinese",
            "multilingual",
        ],
        parameter_count: 35.0,
        size_bytes: 23_566_209_952,
        author: "Qwen",
        license: "Apache 2.0",
        icon: "/model_logo/qwen.png",
        main_gguf: "Qwen3.6-35B-A3B-UD-Q4_K_M.gguf",
        mmproj: Some("mmproj-F16.gguf"),
        dflash: None,
        supports_mtp: true,
        supports_dflash: false,
        context_size: 256_144,
        max_tokens: 81_920,
        dflash_gpu_layers: 99,
        dflash_draft_min: 0,
        dflash_draft_max: 2,
        files: &[
            FileSpec {
                file_name: "Qwen3.6-35B-A3B-UD-Q4_K_M.gguf",
                file_size: 22_663_387_424,
                sha256: Some(
                    "0b21525e972670ed59e1812e170b27c26355381f0656ecc4e25617ece7dac58b",
                ),
                modelscope: "https://modelscope.cn/models/flowy2025/herdsman_models/resolve/master/qwen3.6_35b_a3b_mtp/Qwen3.6-35B-A3B-UD-Q4_K_M.gguf",
                huggingface: "https://huggingface.co/datasets/flowyrepo/herdsman_models/resolve/main/qwen3.6_35b_a3b_mtp/Qwen3.6-35B-A3B-UD-Q4_K_M.gguf",
            },
            FileSpec {
                file_name: "mmproj-F16.gguf",
                file_size: 902_822_528,
                sha256: Some(
                    "da63cb47a76763c712393f8a017070188a304fa39f8aeea6edc629ed7b975cfa",
                ),
                modelscope: "https://modelscope.cn/models/flowy2025/herdsman_models/resolve/master/qwen3.6_35b_a3b_mtp/mmproj-F16.gguf",
                huggingface: "https://huggingface.co/datasets/flowyrepo/herdsman_models/resolve/main/qwen3.6_35b_a3b_mtp/mmproj-F16.gguf",
            },
        ],
    },
];

/// The single supported runtime variant (`llama.cpp` `standard`, Windows
/// Vulkan x64). `llama-server.exe` lives at the zip root after extraction.
pub(crate) const RUNTIME_VERSION: &str = "llama-b10837-g5202104b5-bin-win-vulkan-x64";
pub(crate) const RUNTIME_URL: &str = "https://modelscope.cn/datasets/flowy2025/local-model-runtimes/resolve/master/llamacpp/windows/llama-b10837-g5202104b5-bin-win-vulkan-x64.zip";
pub(crate) const RUNTIME_SHA256: &str =
    "bf045e506db16a0ca243c59a186248b3939ce18fe08f00d68d3c9e1c8aea1479";

/// The linux aarch64 Houmo `HLIELLama` xh2 runtime, paired with the Houmo NPU
/// models. The archive unpacks to a single `houmo-HLIELLama-xh2/` package root
/// whose `bin/llama-server` binary loads its shared libraries from `lib/`.
pub(crate) const HOUMO_RUNTIME_VERSION: &str = "2.1.1";
pub(crate) const HOUMO_RUNTIME_DIR: &str = "runtimes/houmo/xh2";
pub(crate) const HOUMO_RUNTIME_ARCHIVE: &str = "houmo-HLIELLama-xh2_2.1.1_linux_aarch64.tar.gz";
pub(crate) const HOUMO_RUNTIME_URL: &str = "https://modelscope.cn/datasets/flowy2025/local-model-runtimes/resolve/master/houmo/linux-aarch64/houmo-HLIELLama-xh2_2.1.1_linux_aarch64.tar.gz";
pub(crate) const HOUMO_RUNTIME_SHA256: &str =
    "2e9b20e6e1ed2364dfbc6d114d9ef405986afb005efe5082c416919d63f978bc";

/// Which runtime family a catalog id launches with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeKind {
    /// The downloaded `llama.cpp` Vulkan runtime.
    LlamaCpp,
    /// The Houmo `HLIELLama` NPU runtime.
    Houmo,
}

/// Map a catalog id to the runtime family it launches with. Builtin GGUF ids
/// always use `llama.cpp`; Houmo NPU ids always use the Houmo runtime.
pub(crate) fn runtime_kind(id: &str) -> RuntimeKind {
    if crate::houmo::is_houmo_id(id) {
        RuntimeKind::Houmo
    } else {
        RuntimeKind::LlamaCpp
    }
}

/// Look up a supported model by its catalog id.
pub(crate) fn find_model(id: &str) -> Option<&'static ModelManifest> {
    MODELS.iter().find(|model| model.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_kind_pairs_houmo_ids_with_the_houmo_runtime() {
        assert_eq!(runtime_kind("houmo-108"), RuntimeKind::Houmo);
        for id in crate::SUPPORTED_MODEL_IDS.iter().copied() {
            assert_eq!(runtime_kind(id), RuntimeKind::LlamaCpp);
        }
    }
}
