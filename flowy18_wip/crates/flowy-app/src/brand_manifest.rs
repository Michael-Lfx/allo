use std::path::{Path, PathBuf};

const DEFAULT_OTA_GITHUB_REPO: &str = "Michael-Lfx/allo";
const DEFAULT_OTA_MS_CN_REPO: &str = "flowy2025/flowyaipc";
const DEFAULT_OTA_MS_AI_REPO: &str = "flowy2025/flowy";
const DEFAULT_OTA_PREFIX: &str = "allo";
const DEFAULT_OTA_PUBKEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDZGRDA3NTMzQzQxODdCNjQKUldSa2V4akVNM1hRYjlrRmZLalZRdGpaM0FzeGxVd0s3VkdoY0VMY01lY2UrQ2dMdkVMTWF6SFUK";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrandManifest {
    pub id: String,
    pub product: String,
    pub bundle_id: String,
    pub qualifier_tld: String,
    pub qualifier_org: String,
    pub qualifier_app: String,
    pub url_scheme: String,
    pub icon: PathBuf,
    pub logo: PathBuf,
    pub ota_github_repo: String,
    pub ota_modelscope_cn_repo: String,
    pub ota_modelscope_ai_repo: String,
    pub ota_prefix: String,
    pub ota_pubkey: String,
}

pub fn load(dir: &Path) -> Result<BrandManifest, String> {
    if !dir.is_dir() {
        return Err(format!("{} is not a brand directory", dir.display()));
    }
    let text =
        std::fs::read_to_string(dir.join("brand.toml")).map_err(|error| {
            format!("read {}: {error}", dir.join("brand.toml").display())
        })?;
    let fields = parse_fields(&text)?;
    let brand = BrandManifest {
        id: required(&fields, "id")?,
        product: required(&fields, "product")?,
        bundle_id: required(&fields, "bundle_id")?,
        qualifier_tld: required(&fields, "qualifier_tld")?,
        qualifier_org: required(&fields, "qualifier_org")?,
        qualifier_app: required(&fields, "qualifier_app")?,
        url_scheme: required(&fields, "url_scheme")?,
        icon: dir.join(required(&fields, "icon")?),
        logo: dir.join(required(&fields, "logo")?),
        ota_github_repo: optional(
            &fields,
            "ota_github_repo",
            DEFAULT_OTA_GITHUB_REPO,
        ),
        ota_modelscope_cn_repo: optional(
            &fields,
            "ota_modelscope_cn_repo",
            DEFAULT_OTA_MS_CN_REPO,
        ),
        ota_modelscope_ai_repo: optional(
            &fields,
            "ota_modelscope_ai_repo",
            DEFAULT_OTA_MS_AI_REPO,
        ),
        ota_prefix: optional(&fields, "ota_prefix", DEFAULT_OTA_PREFIX),
        ota_pubkey: optional(&fields, "ota_pubkey", DEFAULT_OTA_PUBKEY),
    };
    validate(&brand)?;
    if !brand.icon.is_file() {
        return Err(format!("missing icon {}", brand.icon.display()));
    }
    if !brand.logo.is_file() {
        return Err(format!("missing logo {}", brand.logo.display()));
    }
    Ok(brand)
}

fn required(
    fields: &std::collections::BTreeMap<String, String>,
    key: &str,
) -> Result<String, String> {
    fields
        .get(key)
        .filter(|value| !value.is_empty())
        .cloned()
        .ok_or_else(|| format!("{key} is empty"))
}

fn optional(
    fields: &std::collections::BTreeMap<String, String>,
    key: &str,
    default: &str,
) -> String {
    fields
        .get(key)
        .filter(|value| !value.is_empty())
        .cloned()
        .unwrap_or_else(|| default.to_owned())
}

