//! Quick-create activation path: sample seed, optional bind, suggest prompt.

use nomifun_api_types::{KnowledgeSource, KnowledgeSourceEntry, KnowledgeSourceMode};
use nomifun_common::{AppError, KnowledgeBaseId};
use serde::Serialize;

use crate::service::{KnowledgeBaseInfo, KnowledgeBinding, KnowledgeService};

pub const SAMPLE_SEED_NAME: &str = "Demo Knowledge";
pub const SAMPLE_SEED_DESCRIPTION: &str =
    "A ready-to-try knowledge base with product FAQ and style notes. Mount it, then ask a question.";

const SAMPLE_README: &str = r#"# Demo Knowledge

This sample library shows how Flowy knowledge works.

1. Mount this base into a conversation.
2. Ask a question about the product FAQ or writing style.
3. Watch the grounded search chip light up when the agent retrieves a hit.

## Contents

- `PRODUCT_FAQ.md` — common product questions
- `STYLE_GUIDE.md` — short writing conventions
"#;

const SAMPLE_FAQ: &str = r#"# Product FAQ

## What is a knowledge base?

A knowledge base is a Markdown directory that agents can search while answering you.

## How do I make an agent use it?

Mount the base onto a conversation or workpath. The agent then calls knowledge search when your question matches the description.

## Can the agent write back?

Yes. When write-back is enabled, the agent can persist durable notes into the library (manual disposition waits for an explicit ask; auto disposition decides on its own).

## What formats are supported?

Markdown (`.md`) files. Drop files into the folder or upload them from the detail page.
"#;

const SAMPLE_STYLE: &str = r#"# Style Guide

## Tone

Be direct, concrete, and short. Prefer one idea per sentence.

## Structure

- Lead with the answer
- Follow with one example when helpful
- Avoid filler openers

## Naming

Use clear nouns. Prefer "knowledge base" over vague "docs" when referring to mounted libraries.
"#;

#[derive(Debug, Clone, Serialize)]
pub struct QuickCreateOutcome {
    pub base: KnowledgeBaseInfo,
    pub suggest_prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub binding: Option<KnowledgeBinding>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SuggestPromptOutcome {
    pub prompt: String,
}

impl KnowledgeService {
    /// One-shot activation create: blank / sample / local / web + optional bind.
    pub async fn quick_create_base(
        &self,
        name: Option<&str>,
        description: Option<&str>,
        seed: &str,
        root_path: Option<&str>,
        url: Option<&str>,
        bind_kind: Option<&str>,
        bind_target_id: Option<&str>,
    ) -> Result<QuickCreateOutcome, AppError> {
        let seed = seed.trim().to_ascii_lowercase();
        let (resolved_name, resolved_desc, resolved_root, source, write_sample) = match seed.as_str() {
            "sample" => (
                name.unwrap_or(SAMPLE_SEED_NAME).trim().to_owned(),
                description.unwrap_or(SAMPLE_SEED_DESCRIPTION).trim().to_owned(),
                None,
                None,
                true,
            ),
            "blank" => (
                name.unwrap_or("Untitled Knowledge").trim().to_owned(),
                description.unwrap_or("").trim().to_owned(),
                None,
                None,
                false,
            ),
            "local" => {
                let path = root_path
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| AppError::BadRequest("root_path is required for local seed".into()))?;
                let folder_name = std::path::Path::new(path)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("Local Knowledge");
                (
                    name.unwrap_or(folder_name).trim().to_owned(),
                    description.unwrap_or("Local directory knowledge base").trim().to_owned(),
                    Some(path.to_owned()),
                    None,
                    false,
                )
            }
            "web" => {
                let url = url
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| AppError::BadRequest("url is required for web seed".into()))?;
                let source = KnowledgeSource {
                    kind: "url".into(),
                    mode: KnowledgeSourceMode::Snapshot,
                    entries: vec![KnowledgeSourceEntry {
                        url: url.to_owned(),
                        title: None,
                        rendered: false,
                    }],
                    last_fetched_at: None,
                };
                (
                    name.unwrap_or("Web Knowledge").trim().to_owned(),
                    description
                        .unwrap_or("Captured from a web page for agent grounding")
                        .trim()
                        .to_owned(),
                    None,
                    Some(source),
                    false,
                )
            }
            other => {
                return Err(AppError::BadRequest(format!(
                    "unsupported quick seed: {other} (expected sample|blank|local|web)"
                )));
            }
        };

