//! Novel2Video — compress → events → keyword RAG → scenes → Script2Video per scene.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::agents::{
    EventExtractor, GlobalInformationPlanner, NovelCompressor, SceneExtractor,
};
use crate::domain::{CharacterInNovel, Event, Scene};
use crate::error::VimaxResult;
use crate::media_local;
use crate::progress::ProgressCallback;
use crate::rag;
use crate::session::{write_json_artifact, write_text_artifact};

use super::script2video::Script2VideoPipeline;
use super::{PipelineBackends, emit, emit_pct, load_or_write_text};

pub struct Novel2VideoPipeline {
    backends: PipelineBackends,
    working_dir: PathBuf,
    compressor: NovelCompressor,
    events: EventExtractor,
    scenes: SceneExtractor,
    global: GlobalInformationPlanner,
}

impl Novel2VideoPipeline {
    pub fn new(backends: PipelineBackends, working_dir: PathBuf) -> Self {
        Self {
            compressor: NovelCompressor::new(Arc::clone(&backends.chat)),
            events: EventExtractor::new(Arc::clone(&backends.chat)),
            scenes: SceneExtractor::new(Arc::clone(&backends.chat)),
            global: GlobalInformationPlanner::new(Arc::clone(&backends.chat)),
            backends,
            working_dir,
        }
    }

    pub async fn plan_text_artifacts(
        &self,
        novel_text: &str,
        user_requirement: &str,
        style: &str,
        progress: Option<ProgressCallback>,
    ) -> VimaxResult<()> {
        let _ = (user_requirement, style);
        let novel_dir = self.working_dir.join("novel");
        tokio::fs::create_dir_all(&novel_dir).await?;
        emit_pct(&progress, "save_novel", "正在保存并切分小说文本", 2.0);
        write_text_artifact(&novel_dir.join("novel.txt"), novel_text).await?;

        let chunks = self.compressor.split(novel_text);
        for (idx, chunk) in chunks.iter().enumerate() {
            write_text_artifact(
                &novel_dir.join(format!("novel_chunk_{idx}.txt")),
                chunk,
            )
            .await?;
        }

        emit_pct(
            &progress,
            "compress_novel",
            &format!("准备压缩小说（{} 个分片）", chunks.len()),
            5.0,
        );
        let progress_for_compress = progress.clone();
        let compressed = load_or_write_text(&novel_dir.join("novel_compressed.txt"), || async {
            let (_, agg) = self
                .compressor
                .compress_novel(novel_text, progress_for_compress.as_ref())
                .await?;
            Ok(agg)
        })
        .await?;

        emit_pct(&progress, "extract_events", "正在从压缩文本提取事件", 58.0);
        let events_dir = self.working_dir.join("events");
        tokio::fs::create_dir_all(&events_dir).await?;
        let events: Vec<Event> = {
            let list_path = events_dir.join("events.json");
            if list_path.exists() {
                serde_json::from_str(&tokio::fs::read_to_string(&list_path).await?)?
            } else {
                let events = self.events.extract_all(&compressed).await?;
                for e in &events {
                    write_json_artifact(
                        &events_dir.join(format!("event_{}.json", e.index)),
                        e,
                    )
                    .await?;
                }
                write_json_artifact(&list_path, &events).await?;
                events
            }
        };

        let event_count = events.len().max(1);
        let mut novel_chars: Vec<CharacterInNovel> = Vec::new();
        let gi_dir = self
            .working_dir
            .join("global_information")
            .join("characters");
        tokio::fs::create_dir_all(gi_dir.join("event_level")).await?;
        tokio::fs::create_dir_all(gi_dir.join("novel_level")).await?;

        for (event_i, event) in events.iter().enumerate() {
            let base = 60.0 + 35.0 * (event_i as f32 / event_count as f32);
            emit_pct(
                &progress,
                "event_rag",
                &format!(
                    "检索事件相关片段（事件 {}/{}）",
                    event.index,
                    event_count
                ),
                base,
            );
            // BM25 + optional Flowy embeddings + LLM rerank (ViMax FAISS+BGE analogue).
            let relevant = rag::retrieve_relevant_chunks(
                &self.backends.chat,
                self.backends.flowy.as_ref(),
                &event.description,
                &chunks,
                5,
            )
            .await?;
            let rel_dir = self
                .working_dir
                .join("relevant_chunks")
                .join(format!("event_{}", event.index));
            tokio::fs::create_dir_all(&rel_dir).await?;
            for (i, chunk) in relevant.iter().enumerate() {
                write_text_artifact(
                    &rel_dir.join(format!("chunk_{i}-score_rag.txt")),
                    chunk,
                )
                .await?;
            }

            emit_pct(
                &progress,
                "extract_scenes",
                &format!(
                    "提取场景（事件 {}/{}）",
                    event.index,
                    event_count
                ),
                base + 5.0,
            );
            let scenes_dir = self
                .working_dir
                .join("scenes")
                .join(format!("event_{}", event.index));
            tokio::fs::create_dir_all(&scenes_dir).await?;
            let scenes: Vec<Scene> = {
                let list = scenes_dir.join("scenes.json");
                if list.exists() {
                    serde_json::from_str(&tokio::fs::read_to_string(&list).await?)?
                } else {
                    let scenes = self
                        .scenes
                        .extract_all_for_event(event, &relevant)
                        .await?;
                    for s in &scenes {
                        write_json_artifact(
                            &scenes_dir.join(format!("scene_{}.json", s.index)),
                            s,
                        )
                        .await?;
                    }
                    write_json_artifact(&list, &scenes).await?;
                    scenes
                }
            };

            emit_pct(
                &progress,
                "merge_characters",
                &format!(
                    "合并角色信息（事件 {}/{}）",
                    event.index,
                    event_count
                ),
                base + 10.0,
            );
            let scene_char_pairs: Vec<(Scene, Vec<String>)> = scenes
                .iter()
                .map(|s| (s.clone(), s.characters.clone()))
                .collect();
            let event_chars = self
                .global
                .merge_characters_across_scenes_in_event(&scene_char_pairs)
                .await?;
            write_json_artifact(
                &gi_dir
                    .join("event_level")
                    .join(format!("event_{}_characters.json", event.index)),
                &event_chars,
            )
            .await?;

            novel_chars = self
                .global
                .merge_characters_to_existing_in_novel(&novel_chars, &event_chars, event.index)
                .await?;
            write_json_artifact(
                &gi_dir
                    .join("novel_level")
                    .join(format!("novel_characters_after_event_{}.json", event.index)),
                &novel_chars,
            )
            .await?;

            // Plan script2video text for each scene.
            let film_total = {
                let p = self.working_dir.join("target_duration_secs.txt");
                tokio::fs::read_to_string(&p)
                    .await
                    .ok()
                    .and_then(|t| t.trim().parse::<u32>().ok())
                    .filter(|&n| n > 0)
                    .map(|n| crate::planning::normalize_target_duration_secs(Some(n)))
                    .unwrap_or(crate::planning::DEFAULT_TARGET_DURATION_SECS)
            };
            let scene_n = scenes.len().max(1);
            let budgets = crate::planning::allocate_scene_budgets(film_total, scene_n);
            for (si, scene) in scenes.iter().enumerate() {
                emit_pct(
                    &progress,
                    "plan_scene",
                    &format!(
                        "规划场景剧本（事件 {} · 场景 {}）",
                        event.index, scene.index
                    ),
                    base + 15.0,
                );
                let scene_work = self
                    .working_dir
                    .join("scene_renders")
                    .join(format!("event_{}", event.index))
                    .join(format!("scene_{}", scene.index));
                tokio::fs::create_dir_all(&scene_work).await?;
                write_text_artifact(&scene_work.join("script.txt"), &scene.script).await?;
                let budget = budgets
                    .get(si)
                    .copied()
                    .unwrap_or(crate::planning::DEFAULT_TARGET_DURATION_SECS);
                write_text_artifact(
                    &scene_work.join("target_duration_secs.txt"),
                    &budget.to_string(),
                )
                .await?;
                let scene_req = crate::planning::enrich_requirement_for_scene(
                    user_requirement,
                    budget,
                    si,
                    scene_n,
                    film_total,
                );
                let s2v = Script2VideoPipeline::new(self.backends.clone(), scene_work);
                let _ = s2v
                    .plan_text_artifacts(&scene.script, &scene_req, style, progress.clone())
                    .await?;
            }
        }
        emit_pct(&progress, "planned", "规划完成，可以开始渲染", 100.0);
        Ok(())
    }

