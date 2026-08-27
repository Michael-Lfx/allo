//! Novel chunk retrieval — BM25 + optional Flowy embeddings (cosine top-k).
//!
//! ViMax used FAISS + Silicon BGE reranker. Under Flowy-only constraints we:
//! 1. Prefer `/embeddings` cosine retrieval when the server supports it
//!    (chunk vectors cached per novel chunk-set hash; query embedded per call)
//! 2. Otherwise Okapi BM25 with CJK overlapping 2-gram tokenization

use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};

use sha2::{Digest, Sha256};

use crate::backends::FlowyVimaxServices;
use crate::error::VimaxResult;
use crate::planning::is_cjk_speech_char;

/// LLM chat rerank is opt-in only; default path uses cosine / BM25 top-k.
pub const LLM_RERANK: bool = false;

static CHUNK_EMBED_CACHE: LazyLock<Mutex<HashMap<String, Vec<Vec<f32>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn cmp_f32_desc(a: f32, b: f32) -> std::cmp::Ordering {
    b.partial_cmp(&a).unwrap_or(std::cmp::Ordering::Less)
}

fn chunks_content_hash(chunks: &[String]) -> String {
    let joined = chunks.join("\n");
    let mut hasher = Sha256::new();
    hasher.update(joined.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Retrieve top chunks for an event query.
pub async fn retrieve_relevant_chunks(
    _chat: &std::sync::Arc<dyn crate::backends::VimaxChat>,
    flowy: Option<&FlowyVimaxServices>,
    query: &str,
    chunks: &[String],
    top_k: usize,
) -> VimaxResult<Vec<String>> {
    if chunks.is_empty() || top_k == 0 {
        return Ok(Vec::new());
    }
    let candidate_n = (top_k * 3).clamp(top_k, chunks.len().min(12));

    let shortlist: Vec<(usize, f32)> = match try_embed_rank(flowy, query, chunks, candidate_n).await {
        Some(ranked) if !ranked.is_empty() => ranked,
        _ => bm25_rank(query, chunks, candidate_n),
    };

    Ok(shortlist
        .into_iter()
        .take(top_k)
        .map(|(i, _)| chunks[i].clone())
        .collect())
}

async fn try_embed_rank(
    flowy: Option<&FlowyVimaxServices>,
    query: &str,
    chunks: &[String],
    top_n: usize,
) -> Option<Vec<(usize, f32)>> {
    let services = flowy?;
    let cache_key = chunks_content_hash(chunks);
    let chunk_vectors = {
        let cache = CHUNK_EMBED_CACHE.lock().ok()?;
        cache.get(&cache_key).cloned()
    };
    let chunk_vectors = match chunk_vectors {
        Some(v) if v.len() == chunks.len() => v,
        _ => {
            let vectors = services
                .api
                .embeddings(&services.session, chunks, None)
                .await
                .ok()?;
            if vectors.len() != chunks.len() {
                return None;
            }
            if let Ok(mut cache) = CHUNK_EMBED_CACHE.lock() {
                cache.insert(cache_key, vectors.clone());
            }
            vectors
        }
    };
    let q_vec = services
        .api
        .embeddings(&services.session, &[query.to_string()], None)
        .await
        .ok()?
        .into_iter()
        .next()?;
    let mut scored: Vec<(usize, f32)> = chunk_vectors
        .iter()
        .enumerate()
        .map(|(i, v)| (i, cosine(&q_vec, v)))
        .collect();
    scored.sort_by(|a, b| cmp_f32_desc(a.1, b.1));
    scored.truncate(top_n);
    Some(scored)
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len().min(b.len());
    if n == 0 {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for i in 0..n {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    let denom = na.sqrt() * nb.sqrt();
    if denom < f32::EPSILON {
        0.0
    } else {
        dot / denom
    }
}

/// Okapi BM25 ranking.
pub fn bm25_rank(query: &str, chunks: &[String], top_n: usize) -> Vec<(usize, f32)> {
    let k1 = 1.5f32;
    let b = 0.75f32;
    let docs: Vec<Vec<String>> = chunks.iter().map(|c| tokenize(c)).collect();
    let avgdl = if docs.is_empty() {
        0.0
    } else {
        docs.iter().map(|d| d.len() as f32).sum::<f32>() / docs.len() as f32
    };
    let mut df: HashMap<String, usize> = HashMap::new();
    for doc in &docs {
        let mut seen = HashSet::new();
        for t in doc {
            if seen.insert(t.clone()) {
                *df.entry(t.clone()).or_default() += 1;
            }
        }
    }
    let n = docs.len() as f32;
    let q_tokens = tokenize(query);
    let mut scored: Vec<(usize, f32)> = docs
        .iter()
        .enumerate()
        .map(|(i, doc)| {
            let mut tf: HashMap<&str, f32> = HashMap::new();
            for t in doc {
                *tf.entry(t.as_str()).or_default() += 1.0;
            }
            let dl = doc.len() as f32;
            let mut score = 0.0f32;
            for qt in &q_tokens {
                let f = *tf.get(qt.as_str()).unwrap_or(&0.0);
                if f <= 0.0 {
                    continue;
                }
                let ni = *df.get(qt).unwrap_or(&0) as f32;
                let idf = ((n - ni + 0.5) / (ni + 0.5) + 1.0).ln().max(0.0);
                let denom = f + k1 * (1.0 - b + b * dl / avgdl.max(1.0));
                score += idf * (f * (k1 + 1.0)) / denom;
            }
            (i, score)
        })
        .collect();
    scored.sort_by(|a, b| cmp_f32_desc(a.1, b.1));
    scored.truncate(top_n);
    scored
}

pub(crate) fn tokenize(s: &str) -> Vec<String> {
    let lower = s.to_lowercase();
    let chars: Vec<char> = lower.chars().collect();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < chars.len() {
        if is_cjk_char(chars[i]) {
            let run_start = i;
            while i < chars.len() && is_cjk_char(chars[i]) {
                i += 1;
            }
            let run: Vec<char> = chars[run_start..i].to_vec();
            if run.len() == 1 {
                out.push(run[0].to_string());
            } else {
                for j in 0..run.len().saturating_sub(1) {
                    out.push(format!("{}{}", run[j], run[j + 1]));
                }
            }
        } else if chars[i].is_ascii_alphanumeric() {
            let start = i;
            i += 1;
            while i < chars.len() && chars[i].is_ascii_alphanumeric() {
                i += 1;
            }
            let token: String = chars[start..i].iter().collect();
            if token.len() > 1 {
                out.push(token);
            }
        } else {
            i += 1;
        }
    }
    out
}

fn is_cjk_char(ch: char) -> bool {
    is_cjk_speech_char(ch) || ('\u{4E00}'..='\u{9FFF}').contains(&ch)
}

/// Back-compat helper.
pub fn rank_chunks_by_keyword_overlap(query: &str, chunks: &[String], top_k: usize) -> Vec<String> {
    bm25_rank(query, chunks, top_k)
        .into_iter()
        .map(|(i, _)| chunks[i].clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bm25_chinese_query_matches_bigram_chunk() {
        let chunks = vec!["张三走进客栈，要了一碗面。".to_string()];
        let ranked = bm25_rank("张三", &chunks, 1);
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].0, 0);
        assert!(ranked[0].1 > 0.0);
    }

    #[test]
    fn cjk_tokenize_uses_overlapping_bigrams() {
        let tokens = tokenize("张三走进");
        assert!(tokens.contains(&"张三".to_string()));
        assert!(tokens.contains(&"三走".to_string()));
        assert!(tokens.contains(&"走进".to_string()));
    }

    #[test]
    fn llm_rerank_disabled_by_default() {
        assert!(!LLM_RERANK);
    }
}
