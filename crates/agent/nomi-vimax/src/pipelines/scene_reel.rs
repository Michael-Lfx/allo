//! Ordered scene videos waiting to be spliced into one film.
//!
//! Each scene is generated from its own blocking text, cast portraits, and
//! empty-set plates. The previous scene's last frame is not a reference, so
//! every scene join is a hard cut.

use std::path::PathBuf;

use crate::media_local::{ConcatClip, SpliceSeam};

/// Scene videos collected in timeline order, each tagged with how it meets the
/// scene before it.
#[derive(Debug, Default)]
pub(crate) struct SceneReel {
    scenes: Vec<(PathBuf, SpliceSeam)>,
}

impl SceneReel {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Record a finished scene. Joins are hard cuts: the next scene does not
    /// open on this scene's tail frame.
    pub(crate) fn push(&mut self, video: PathBuf) {
        self.scenes.push((video, SpliceSeam::Cut));
    }

    #[allow(dead_code)]
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

    #[test]
    fn every_scene_join_is_a_hard_cut() {
        let mut reel = SceneReel::new();
        assert!(reel.is_empty());
        reel.push(PathBuf::from("a.mp4"));
        reel.push(PathBuf::from("b.mp4"));
        assert_eq!(reel.len(), 2);
        assert!(reel.concat_clips().iter().all(|c| c.seam == SpliceSeam::Cut));
    }
}
