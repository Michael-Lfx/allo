//! Curated language / technology term catalogs for rule-based POI extraction.

use super::store::InterestSignal;
use super::types::SignalSource;

/// ASCII language names → canonical lang id.
const LANG_TERMS: &[(&str, &str)] = &[
    ("rust", "rust"),
    ("python", "python"),
    ("typescript", "typescript"),
    ("javascript", "javascript"),
    ("golang", "go"),
    ("go", "go"),
    ("java", "java"),
    ("kotlin", "kotlin"),
    ("swift", "swift"),
    ("ruby", "ruby"),
    ("php", "php"),
    ("scala", "scala"),
    ("csharp", "csharp"),
    ("c#", "csharp"),
    ("cpp", "cpp"),
    ("c++", "cpp"),
    ("sql", "sql"),
    ("lua", "lua"),
    ("zig", "zig"),
    ("elixir", "elixir"),
    ("haskell", "haskell"),
    ("bash", "bash"),
    ("shell", "shell"),
    ("wasm", "wasm"),
    ("dart", "dart"),
    ("r", "r"),
];

/// ASCII stack / product terms (must not appear in POI blocklist).
const TECH_TERMS: &[&str] = &[
    "hermes",
    "parity",
    "mcp",
    "sqlite",
    "postgres",
    "postgresql",
    "mysql",
    "redis",
    "mongodb",
    "kafka",
    "rabbitmq",
    "tokio",
    "docker",
    "podman",
    "kubernetes",
    "k8s",
    "helm",
    "terraform",
    "ansible",
    "nginx",
    "grpc",
    "graphql",
    "websocket",
    "anthropic",
    "claude",
    "openai",
    "gemini",
    "deepseek",
    "ollama",
    "llama",
    "llm",
    "langchain",
    "pytorch",
    "tensorflow",
    "huggingface",
    "cuda",
    "nvidia",
    "react",
    "vue",
    "nextjs",
    "nuxt",
    "svelte",
    "angular",
    "webpack",
    "vite",
    "tailwind",
    "django",
    "fastapi",
    "flask",
    "rails",
    "spring",
    "nestjs",
    "express",
    "electron",
    "tauri",
    "linux",
    "ubuntu",
    "debian",
    "aws",
    "azure",
    "gcp",
    "cloudflare",
    "github",
    "gitlab",
    "bitbucket",
    "jenkins",
    "prometheus",
    "grafana",
    "elasticsearch",
    "clickhouse",
    "duckdb",
    "snowflake",
    "spark",
    "airflow",
    "dbt",
    "supabase",
    "firebase",
    "vercel",
    "netlify",
    "figma",
    "notion",
    "obsidian",
    "vim",
    "neovim",
    "emacs",
    "vscode",
    "cursor",
    "copilot",
    "codex",
    "bedrock",
    "openrouter",
    "litellm",
    "vllm",
    "gguf",
    "rag",
    "embedding",
    "chromadb",
    "qdrant",
    "milvus",
    "weaviate",
    "pinecone",
    "blockchain",
    "ethereum",
    "solidity",
    "bitcoin",
    "ios",
    "android",
    "flutter",
    "react-native",
    "unity",
    "unreal",
    "blender",
    "ffmpeg",
    "opencv",
    "pandas",
    "numpy",
    "sklearn",
    "jupyter",
    "webassembly",
    "oauth",
    "jwt",
    "saml",
    "ldap",
];

/// Chinese surface forms → canonical tech id (ASCII slug).
const ZH_TECH_TERMS: &[(&str, &str)] = &[
    ("大模型", "llm"),
    ("机器学习", "machine-learning"),
    ("深度学习", "deep-learning"),
    ("区块链", "blockchain"),
    ("智能合约", "smart-contract"),
    ("微服务", "microservices"),
    ("全栈", "fullstack"),
    ("前端", "frontend"),
    ("后端", "backend"),
    ("运维", "devops"),
    ("数据库", "database"),
    ("向量数据库", "vector-db"),
    ("知识库", "knowledge-base"),
    ("检索增强", "rag"),
    ("嵌入式", "embedded"),
    ("物联网", "iot"),
    ("自动驾驶", "autonomous-driving"),
    ("计算机视觉", "computer-vision"),
    ("自然语言处理", "nlp"),
    ("推荐系统", "recommender"),
    ("量化交易", "quant-trading"),
    ("游戏开发", "game-dev"),
    ("音视频", "media-processing"),
    ("云原生", "cloud-native"),
    ("容器化", "containerization"),
    ("持续集成", "ci"),
    ("持续交付", "cd"),
];

