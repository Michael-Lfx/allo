//! Ordered scene videos waiting to be spliced into one film.
//!
//! Film-level pipelines (idea / novel / script) all render scenes in timeline
//! order and hand scene N's tail frame to scene N+1's first shot as a match-cut
//! reference. That single fact decides two things at once:
//!
//! - what `prior_continuity` the next scene renders from, and
//! - whether the finished film's seam between the two scenes is a hard cut or a
//!   match-cut that shares one soundtrack (which the concat must only de-click).
//!
//! Keeping both in one place means a pipeline cannot accidentally chain the
//! render but describe the splice as a cut, or vice versa.
//!
//! Scene seams are never [`SpliceSeam::SameTake`]: the scene's own shot join
//! already trimmed whatever its opening shot replayed.

use std::path::{Path, PathBuf};

use crate::media_local::{ConcatClip, SpliceSeam};

use super::resolve_scene_tail_continuity;

/// Scene videos collected in timeline order, each tagged with how it meets the
/// scene before it.
#[derive(Debug, Default)]
pub(crate) struct SceneReel {
    scenes: Vec<(PathBuf, SpliceSeam)>,
    tail_frame: Option<PathBuf>,
}

impl SceneReel {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Continuity frame for the next scene's opening shot, if the last scene
    /// left a usable one.
    pub(crate) fn tail_frame(&self) -> Option<&Path> {
        self.tail_frame.as_deref()
    }

    /// Record a finished scene, then pick up its tail frame for the next one.
    ///
    /// The seam is read from the reel state *before* the push, which is exactly
    /// the frame the scene was rendered from.
    pub(crate) async fn push(&mut self, video: PathBuf, scene_dir: &Path) {
        let seam = match self.tail_frame {
            Some(_) => SpliceSeam::MatchCut,
            None => SpliceSeam::Cut,
        };
        self.scenes.push((video, seam));
        self.tail_frame = resolve_scene_tail_continuity(scene_dir).await;
    }

    pub(crate) fn len(&self) -> usize {
        self.scenes.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.scenes.is_empty()
    }

    /// Concat inputs in timeline order.
    pub(crate) fn concat_clips(&self) -> Vec<ConcatClip<'_>> {
        self.scenes
            .iter()
            .map(|(path, seam)| ConcatClip::new(path.as_path(), *seam))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A reel built without any tail frames is a sequence of hard cuts.
    #[test]
    fn scenes_without_tail_frames_are_cuts() {
        let mut reel = SceneReel::new();
        assert!(reel.is_empty());
        reel.scenes.push((PathBuf::from("a.mp4"), SpliceSeam::Cut));
        reel.scenes.push((PathBuf::from("b.mp4"), SpliceSeam::Cut));
        assert_eq!(reel.len(), 2);
        assert!(reel.concat_clips().iter().all(|c| c.seam == SpliceSeam::Cut));
    }

    #[tokio::test]
    async fn first_scene_is_a_cut_and_chained_scenes_match_cut() {
        let tmp = tempfile::tempdir().unwrap();
        let mut reel = SceneReel::new();

        // No previous scene → hard cut, and an empty scene dir yields no tail.
        reel.push(PathBuf::from("scene_0.mp4"), tmp.path()).await;
        assert!(reel.tail_frame().is_none());

        // Simulate a scene that did leave a tail frame behind.
        reel.tail_frame = Some(tmp.path().join("video_last_frame.png"));
        reel.push(PathBuf::from("scene_1.mp4"), tmp.path()).await;

        let clips = reel.concat_clips();
        assert_eq!(clips[0].seam, SpliceSeam::Cut);
        assert_eq!(clips[1].seam, SpliceSeam::MatchCut);
    }
}
