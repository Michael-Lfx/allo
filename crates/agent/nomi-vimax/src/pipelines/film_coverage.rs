//! Film-level storyboard completeness: one engine, one existence check.

use std::path::Path;

use crate::clip_bounds::ClipBounds;
use crate::domain::ShotBriefDescription;
use crate::drama::{
    load_drama_engine, pack_split_needles, place_missing_film_beats,
};
use crate::error::VimaxResult;
use crate::planning::max_shots_for_budget;
use crate::session::{read_json_artifact, write_json_artifact};
use crate::skills::DirectorSpec;

use super::clip_beats::{self, PackOpts};

/// After every scene has drafted its own SCRIPT, insert missing hook/turn/payoff
/// onto the scene that already owns them, then pack those boards once.
pub(crate) async fn apply_film_coverage(film_root: &Path, clip: ClipBounds) -> VimaxResult<()> {
    let Some(engine) = load_drama_engine(film_root) else {
        return Ok(());
    };
    let mut dirs = Vec::new();
    let mut scripts = Vec::new();
    let mut boards: Vec<Vec<ShotBriefDescription>> = Vec::new();
    for i in 0.. {
        let dir = film_root.join(format!("scene_{i}"));
        let board_path = dir.join("storyboard.json");
        if !board_path.is_file() {
            break;
        }
        boards.push(read_json_artifact(&board_path).await?);
        let script = tokio::fs::read_to_string(dir.join("script.txt"))
            .await
            .unwrap_or_default();
        scripts.push(script);
        dirs.push(dir);
    }
    if boards.is_empty() {
        return Ok(());
    }
    let dirty = place_missing_film_beats(&engine, &scripts, &mut boards);
    if dirty.is_empty() {
        return Ok(());
    }
    let spec = DirectorSpec::load_from_dir(film_root);
    let split_needles = pack_split_needles(&engine);
    let opts = PackOpts {
        policy: spec.pack_policy,
        split_needles,
    };
    for i in dirty {
        let max_shots = scene_max_shots(&dirs[i], clip).await;
        let packed = clip_beats::pack_briefs_for_publish(
            clip,
            std::mem::take(&mut boards[i]),
            opts.clone(),
            max_shots,
            spec.over_budget,
        );
        write_json_artifact(&dirs[i].join("storyboard.json"), &packed).await?;
        for name in ["shot_descriptions.json", "camera_tree.json"] {
            let p = dirs[i].join(name);
            if p.exists() {
                let _ = tokio::fs::remove_file(&p).await;
            }
        }
        tracing::info!(
            scene = i,
            clips = packed.len(),
            "inserted missing film beats into owning scene and packed"
        );
    }
    Ok(())
}

async fn scene_max_shots(scene_dir: &Path, clip: ClipBounds) -> Option<usize> {
    let p = scene_dir.join("target_duration_secs.txt");
    let text = tokio::fs::read_to_string(&p).await.ok()?;
    let n: u32 = text.trim().parse().ok()?;
    (n > 0).then(|| max_shots_for_budget(clip, n))
}
