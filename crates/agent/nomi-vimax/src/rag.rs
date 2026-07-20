//! Novel chunk retrieval ? BM25 + optional Flowy embeddings + LLM rerank.
//!
//! ViMax used FAISS + Silicon BGE reranker. Under Flowy-only constraints we:
//! 1. Prefer `/embeddings` cosine retrieval when the server supports it
//! 2. Otherwise Okapi BM25 (stronger than bare keyword overlap)
//! 3. LLM-rerank the shortlist (same role as the dedicated reranker)

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde::Deserialize;

use crate::backends::{FlowyVimaxServices, VimaxChat};
use crate::error::VimaxResult;
use crate::json_util::parse_llm_json;

fn cmp_f32_desc(a: f32, b: f32) -> std::cmp::Ordering {
    b.partial_cmp(&a).unwrap_or(std::cmp::Ordering::Less)
}

/// Retrieve top chunks for an event query.
pub async fn retrieve_relevant_chunks(
    chat: &Arc<dyn VimaxChat>,
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

    let candidates: Vec<(usize, &str)> = shortlist
        .iter()
        .map(|(i, _)| (*i, chunks[*i].as_str()))
        .collect();

    match llm_rerank(chat, query, &candidates, top_k).await {
        Ok(idxs) if !idxs.is_empty() => Ok(idxs
            .into_iter()
            .filter_map(|i| chunks.get(i).cloned())
            .collect()),
        _ => Ok(shortlist
            .into_iter()
            .take(top_k)
            .map(|(i, _)| chunks[i].clone())
            .collect()),
    }
}

async fn try_embed_rank(
    flowy: Option<&FlowyVimaxServices>,
    query: &str,
    chunks: &[String],
    top_n: usize,
) -> Option<Vec<(usize, f32)>> {
    let services = flowy?;
    let mut inputs = Vec::with_capacity(chunks.len() + 1);
    inputs.push(query.to_string());
    inputs.extend(chunks.iter().cloned());
    let vectors = services
        .api
        .embeddings(&services.session, &inputs, None)
        .await
        .ok()?;
    if vectors.len() != chunks.len() + 1 {
        return None;
    }
    let q = &vectors[0];
    let mut scored: Vec<(usize, f32)> = vectors[1..]
        .iter()
        .enumerate()
        .map(|(i, v)| (i, cosine(q, v)))
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

async fn llm_rerank(
    chat: &Arc<dyn VimaxChat>,
    query: &str,
    candidates: &[(usize, &str)],
    top_k: usize,
) -> VimaxResult<Vec<usize>> {
    if candidates.is_empty() {
        return Ok(Vec::new());
    }
    let mut body = String::from("Passages:\n");
    for (rank, (orig_idx, text)) in candidates.iter().enumerate() {
        let preview: String = text.chars().take(600).collect();
        body.push_str(&format!("[{rank}] (orig={orig_idx})\n{preview}\n---\n"));
    }
    let system = format!(
        "You rank passages for novel-to-video retrieval. \
Return ONLY JSON: {{\"indices\":[0,2,...]}} with up to {top_k} zero-based ranks \
from the Passages list (the [rank] numbers), most relevant first. \
Drop passages with relevance below ~0.7."
    );
    let user = format!("Query:\n{query}\n\n{body}");
    let raw = chat.complete_text(&system, &user).await?;
    #[derive(Deserialize)]
    struct Resp {
        indices: Vec<usize>,
    }
    let resp: Resp = parse_llm_json(&raw)?;
    let mut out = Vec::new();
    for rank in resp.indices {
        if let Some((orig, _)) = candidates.get(rank)
            && !out.contains(orig)
        {
            out.push(*orig);
        }
        if out.len() >= top_k {
            break;
        }
    }
    Ok(out)
}

fn tokenize(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() > 1)
        .map(|t| t.to_string())
        .collect()
}

/// Back-compat helper.
pub fn rank_chunks_by_keyword_overlap(query: &str, chunks: &[String], top_k: usize) -> Vec<String> {
    bm25_rank(query, chunks, top_k)
        .into_iter()
        .map(|(i, _)| chunks[i].clone())
        .collect()
}