    pub async fn render(
        &self,
        novel_text: &str,
        user_requirement: &str,
        style: &str,
        progress: Option<ProgressCallback>,
    ) -> VimaxResult<PathBuf> {
        emit_pct(&progress, "render_start", "开始渲染小说成片", 2.0);
        self.plan_text_artifacts(novel_text, user_requirement, style, progress.clone())
            .await?;

        let events_path = self.working_dir.join("events").join("events.json");
        let events: Vec<Event> =
            serde_json::from_str(&tokio::fs::read_to_string(events_path).await?)?;

        let mut all_videos = Vec::new();
        let mut pending = 0usize;
        let mut total_scenes = 0usize;
        for event in &events {
            let scenes_dir = self
                .working_dir
                .join("scenes")
                .join(format!("event_{}", event.index));
            let scenes: Vec<Scene> = serde_json::from_str(
                &tokio::fs::read_to_string(scenes_dir.join("scenes.json")).await?,
            )?;
            for scene in &scenes {
                total_scenes += 1;
                let scene_work = self
                    .working_dir
                    .join("scene_renders")
                    .join(format!("event_{}", event.index))
                    .join(format!("scene_{}", scene.index));
                let scene_final = scene_work.join("final_video.mp4");
                media_local::scrub_unusable_video(&scene_final).await?;
                if media_local::is_usable_video_file(&scene_final) {
                    emit(
                        &progress,
                        "render_scene_skip",
                        &format!(
                            "事件 {} 场景 {} 已完成，跳过",
                            event.index, scene.index
                        ),
                    );
                    all_videos.push(scene_final);
                    continue;
                }
                pending += 1;
                let s2v = Script2VideoPipeline::new(self.backends.clone(), scene_work);
                emit(
                    &progress,
                    "render_scene",
                    &format!(
                        "正在渲染事件 {} 场景 {}",
                        event.index, scene.index
                    ),
                );
                let video = s2v
                    .render(&scene.script, user_requirement, style, None)
                    .await?;
                all_videos.push(video);
            }
        }
        if pending > 0 && pending < total_scenes {
            emit(
                &progress,
                "render_resume",
                &format!("从断点继续：待渲染 {pending}/{total_scenes} 个场景"),
            );
        }

        let final_path = self.working_dir.join("final_video.mp4");
        media_local::scrub_unusable_video(&final_path).await?;
        if !media_local::is_usable_video_file(&final_path) && !all_videos.is_empty() {
            emit_pct(&progress, "concat_start", "正在拼接全部场景视频", 95.0);
            let refs: Vec<&Path> = all_videos.iter().map(|p| p.as_path()).collect();
            media_local::concat_videos(&refs, &final_path).await?;
        }
        emit_pct(&progress, "render_done", "小说成片渲染完成", 100.0);
        Ok(final_path)
    }
}
