use std::sync::Arc;

use serde_json::json;

use crate::backends::VimaxChat;
use crate::error::VimaxResult;
use crate::progress::ProgressCallback;

pub struct NovelCompressor {
    chat: Arc<dyn VimaxChat>,
    chunk_size: usize,
    chunk_overlap: usize,
}

impl NovelCompressor {
    pub fn new(chat: Arc<dyn VimaxChat>) -> Self {
        Self {
            chat,
            chunk_size: 4000,
            chunk_overlap: 400,
        }
    }

    pub fn split(&self, novel_text: &str) -> Vec<String> {
        let chars: Vec<char> = novel_text.chars().collect();
        if chars.is_empty() {
            return vec![];
        }
        let mut chunks = Vec::new();
        let mut start = 0;
        while start < chars.len() {
            let end = (start + self.chunk_size).min(chars.len());
            chunks.push(chars[start..end].iter().collect());
            if end >= chars.len() {
                break;
            }
            start = end.saturating_sub(self.chunk_overlap);
            if start >= end {
                start = end;
            }
        }
        chunks
    }

    pub async fn compress_chunk(&self, novel_chunk: &str) -> VimaxResult<String> {
        let system = include_str!(
            "../../prompts/novel_compressor__system_prompt_template_compress_novel_chunk.txt"
        );
        let user = include_str!(
            "../../prompts/novel_compressor__human_prompt_template_compress_novel_chunk.txt"
        )
        .replace("{novel_chunk}", novel_chunk);
        self.chat.complete_text(system, &user).await
    }

    pub async fn aggregate(&self, compressed_chunks: &[String]) -> VimaxResult<String> {
        let mut chunks_blob = String::new();
        for (i, c) in compressed_chunks.iter().enumerate() {
            chunks_blob.push_str(&format!("<CHUNK_{i}_START>\n{c}\n<CHUNK_{i}_END>\n\n"));
        }
        let system =
            include_str!("../../prompts/novel_compressor__system_prompt_template_aggregate.txt");
        let user = include_str!(
            "../../prompts/novel_compressor__human_prompt_template_aggregate.txt"
        )
        .replace("{chunks}", &chunks_blob);
        self.chat.complete_text(system, &user).await
    }

    pub async fn compress_novel(
        &self,
        novel_text: &str,
        progress: Option<&ProgressCallback>,
    ) -> VimaxResult<(Vec<String>, String)> {
        let chunks = self.split(novel_text);
        let total = chunks.len().max(1);
        if let Some(cb) = progress {
            cb(
                "compress_novel",
                &format!("开始压缩小说：共 {total} 个分片"),
                Some(json!({ "progress": 5.0, "done": 0, "total": total })),
            );
        }

        let mut set = tokio::task::JoinSet::new();
        let sem = Arc::new(tokio::sync::Semaphore::new(5));
        for (i, chunk) in chunks.iter().enumerate() {
            let chat = Arc::clone(&self.chat);
            let chunk = chunk.clone();
            let permit = Arc::clone(&sem);
            set.spawn(async move {
                let _permit = permit
                    .acquire()
                    .await
                    .map_err(|_| crate::error::VimaxError::msg("semaphore closed"))?;
                let compressor = NovelCompressor {
                    chat,
                    chunk_size: 4000,
                    chunk_overlap: 400,
                };
                let out = compressor.compress_chunk(&chunk).await?;
                Ok::<_, crate::error::VimaxError>((i, out))
            });
        }

        let mut compressed_pairs = Vec::new();
        let mut done = 0usize;
        while let Some(joined) = set.join_next().await {
            compressed_pairs
                .push(joined.map_err(|e| crate::error::VimaxError::msg(e.to_string()))??);
            done += 1;
            if let Some(cb) = progress {
                let pct = 5.0 + 45.0 * (done as f64 / total as f64);
                cb(
                    "compress_novel",
                    &format!("正在压缩小说分片（{done}/{total}）"),
                    Some(json!({ "progress": pct, "done": done, "total": total })),
                );
            }
        }
        compressed_pairs.sort_by_key(|(i, _)| *i);
        let compressed: Vec<String> = compressed_pairs.into_iter().map(|(_, c)| c).collect();

        if compressed.len() > 1 {
            if let Some(cb) = progress {
                cb(
                    "compress_aggregate",
                    "正在汇总压缩后的小说分片",
                    Some(json!({ "progress": 52.0 })),
                );
            }
        }

        let aggregated = if compressed.len() == 1 {
            compressed[0].clone()
        } else {
            self.aggregate(&compressed).await?
        };

        if let Some(cb) = progress {
            cb(
                "compress_novel",
                "小说压缩完成",
                Some(json!({ "progress": 55.0 })),
            );
        }
        Ok((chunks, aggregated))
    }
}
