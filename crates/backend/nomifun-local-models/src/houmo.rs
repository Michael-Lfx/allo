//! Houmo NPU catalog, queried the same way `model-cli query` does.
//!
//! `model-cli` pages `https://llm.houmoai.com/frontend/get/models` with
//! `model_format=gguf&target=xh2` and downloads the cloud `external_url`
//! (falling back to the internal artifact URL), plus the matching mmproj when
//! the entry has one. This module owns that catalog shape. Houmo entries launch
//! through the dedicated Houmo `HLIELLama` runtime, separate from the builtin
//! `llama.cpp` Vulkan runtime.

use std::collections::{BTreeMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::Deserialize;

/// How long a successful (or failed) catalog snapshot is reused.
pub(crate) const CATALOG_TTL: Duration = Duration::from_secs(300);

/// Public list `model-cli` calls. Override with `HLCLOUD_MODEL_API_URL`.
const DEFAULT_API_URL: &str = "https://llm.houmoai.com/frontend/get/models";
const DEFAULT_TARGET: &str = "xh2";
const PAGE_LIMIT: u64 = 100;
const MAX_PAGES: usize = 20;

/// Stable id prefix so a Houmo numeric id cannot collide with a builtin id.
const ID_PREFIX: &str = "houmo-";

/// In-memory snapshot of the remote catalog.
#[derive(Default)]
pub(crate) struct HoumoCache {
    pub models: Vec<HoumoModel>,
    pub fetched_at: Option<Instant>,
    pub error: Option<String>,
}

impl HoumoCache {
    pub(crate) fn is_fresh(&self) -> bool {
        self.fetched_at
            .is_some_and(|at| at.elapsed() < CATALOG_TTL)
    }
}

/// One downloadable Houmo GGUF (and optional mmproj) the store can install.
#[derive(Debug, Clone)]
pub(crate) struct HoumoModel {
    pub id: String,
    pub display_name: String,
    pub description_zh: String,
    pub description_en: String,
    pub model_type: String,
    pub parameter_count: f64,
    pub tags: Vec<String>,
    pub engines: Vec<String>,
    pub icon: Option<String>,
    pub context_size: u64,
    pub supports_mtp: bool,
    pub main_gguf: String,
    pub mmproj: Option<String>,
    pub files: Vec<HoumoFile>,
}

/// One file, with mirrors in the order `model-cli` tries them (cloud first).
#[derive(Debug, Clone)]
pub(crate) struct HoumoFile {
    pub file_name: String,
    pub urls: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RemoteModel {
    id: u64,
    #[serde(default)]
    model_id: String,
    #[serde(default)]
    release_status: String,
    #[serde(default)]
    lanyue_version: String,
    #[serde(default)]
    model_type: String,
    #[serde(default)]
    parameter_size: f64,
    #[serde(default)]
    quantization: String,
    #[serde(default)]
    context_length: u64,
    #[serde(default)]
    core_num: u32,
    #[serde(default)]
    chip_num: u32,
    #[serde(default)]
    support_engines: Vec<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    external_url: Option<String>,
    #[serde(default)]
    mmproj_url: Option<String>,
    #[serde(default)]
    external_mmproj_url: Option<String>,
    #[serde(default)]
    is_available: bool,
    #[serde(default)]
    description: Option<String>,
}

/// Windows dev install of the Houmo `llama-server`. Linux aarch64 uses the
/// `bin/llama-server` from the downloaded `HLIELLama` package.
const WINDOWS_SERVER: &str = r"D:\houmo-HLIELLama-xh2\bin\llama-server.exe";

/// Package root of the `HLIELLama` xh2 archive and the relative paths the
/// extracted runtime exposes (`bin/llama-server`, `lib/`).
const PACKAGE_ROOT: &str = "houmo-HLIELLama-xh2";
const UNIX_BIN_REL: &str = "bin/llama-server";
const UNIX_LIB_REL: &str = "lib";

/// The `HOUMO_LLAMA_SERVER` override, when set to a non-empty value.
pub(crate) fn override_path() -> Option<PathBuf> {
    std::env::var("HOUMO_LLAMA_SERVER")
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// The extracted Houmo package root under the runtime directory.
pub(crate) fn package_root(runtime_dir: &Path) -> PathBuf {
    runtime_dir.join(PACKAGE_ROOT)
}

/// The fixed Houmo runtime. `HOUMO_LLAMA_SERVER` replaces the built-in path.
pub(crate) fn server_program(runtime_dir: &Path) -> PathBuf {
    match override_path() {
        Some(path) => path,
        None => server_program_from(runtime_dir, None),
    }
}

/// Pick the runtime without reading the process environment. Windows keeps the
/// fixed dev path; every other target uses the extracted package binary instead
/// of relying on a `llama-server` on `PATH`.
pub(crate) fn server_program_from(runtime_dir: &Path, override_path: Option<&str>) -> PathBuf {
    if let Some(value) = override_path.map(str::trim).filter(|value| !value.is_empty()) {
        return PathBuf::from(value);
    }
    if cfg!(windows) {
        PathBuf::from(WINDOWS_SERVER)
    } else {
        package_root(runtime_dir).join(UNIX_BIN_REL)
    }
}

/// The working directory and environment a Houmo `llama-server` needs.
pub(crate) struct ServerContext {
    pub cwd: PathBuf,
    pub env: BTreeMap<OsString, OsString>,
}

/// Resolve the launch context for a `llama-server` program.
///
/// A program inside the extracted package root runs from that root with
/// `LD_LIBRARY_PATH` pointing at its `lib/` folder. Anything else (the Windows
/// dev install or a `HOUMO_LLAMA_SERVER` override) runs from the binary's own
/// directory with no extra environment.
pub(crate) fn server_context(runtime_dir: &Path, program: &Path) -> ServerContext {
    let root = package_root(runtime_dir);
    if program.starts_with(&root) {
        let mut env = BTreeMap::new();
        env.insert(
            OsString::from("LD_LIBRARY_PATH"),
            root.join(UNIX_LIB_REL).into_os_string(),
        );
        ServerContext { cwd: root, env }
    } else {
        let cwd = program
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| runtime_dir.to_path_buf());
        ServerContext {
            cwd,
            env: BTreeMap::new(),
        }
    }
}

/// True for catalog ids this module mints (`houmo-108`).
pub(crate) fn is_houmo_id(id: &str) -> bool {
    let Some(rest) = id.strip_prefix(ID_PREFIX) else {
        return false;
    };
    !rest.is_empty() && rest.bytes().all(|byte| byte.is_ascii_digit())
}

/// Fetch every page of the Houmo GGUF catalog for the configured target.
pub(crate) async fn fetch_catalog(client: &reqwest::Client) -> Result<Vec<HoumoModel>, String> {
    let base = std::env::var("HLCLOUD_MODEL_API_URL").unwrap_or_else(|_| DEFAULT_API_URL.to_owned());
    let target = std::env::var("HOUMO_MODEL_TARGET").unwrap_or_else(|_| DEFAULT_TARGET.to_owned());
    let mut skip = 0u64;
    let mut raw = Vec::new();
    let mut seen_pages = HashSet::new();
    for _ in 0..MAX_PAGES {
        let url = page_url(&base, &target, PAGE_LIMIT, skip);
        let response = client
            .get(&url)
            .send()
            .await
            .map_err(|error| error.to_string())?;
        if !response.status().is_success() {
            return Err(format!("HTTP {}", response.status()));
        }
        let page: Vec<RemoteModel> = response
            .json()
            .await
            .map_err(|error| error.to_string())?;
        if page.is_empty() {
            break;
        }
        let page_key = page.first().map(|model| model.id).unwrap_or(0);
        if !seen_pages.insert(page_key) {
            break;
        }
        let len = page.len() as u64;
        raw.extend(page);
        if len < PAGE_LIMIT {
            break;
        }
        skip += len;
    }
    Ok(models_from_remote(raw))
}

/// Ask the first mirror that answers for the object size.
///
/// `Response::content_length()` is the body size. A `HEAD` has no body, so
/// that method is always empty even when the `Content-Length` header is set.
/// Read the header itself.
pub(crate) async fn probe_file_size(client: &reqwest::Client, urls: &[String]) -> Option<u64> {
    for url in urls {
        let Ok(response) = client.head(url).send().await else {
            continue;
        };
        if response.status().is_success()
            && let Some(len) = content_length_header(&response)
        {
            return Some(len);
        }
    }
    None
}

/// `Content-Length` header value, ignoring an empty or zero length.
pub(crate) fn content_length_header(response: &reqwest::Response) -> Option<u64> {
    let len = response
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)?
        .to_str()
        .ok()?
        .parse()
        .ok()?;
    (len > 0).then_some(len)
}

/// Build one page URL, preserving a query string already present on `base`.
pub(crate) fn page_url(base: &str, target: &str, limit: u64, skip: u64) -> String {
    let joiner = if base.contains('?') { '&' } else { '?' };
    format!(
        "{base}{joiner}model_format=gguf&target={target}&limit={limit}&skip={skip}"
    )
}

fn models_from_remote(raw: Vec<RemoteModel>) -> Vec<HoumoModel> {
    let mut name_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for model in &raw {
        if model.is_available && has_download(&model) {
            *name_counts.entry(model.model_id.clone()).or_default() += 1;
        }
    }
    let mut seen_ids = HashSet::new();
    let mut models = Vec::new();
    for model in raw {
        if !model.is_available || !has_download(&model) || !seen_ids.insert(model.id) {
            continue;
        }
        let duplicate = name_counts.get(&model.model_id).copied().unwrap_or(0) > 1;
        if let Some(mapped) = map_model(model, duplicate) {
            models.push(mapped);
        }
    }
    models
}

fn has_download(model: &RemoteModel) -> bool {
    usable(&model.external_url) || usable(&model.url)
}

fn usable(value: &Option<String>) -> bool {
    value.as_deref().is_some_and(|url| !url.trim().is_empty())
}

fn map_model(model: RemoteModel, duplicate_name: bool) -> Option<HoumoModel> {
    let main_urls = mirrors(model.external_url.clone(), model.url.clone());
    let main_name = file_name_from_url(main_urls.first()?)?;
    let mmproj_urls = mirrors(model.external_mmproj_url.clone(), model.mmproj_url.clone());
    let mmproj_name = mmproj_urls
        .first()
        .and_then(|url| file_name_from_url(url));
    let mut files = vec![HoumoFile {
        file_name: main_name.clone(),
        urls: main_urls,
    }];
    if let Some(name) = mmproj_name.clone() {
        files.push(HoumoFile {
            file_name: name,
            urls: mmproj_urls,
        });
    }
    let (description_zh, description_en) = descriptions(&model);
    let supports_mtp = model.model_id.to_ascii_lowercase().contains("mtp");
    let mut tags = Vec::new();
    if !model.quantization.is_empty() {
        tags.push(model.quantization.clone());
    }
    if model.context_length > 0 {
        tags.push(format_ctx(model.context_length));
    }
    if supports_mtp {
        tags.push("mtp".to_owned());
    } else if model.chip_num > 0 {
        tags.push(format!("{}chip", model.chip_num));
    }
    let display_name = if duplicate_name && !model.quantization.is_empty() {
        format!("{} · {}", model.model_id, model.quantization)
    } else {
        model.model_id.clone()
    };
    let icon = if model.model_id.to_ascii_lowercase().contains("qwen")
        || model.model_type.to_ascii_lowercase().contains("qwen")
    {
        Some("/model_logo/qwen.png".to_owned())
    } else {
        None
    };
    let engines = if model.support_engines.is_empty() {
        vec!["HLIELLama".to_owned()]
    } else {
        model.support_engines.clone()
    };
    Some(HoumoModel {
        id: format!("{ID_PREFIX}{}", model.id),
        display_name,
        description_zh,
        description_en,
        model_type: if model.model_type.is_empty() {
            "gguf".to_owned()
        } else {
            model.model_type
        },
        parameter_count: model.parameter_size,
        tags,
        engines,
        icon,
        context_size: model.context_length,
        supports_mtp,
        main_gguf: main_name,
        mmproj: mmproj_name,
        files,
    })
}

fn mirrors(external: Option<String>, internal: Option<String>) -> Vec<String> {
    let mut urls = Vec::new();
    for candidate in [external, internal] {
        if let Some(url) = candidate.map(|value| value.trim().to_owned())
            && !url.is_empty()
            && !urls.iter().any(|existing| existing == &url)
        {
            urls.push(url);
        }
    }
    urls
}

fn descriptions(model: &RemoteModel) -> (String, String) {
    let params = format_params(model.parameter_size);
    let ctx = format_ctx(model.context_length);
    let status_zh = match model.release_status.to_ascii_lowercase().as_str() {
        "stable" => "稳定",
        "temporary" => "临时",
        "testing" => "测试",
        _ => model.release_status.as_str(),
    };
    let zh = format!(
        "{params}B · {} · 上下文 {ctx} · {} 芯片 / {} 核 · 览月 {} · {status_zh}",
        model.quantization, model.chip_num, model.core_num, model.lanyue_version
    );
    let en = format!(
        "{params}B · {} · context {ctx} · {} chip(s) / {} core(s) · Lanyue {} · {}",
        model.quantization, model.chip_num, model.core_num, model.lanyue_version, model.release_status
    );
    match model.description.as_deref().map(str::trim).filter(|text| !text.is_empty()) {
        Some(text) => (text.to_owned(), en),
        None => (zh, en),
    }
}

fn format_params(value: f64) -> String {
    if !value.is_finite() || value <= 0.0 {
        return "0".to_owned();
    }
    if value.fract() == 0.0 {
        format!("{}", value as u64)
    } else {
        let text = format!("{value:.1}");
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    }
}

fn format_ctx(len: u64) -> String {
    if len == 0 {
        return "0".to_owned();
    }
    if len % 1024 == 0 {
        let kib = len / 1024;
        if kib % 1024 == 0 {
            return format!("{}M", kib / 1024);
        }
        return format!("{kib}K");
    }
    len.to_string()
}

fn file_name_from_url(url: &str) -> Option<String> {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    let raw = path.rsplit(['/', '\\']).next().unwrap_or("");
    let name = percent_decode(raw);
    let name = name.trim();
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains(['/', '\\', ':'])
    {
        return None;
    }
    Some(name.to_owned())
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(decoded) = u8::from_str_radix(
                std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or(""),
                16,
            ) {
                out.push(decoded);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_json() -> &'static str {
        r#"[
          {
            "id": 108,
            "model_id": "Qwen3.6_35b-a3b_256k_b1_2core_1chip(dynamic)",
            "release_status": "Stable",
            "lanyue_version": "2.1.0",
            "model_type": "Qwen3.6",
            "parameter_size": 35.0,
            "quantization": "w4a8",
            "context_length": 262144,
            "core_num": 2,
            "chip_num": 1,
            "support_engines": ["HLIELLama"],
            "url": "http://10.10.1.53:8082/artifactory/internal/model.gguf",
            "external_url": "https://llmoss.houmoai.com/models/HiModel.gguf",
            "mmproj_url": "http://10.10.1.53:8082/artifactory/internal/mmproj.gguf",
            "external_mmproj_url": "https://llmoss.houmoai.com/models/mmproj.gguf",
            "is_available": true,
            "description": null
          },
          {
            "id": 109,
            "model_id": "Qwen3.6_35b-a3b_256k_b1_2core_2chips(dynamic)",
            "release_status": "Stable",
            "lanyue_version": "2.1.0",
            "model_type": "Qwen3.6",
            "parameter_size": 35.0,
            "quantization": "w5a8",
            "context_length": 262144,
            "chip_num": 2,
            "core_num": 2,
            "external_url": "https://llmoss.houmoai.com/models/w5.gguf",
            "is_available": true
          },
          {
            "id": 110,
            "model_id": "Qwen3.6_35b-a3b_256k_b1_2core_2chips(dynamic)",
            "release_status": "Temporary",
            "lanyue_version": "2.1.0",
            "model_type": "Qwen3.6",
            "parameter_size": 0.8,
            "quantization": "w8a8",
            "context_length": 131072,
            "chip_num": 2,
            "core_num": 2,
            "external_url": "https://llmoss.houmoai.com/models/w8.gguf",
            "is_available": true
          },
          {
            "id": 112,
            "model_id": "Spark_x1",
            "is_available": false,
            "external_url": "https://llmoss.houmoai.com/models/spark.gguf"
          },
          {
            "id": 108,
            "model_id": "duplicate-numeric",
            "is_available": true,
            "external_url": "https://llmoss.houmoai.com/models/dup.gguf"
          }
        ]"#
    }

    #[test]
    fn catalog_keeps_available_models_and_prefers_the_cloud_url() {
        let raw: Vec<RemoteModel> = serde_json::from_str(sample_json()).expect("json");
        let models = models_from_remote(raw);
        let ids: Vec<&str> = models.iter().map(|model| model.id.as_str()).collect();
        assert_eq!(ids, vec!["houmo-108", "houmo-109", "houmo-110"]);

        let first = &models[0];
        assert_eq!(first.main_gguf, "HiModel.gguf");
        assert_eq!(first.mmproj.as_deref(), Some("mmproj.gguf"));
        assert_eq!(
            first.files[0].urls,
            vec![
                "https://llmoss.houmoai.com/models/HiModel.gguf",
                "http://10.10.1.53:8082/artifactory/internal/model.gguf",
            ]
        );
        assert_eq!(first.files[1].file_name, "mmproj.gguf");
        assert_eq!(first.parameter_count, 35.0);
        assert!(first.icon.as_deref().unwrap().contains("qwen"));
        assert!(first.description_zh.contains("256K"));
        assert!(first.description_en.contains("Lanyue"));
        assert_eq!(first.tags, vec!["w4a8", "256K", "1chip"]);

        assert!(models[1].display_name.contains("w5a8"));
        assert!(models[2].display_name.contains("w8a8"));
        assert_eq!(models[2].tags, vec!["w8a8", "128K", "2chip"]);
        assert!(models[2].description_zh.contains("0.8B"));
    }

    #[test]
    fn page_url_appends_the_model_cli_query() {
        assert_eq!(
            page_url("https://llm.houmoai.com/frontend/get/models", "xh2", 100, 0),
            "https://llm.houmoai.com/frontend/get/models?model_format=gguf&target=xh2&limit=100&skip=0"
        );
        assert!(
            page_url(
                "https://example.test/models?open_type=internal",
                "xh2",
                100,
                100
            )
            .starts_with("https://example.test/models?open_type=internal&")
        );
    }

    #[test]
    fn the_runtime_is_the_fixed_houmo_llama_server() {
        let runtime_dir = Path::new("runtime");
        assert_eq!(
            server_program_from(runtime_dir, None),
            if cfg!(windows) {
                PathBuf::from(r"D:\houmo-HLIELLama-xh2\bin\llama-server.exe")
            } else {
                runtime_dir
                    .join("houmo-HLIELLama-xh2")
                    .join("bin")
                    .join("llama-server")
            }
        );
        assert_eq!(
            server_program_from(runtime_dir, Some(r"C:\runtime\llama-server.exe")),
            PathBuf::from(r"C:\runtime\llama-server.exe")
        );
    }

    #[test]
    fn the_extracted_package_runs_from_its_root_with_its_lib_on_the_path() {
        let runtime_dir = Path::new("runtime");
        let program = package_root(runtime_dir).join(UNIX_BIN_REL);
        let context = server_context(runtime_dir, &program);
        assert_eq!(context.cwd, runtime_dir.join("houmo-HLIELLama-xh2"));
        assert_eq!(
            context.env.get(&OsString::from("LD_LIBRARY_PATH")),
            Some(&runtime_dir.join("houmo-HLIELLama-xh2").join("lib").into_os_string())
        );
    }

    #[test]
    fn an_override_runs_from_its_own_directory_without_library_env() {
        let runtime_dir = Path::new("runtime");
        let program = Path::new("override/llama-server");
        let context = server_context(runtime_dir, program);
        assert_eq!(context.cwd, PathBuf::from("override"));
        assert!(context.env.is_empty());
    }

    #[test]
    fn houmo_ids_are_the_numeric_catalog_ids() {
        assert!(is_houmo_id("houmo-108"));
        assert!(!is_houmo_id("houmo-"));
        assert!(!is_houmo_id("houmo-abc"));
        assert!(!is_houmo_id("Qwen3.8-27B"));
    }

    #[test]
    fn file_names_come_from_the_url_path() {
        assert_eq!(
            file_name_from_url(
                "https://llmoss.houmoai.com/models/HiModel%20Qwen.gguf?rev=1"
            )
            .as_deref(),
            Some("HiModel Qwen.gguf")
        );
        assert_eq!(
            file_name_from_url("https://example.test/../secret").as_deref(),
            Some("secret")
        );
        assert_eq!(file_name_from_url("https://example.test/a/.."), None);
    }
}