/// Chinese language names → canonical lang id.
const ZH_LANG_TERMS: &[(&str, &str)] = &[
    ("Rust", "rust"),
    ("Python", "python"),
    ("TypeScript", "typescript"),
    ("JavaScript", "javascript"),
    ("Go语言", "go"),
    ("Golang", "go"),
    ("Java", "java"),
    ("Kotlin", "kotlin"),
    ("Swift", "swift"),
    ("Ruby", "ruby"),
    ("PHP", "php"),
    ("Scala", "scala"),
    ("C++", "cpp"),
    ("C语言", "c"),
    ("SQL", "sql"),
    ("Lua", "lua"),
    ("Zig", "zig"),
];

pub fn scan_lang_signals(text: &str, weight_scale: f64) -> Vec<InterestSignal> {
    let mut out = Vec::new();
    let lower = text.to_ascii_lowercase();

    for (term, id) in LANG_TERMS {
        if contains_ascii_term(&lower, term) {
            push_lang(&mut out, id, weight_scale);
        }
    }

    for (term, id) in ZH_LANG_TERMS {
        if text.contains(term) {
            push_lang(&mut out, id, weight_scale);
        }
    }

    dedupe_by_id(out)
}

pub fn scan_tech_signals(text: &str, weight_scale: f64) -> Vec<InterestSignal> {
    let mut out = Vec::new();
    let lower = text.to_ascii_lowercase();

    for term in TECH_TERMS {
        if contains_ascii_term(&lower, term) {
            push_tech(&mut out, term, weight_scale);
        }
    }

    for (surface, id) in ZH_TECH_TERMS {
        if text.contains(surface) {
            push_tech(&mut out, id, weight_scale);
        }
    }

    dedupe_by_id(out)
}

fn contains_ascii_term(haystack_lower: &str, term: &str) -> bool {
    let term = term.to_ascii_lowercase();
    if term.len() <= 2 && term != "go" && term != "r" && term != "ci" && term != "cd" {
        return false;
    }
    if term.contains('.') || term.contains('#') || term.contains('+') {
        return haystack_lower.contains(term.as_str());
    }
    haystack_lower
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .any(|tok| tok == term.as_str())
        || haystack_lower.contains(&format!(" {term} "))
        || haystack_lower.starts_with(&format!("{term} "))
        || haystack_lower.ends_with(&format!(" {term}"))
}

fn push_lang(out: &mut Vec<InterestSignal>, id: &str, weight_scale: f64) {
    out.push(InterestSignal::new(
        format!("lang:{id}"),
        format!("language: {id}"),
        format!("Discussed {id} development"),
        0.2 * weight_scale,
        vec!["lang".to_string(), id.to_string()],
        SignalSource::Lang,
    ));
}

fn push_tech(out: &mut Vec<InterestSignal>, canonical: &str, weight_scale: f64) {
    let id = canonical.to_ascii_lowercase();
    out.push(InterestSignal::new(
        format!("tech:{id}"),
        format!("topic: {id}"),
        format!("Recurring interest in {id}"),
        0.18 * weight_scale,
        vec!["tech".to_string(), id.clone()],
        SignalSource::Tech,
    ));
}

fn dedupe_by_id(signals: Vec<InterestSignal>) -> Vec<InterestSignal> {
    let mut seen = std::collections::HashSet::new();
    signals
        .into_iter()
        .filter(|s| seen.insert(s.id.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_chinese_tech_and_lang() {
        let langs = scan_lang_signals("我在用 Rust 和 Python 写工具", 1.0);
        assert!(langs.iter().any(|s| s.id == "lang:rust"));
        assert!(langs.iter().any(|s| s.id == "lang:python"));

        let tech = scan_tech_signals("最近在研究大模型和向量数据库", 1.0);
        assert!(tech.iter().any(|s| s.id == "tech:llm"));
        assert!(tech.iter().any(|s| s.id == "tech:vector-db"));
    }

    #[test]
    fn detects_expanded_stack_terms() {
        let tech = scan_tech_signals("deploy with docker kubernetes and redis", 1.0);
        let ids: std::collections::HashSet<_> = tech.iter().map(|s| s.id.as_str()).collect();
        assert!(ids.contains("tech:docker"));
        assert!(ids.contains("tech:kubernetes"));
        assert!(ids.contains("tech:redis"));
    }
}