        if resolved_name.is_empty() {
            return Err(AppError::BadRequest("knowledge base name must not be empty".into()));
        }

        let mut info = self
            .create_base(
                &resolved_name,
                &resolved_desc,
                resolved_root.as_deref(),
                source,
            )
            .await?;

        if write_sample {
            self.write_file(info.knowledge_base_id.as_str(), "README.md", SAMPLE_README)
                .await?;
            self.write_file(info.knowledge_base_id.as_str(), "PRODUCT_FAQ.md", SAMPLE_FAQ)
                .await?;
            self.write_file(info.knowledge_base_id.as_str(), "STYLE_GUIDE.md", SAMPLE_STYLE)
                .await?;
            info = self.get_base_info(info.knowledge_base_id.as_str()).await?;
        }

        let binding = if let (Some(kind), Some(target)) = (bind_kind, bind_target_id) {
            let kind = kind.trim();
            let target = target.trim();
            if kind.is_empty() || target.is_empty() {
                None
            } else {
                Some(
                    self.mount_base_onto_target(kind, target, &info.knowledge_base_id)
                        .await?,
                )
            }
        } else {
            None
        };

        let suggest_prompt = self
            .suggest_prompt_for_base(info.knowledge_base_id.as_str())
            .await
            .unwrap_or_else(|_| default_suggest_prompt(&info.name));

        Ok(QuickCreateOutcome {
            base: info,
            suggest_prompt,
            binding,
        })
    }

    /// Merge a base onto an existing binding target and enable mounting.
    pub async fn mount_base_onto_target(
        &self,
        kind: &str,
        target_id: &str,
        kb_id: &KnowledgeBaseId,
    ) -> Result<KnowledgeBinding, AppError> {
        let mut binding = self.get_binding(kind, target_id).await?;
        if !binding.kb_ids.iter().any(|id| id == kb_id) {
            binding.kb_ids.push(kb_id.clone());
        }
        binding.enabled = true;
        if binding.writeback_eagerness.trim().is_empty() {
            binding.writeback_eagerness = "manual".into();
        }
        self.set_binding(kind, target_id, binding).await
    }

    pub async fn suggest_prompt_for_base(&self, id: &str) -> Result<String, AppError> {
        let info = self.get_base_info(id).await?;
        let files = self.list_files(id).await?;
        if files.iter().any(|f| f.rel_path.eq_ignore_ascii_case("PRODUCT_FAQ.md")) {
            return Ok(
                "请先调用 knowledge_search 检索已挂载的知识库（查询「知识库」或「FAQ」），再根据命中内容回答：知识库是什么？怎样让 Agent 使用它？"
                    .to_owned(),
            );
        }
        if let Some(first) = files.first() {
            let stem = first
                .rel_path
                .rsplit('/')
                .next()
                .unwrap_or(first.rel_path.as_str())
                .trim_end_matches(".md");
            return Ok(format!(
                "请先调用 knowledge_search 检索已挂载的「{}」知识库（查询「{}」），再根据命中内容总结最重要的三点。",
                info.name, stem
            ));
        }
        Ok(default_suggest_prompt(&info.name))
    }

    pub async fn upload_files_batch(
        &self,
        id: &str,
        files: &[(String, String)],
    ) -> Result<usize, AppError> {
        let mut written = 0usize;
        for (path, content) in files {
            let path = path.trim();
            if path.is_empty() {
                continue;
            }
            self.write_file(id, path, content).await?;
            written += 1;
        }
        Ok(written)
    }
}

fn default_suggest_prompt(name: &str) -> String {
    format!(
        "请先调用 knowledge_search 检索已挂载的「{name}」知识库，再根据命中内容总结最重要的三点。"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::make_service;

    #[tokio::test]
    async fn quick_sample_seeds_markdown_and_suggests_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let service = make_service(&dir.path().join("data"));
        let outcome = service
            .quick_create_base(None, None, "sample", None, None, None, None)
            .await
            .expect("quick sample");
        assert_eq!(outcome.base.name, SAMPLE_SEED_NAME);
        assert!(outcome.base.file_count >= 3);
        assert!(outcome.suggest_prompt.contains("FAQ") || outcome.suggest_prompt.contains("知识库"));
        let files = service
            .list_files(outcome.base.knowledge_base_id.as_str())
            .await
            .unwrap();
        assert!(files.iter().any(|f| f.rel_path == "PRODUCT_FAQ.md"));
    }
}
