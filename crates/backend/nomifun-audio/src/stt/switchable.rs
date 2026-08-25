//! [`SwitchableSttCallback`]: Auto / Local / Cloud selection.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;

use crate::frame::AudioChannel;
use crate::recorder::SttCallback;
use crate::session::SttBackendChoice;

use super::cloud::{CloudSttCallback, MeetingCloudStt};
use super::null::NullSttCallback;
use super::try_load_sherpa;

/// Routes transcription according to [`SttBackendChoice`].
///
/// - **LocalSherpa**: local only (missing models → `None`)
/// - **CloudModelInvoke**: cloud only (unwired → `None`)
/// - **Auto**: local first when available; fall back to cloud on `None`
pub struct SwitchableSttCallback {
    choice: SttBackendChoice,
    local: Option<Arc<dyn SttCallback>>,
    cloud: Option<Arc<dyn SttCallback>>,
}

impl SwitchableSttCallback {
    pub fn new(
        choice: SttBackendChoice,
        local: Option<Arc<dyn SttCallback>>,
        cloud: Option<Arc<dyn SttCallback>>,
    ) -> Self {
        Self {
            choice,
            local,
            cloud,
        }
    }

    pub fn choice(&self) -> SttBackendChoice {
        self.choice
    }

    pub fn has_local(&self) -> bool {
        self.local.is_some()
    }

    pub fn has_cloud(&self) -> bool {
        self.cloud.is_some()
    }
}

#[async_trait]
impl SttCallback for SwitchableSttCallback {
    async fn transcribe(
        &self,
        channel: AudioChannel,
        pcm: Vec<f32>,
        sample_rate: u32,
    ) -> Option<String> {
        match self.choice {
            SttBackendChoice::LocalSherpa => match &self.local {
                Some(local) => local.transcribe(channel, pcm, sample_rate).await,
                None => None,
            },
            SttBackendChoice::CloudModelInvoke => match &self.cloud {
                Some(cloud) => cloud.transcribe(channel, pcm, sample_rate).await,
                None => None,
            },
            SttBackendChoice::Auto => {
                if let Some(local) = &self.local {
                    if let Some(text) = local
                        .transcribe(channel, pcm.clone(), sample_rate)
                        .await
                    {
                        return Some(text);
                    }
                }
                match &self.cloud {
                    Some(cloud) => cloud.transcribe(channel, pcm, sample_rate).await,
                    None => None,
                }
            }
        }
    }
}

/// Build a switchable STT callback from preference + optional cloud host.
///
/// Local models are resolved from `model_dir`, else `NOMI_SHERPA_ASR_MODEL_DIR`.
/// Missing models or a disabled `local-stt` feature yield no local backend
/// (Auto still works with cloud alone).
pub fn build_switchable_stt(
    choice: SttBackendChoice,
    model_dir: Option<&Path>,
    cloud: Option<Arc<dyn MeetingCloudStt>>,
) -> Arc<dyn SttCallback> {
    let local: Option<Arc<dyn SttCallback>> = match try_load_sherpa(model_dir) {
        Ok(Some(sherpa)) => Some(Arc::new(sherpa) as Arc<dyn SttCallback>),
        Ok(None) => None,
        Err(err) => {
            tracing::warn!("local sherpa STT unavailable: {err}");
            None
        }
    };

    let cloud_cb: Option<Arc<dyn SttCallback>> =
        cloud.map(|c| Arc::new(CloudSttCallback::new(c)) as Arc<dyn SttCallback>);

    if local.is_none() && cloud_cb.is_none() {
        tracing::warn!(
            "build_switchable_stt: no local or cloud STT configured (choice={})",
            choice.as_str()
        );
        return Arc::new(NullSttCallback);
    }

    Arc::new(SwitchableSttCallback::new(choice, local, cloud_cb))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stt::FakeSttCallback;

    #[tokio::test]
    async fn auto_prefers_local_when_present() {
        let local: Arc<dyn SttCallback> = Arc::new(FakeSttCallback::always("local"));
        let cloud: Arc<dyn SttCallback> = Arc::new(FakeSttCallback::always("cloud"));
        let sw = SwitchableSttCallback::new(SttBackendChoice::Auto, Some(local), Some(cloud));
        let text = sw
            .transcribe(AudioChannel::Mic, vec![0.1; 100], 16_000)
            .await;
        assert_eq!(text.as_deref(), Some("local"));
    }

    #[tokio::test]
    async fn auto_falls_back_to_cloud_when_local_none() {
        let local: Arc<dyn SttCallback> = Arc::new(FakeSttCallback::script(vec![None]));
        let cloud: Arc<dyn SttCallback> = Arc::new(FakeSttCallback::always("cloud"));
        let sw = SwitchableSttCallback::new(SttBackendChoice::Auto, Some(local), Some(cloud));
        let text = sw
            .transcribe(AudioChannel::Loopback, vec![0.1; 100], 16_000)
            .await;
        assert_eq!(text.as_deref(), Some("cloud"));
    }

    #[tokio::test]
    async fn auto_cloud_only_when_no_local() {
        let cloud: Arc<dyn SttCallback> = Arc::new(FakeSttCallback::always("cloud"));
        let sw = SwitchableSttCallback::new(SttBackendChoice::Auto, None, Some(cloud));
        let text = sw
            .transcribe(AudioChannel::Mic, vec![0.1; 100], 16_000)
            .await;
        assert_eq!(text.as_deref(), Some("cloud"));
    }

    #[tokio::test]
    async fn local_choice_does_not_use_cloud() {
        let cloud: Arc<dyn SttCallback> = Arc::new(FakeSttCallback::always("cloud"));
        let sw = SwitchableSttCallback::new(SttBackendChoice::LocalSherpa, None, Some(cloud));
        assert!(sw
            .transcribe(AudioChannel::Mic, vec![0.1; 100], 16_000)
            .await
            .is_none());
    }

    #[tokio::test]
    async fn cloud_choice_ignores_local() {
        let local: Arc<dyn SttCallback> = Arc::new(FakeSttCallback::always("local"));
        let cloud: Arc<dyn SttCallback> = Arc::new(FakeSttCallback::always("cloud"));
        let sw =
            SwitchableSttCallback::new(SttBackendChoice::CloudModelInvoke, Some(local), Some(cloud));
        let text = sw
            .transcribe(AudioChannel::Mic, vec![0.1; 100], 16_000)
            .await;
        assert_eq!(text.as_deref(), Some("cloud"));
    }

    #[tokio::test]
    async fn dual_track_invokes_per_channel_independently() {
        let local: Arc<dyn SttCallback> = Arc::new(FakeSttCallback::script(vec![
            Some("mic-text".into()),
            Some("lb-text".into()),
        ]));
        let sw = SwitchableSttCallback::new(SttBackendChoice::LocalSherpa, Some(local), None);
        let mic = sw
            .transcribe(AudioChannel::Mic, vec![0.2; 50], 16_000)
            .await;
        let lb = sw
            .transcribe(AudioChannel::Loopback, vec![0.2; 50], 16_000)
            .await;
        assert_eq!(mic.as_deref(), Some("mic-text"));
        assert_eq!(lb.as_deref(), Some("lb-text"));
    }
}