fn validate(brand: &BrandManifest) -> Result<(), String> {
    if brand.id.chars().any(|ch| !is_id_char(ch)) {
        return Err(format!(
            "brand id {} is not a safe directory name",
            brand.id
        ));
    }
    if brand.product.chars().any(char::is_control) {
        return Err("product contains a control character".to_string());
    }
    if !valid_bundle_id(&brand.bundle_id) {
        return Err(format!(
            "bundle id {} is not a reverse-dns id",
            brand.bundle_id
        ));
    }
    for (name, value) in [
        ("qualifier_tld", brand.qualifier_tld.as_str()),
        ("qualifier_org", brand.qualifier_org.as_str()),
        ("qualifier_app", brand.qualifier_app.as_str()),
    ] {
        if value.chars().any(char::is_control) {
            return Err(format!("{name} contains a control character"));
        }
    }
    if !valid_scheme(&brand.url_scheme) {
        return Err(format!(
            "url scheme {} is not a plain scheme",
            brand.url_scheme
        ));
    }
    for (name, value) in [
        ("ota_github_repo", brand.ota_github_repo.as_str()),
        ("ota_modelscope_cn_repo", brand.ota_modelscope_cn_repo.as_str()),
        ("ota_modelscope_ai_repo", brand.ota_modelscope_ai_repo.as_str()),
        ("ota_prefix", brand.ota_prefix.as_str()),
    ] {
        if value.chars().any(char::is_control) || value.contains(' ') {
            return Err(format!("{name} is invalid"));
        }
    }
    if brand.ota_pubkey.chars().any(char::is_control) {
        return Err("ota_pubkey contains a control character".to_string());
    }
    Ok(())
}

fn valid_bundle_id(value: &str) -> bool {
    value.contains('.')
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '.' || ch == '-')
        && !value.starts_with('.')
        && !value.ends_with('.')
        && !value.contains("..")
}

fn valid_scheme(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_lowercase()
        && chars.all(|ch| {
            ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-'
        })
}

const fn is_id_char(ch: char) -> bool {
    ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_'
}

fn parse_fields(
    text: &str,
) -> Result<std::collections::BTreeMap<String, String>, String> {
    let mut fields = std::collections::BTreeMap::new();
    for (index, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(format!(
                "line {}: expected key = \"value\"",
                index + 1
            ));
        };
        let key = key.trim();
        let value = unquote(value.trim())
            .map_err(|error| format!("line {}: {error}", index + 1))?;
        if key.is_empty() {
            return Err(format!("line {}: empty key", index + 1));
        }
        fields.insert(key.to_string(), value);
    }
    Ok(fields)
}

fn unquote(value: &str) -> Result<String, String> {
    let bytes = value.as_bytes();
    if bytes.len() < 2 || bytes[0] != b'"' || bytes[bytes.len() - 1] != b'"' {
        return Err("value must be a quoted string".to_string());
    }
    let inner = &value[1..value.len() - 1];
    if inner.contains(['\n', '"', '\\']) {
        return Err("value cannot contain quotes or backslashes".to_string());
    }
    Ok(inner.to_string())
}

#[cfg(test)]
mod tests {
    use super::load;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn official_brand_loads() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|path| path.parent())
            .expect("workspace root")
            .join("brands/flowy");
        let brand = load(&dir).expect("official brand");
        assert_eq!(brand.id, "flowy");
        assert_eq!(brand.bundle_id, "com.Flowy.Flowy");
        assert_eq!(brand.product, "Flowy");
        assert_eq!(brand.qualifier_tld, "com");
        assert_eq!(brand.qualifier_org, "Flowy");
        assert_eq!(brand.qualifier_app, "Flowy");
        assert_eq!(brand.url_scheme, "flowy");
        assert_eq!(brand.ota_prefix, "allo");
        assert_eq!(brand.ota_github_repo, "Michael-Lfx/allo");
        assert!(brand.logo.is_file());
        assert!(brand.icon.is_file());
    }

    #[test]
    fn missing_directory_fails() {
        let error =
            load(std::path::Path::new("/tmp/flowy-brand-missing")).unwrap_err();
        assert!(error.contains("not a brand directory"));
    }

    #[test]
    fn empty_bundle_id_fails() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("flowy-brand-{nanos}-{id}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("brand.toml"),
            "id = \"demo\"\nproduct = \"Demo\"\nbundle_id = \"\"\n",
        )
        .unwrap();
        let error = load(&dir).unwrap_err();
        assert!(error.contains("bundle_id"));
        let _ = fs::remove_dir_all(dir);
    }
}
