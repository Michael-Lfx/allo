//! In-memory gallery + IMeetingRepository persistence helpers.

use std::sync::Arc;

use nomifun_db::{
    IMeetingRepository, MeetingVoiceprintRow, UpsertMeetingVoiceprintParams,
};
use uuid::Uuid;

use super::{VoiceprintEncoder, cosine_similarity};

/// One enrolled voiceprint (in-memory form).
#[derive(Debug, Clone, PartialEq)]
pub struct VoiceprintEntry {
    pub voiceprint_id: String,
    pub display_name: String,
    pub embedding: Vec<f32>,
}

/// Cosine match result against a gallery entry.
#[derive(Debug, Clone)]
pub struct VoiceprintMatch {
    pub entry: VoiceprintEntry,
    pub score: f32,
}

/// In-memory embedding gallery for matching during a session.
#[derive(Debug, Clone, Default)]
pub struct VoiceprintGallery {
    entries: Vec<VoiceprintEntry>,
}

impl VoiceprintGallery {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_entries(entries: Vec<VoiceprintEntry>) -> Self {
        Self { entries }
    }

    pub fn from_rows(rows: &[MeetingVoiceprintRow]) -> Result<Self, String> {
        let mut entries = Vec::with_capacity(rows.len());
        for row in rows {
            entries.push(VoiceprintEntry {
                voiceprint_id: row.voiceprint_id.clone(),
                display_name: row.display_name.clone(),
                embedding: embedding_from_blob(&row.embedding_blob)?,
            });
        }
        Ok(Self { entries })
    }

    pub fn entries(&self) -> &[VoiceprintEntry] {
        &self.entries
    }

    pub fn push(&mut self, entry: VoiceprintEntry) {
        if let Some(existing) = self
            .entries
            .iter_mut()
            .find(|e| e.voiceprint_id == entry.voiceprint_id)
        {
            *existing = entry;
        } else {
            self.entries.push(entry);
        }
    }

    pub fn remove(&mut self, voiceprint_id: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.voiceprint_id != voiceprint_id);
        self.entries.len() != before
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Best cosine match at or above `threshold`, if any.
    pub fn best_match(&self, query: &[f32], threshold: f32) -> Option<VoiceprintMatch> {
        let mut best: Option<VoiceprintMatch> = None;
        for entry in &self.entries {
            let score = cosine_similarity(query, &entry.embedding);
            if score < threshold {
                continue;
            }
            let replace = match &best {
                None => true,
                Some(cur) => score > cur.score,
            };
            if replace {
                best = Some(VoiceprintMatch {
                    entry: entry.clone(),
                    score,
                });
            }
        }
        best
    }
}

/// Serialize f32 embedding as little-endian bytes for `embedding_blob`.
pub fn embedding_to_blob(embedding: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(embedding.len() * 4);
    for v in embedding {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

/// Deserialize little-endian f32 embedding blob.
pub fn embedding_from_blob(blob: &[u8]) -> Result<Vec<f32>, String> {
    if !blob.len().is_multiple_of(4) {
        return Err(format!(
            "embedding_blob length {} is not a multiple of 4",
            blob.len()
        ));
    }
    let mut out = Vec::with_capacity(blob.len() / 4);
    for chunk in blob.chunks_exact(4) {
        let arr: [u8; 4] = chunk.try_into().map_err(|_| "invalid embedding chunk")?;
        out.push(f32::from_le_bytes(arr));
    }
    Ok(out)
}

/// Persist / list / delete voiceprints via [`IMeetingRepository`].
pub struct VoiceprintStore<E: VoiceprintEncoder> {
    encoder: E,
    repo: Arc<dyn IMeetingRepository>,
}

impl<E: VoiceprintEncoder> VoiceprintStore<E> {
    pub fn new(encoder: E, repo: Arc<dyn IMeetingRepository>) -> Self {
        Self { encoder, repo }
    }

    pub fn encoder(&self) -> &E {
        &self.encoder
    }

    /// Enroll (or overwrite by new UUID) a voiceprint from PCM.
    pub async fn enroll(
        &self,
        user_id: &str,
        display_name: &str,
        pcm: &[f32],
        sample_rate: u32,
    ) -> Result<MeetingVoiceprintRow, String> {
        let embedding = self.encoder.encode(pcm, sample_rate)?;
        self.enroll_embedding(user_id, display_name, &embedding)
            .await
    }

    /// Enroll with a precomputed (or placeholder) embedding vector.
    pub async fn enroll_embedding(
        &self,
        user_id: &str,
        display_name: &str,
        embedding: &[f32],
    ) -> Result<MeetingVoiceprintRow, String> {
        if embedding.is_empty() {
            return Err("embedding must not be empty".into());
        }
        let now = now_ms();
        let params = UpsertMeetingVoiceprintParams {
            voiceprint_id: Uuid::now_v7().to_string(),
            user_id: user_id.to_string(),
            display_name: display_name.to_string(),
            embedding_blob: embedding_to_blob(embedding),
            created_at: now,
            updated_at: now,
        };
        self.repo
            .upsert_voiceprint(&params)
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn list(&self, user_id: &str) -> Result<Vec<MeetingVoiceprintRow>, String> {
        self.repo
            .list_voiceprints(user_id)
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn delete(&self, user_id: &str, voiceprint_id: &str) -> Result<bool, String> {
        self.repo
            .delete_voiceprint(user_id, voiceprint_id)
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn clear(&self, user_id: &str) -> Result<u64, String> {
        self.repo
            .clear_voiceprints(user_id)
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn load_gallery(&self, user_id: &str) -> Result<VoiceprintGallery, String> {
        let rows = self.list(user_id).await?;
        VoiceprintGallery::from_rows(&rows)
    }
}

fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voiceprint::FakeVoiceprintEncoder;
    use async_trait::async_trait;
    use nomifun_db::{
        DbError, InsertMeetingSessionParams, MeetingSegmentRow, MeetingSessionRow,
        MeetingSpeakerRow, UpdateMeetingSessionParams, UpsertMeetingSegmentParams,
        UpsertMeetingSpeakerParams,
    };
    use std::sync::Mutex;

    struct MemMeetingRepo {
        voiceprints: Mutex<Vec<MeetingVoiceprintRow>>,
        next_id: Mutex<i64>,
    }

    impl MemMeetingRepo {
        fn new() -> Self {
            Self {
                voiceprints: Mutex::new(Vec::new()),
                next_id: Mutex::new(1),
            }
        }
    }

    #[async_trait]
    impl IMeetingRepository for MemMeetingRepo {
        async fn insert_session(
            &self,
            _params: &InsertMeetingSessionParams,
        ) -> Result<MeetingSessionRow, DbError> {
            unimplemented!()
        }
        async fn update_session(
            &self,
            _session_id: &str,
            _params: &UpdateMeetingSessionParams,
        ) -> Result<Option<MeetingSessionRow>, DbError> {
            unimplemented!()
        }
        async fn get_session(
            &self,
            _session_id: &str,
        ) -> Result<Option<MeetingSessionRow>, DbError> {
            unimplemented!()
        }
        async fn list_sessions_for_owner(
            &self,
            _user_id: &str,
            _limit: i64,
        ) -> Result<Vec<MeetingSessionRow>, DbError> {
            unimplemented!()
        }
        async fn upsert_segment(
            &self,
            _params: &UpsertMeetingSegmentParams,
        ) -> Result<MeetingSegmentRow, DbError> {
            unimplemented!()
        }
        async fn list_segments(
            &self,
            _session_id: &str,
        ) -> Result<Vec<MeetingSegmentRow>, DbError> {
            unimplemented!()
        }
        async fn search_segments(
            &self,
            _session_id: &str,
            _query: &str,
            _limit: i64,
        ) -> Result<Vec<MeetingSegmentRow>, DbError> {
            unimplemented!()
        }
        async fn upsert_speaker(
            &self,
            _params: &UpsertMeetingSpeakerParams,
        ) -> Result<MeetingSpeakerRow, DbError> {
            unimplemented!()
        }
        async fn list_speakers(
            &self,
            _session_id: &str,
        ) -> Result<Vec<MeetingSpeakerRow>, DbError> {
            unimplemented!()
        }
        async fn upsert_voiceprint(
            &self,
            params: &UpsertMeetingVoiceprintParams,
        ) -> Result<MeetingVoiceprintRow, DbError> {
            let mut id = self.next_id.lock().unwrap();
            let row = MeetingVoiceprintRow {
                id: *id,
                voiceprint_id: params.voiceprint_id.clone(),
                user_id: params.user_id.clone(),
                display_name: params.display_name.clone(),
                embedding_blob: params.embedding_blob.clone(),
                created_at: params.created_at,
                updated_at: params.updated_at,
            };
            *id += 1;
            let mut vps = self.voiceprints.lock().unwrap();
            if let Some(existing) = vps
                .iter_mut()
                .find(|r| r.voiceprint_id == row.voiceprint_id)
            {
                *existing = row.clone();
            } else {
                vps.push(row.clone());
            }
            Ok(row)
        }
        async fn list_voiceprints(
            &self,
            user_id: &str,
        ) -> Result<Vec<MeetingVoiceprintRow>, DbError> {
            Ok(self
                .voiceprints
                .lock()
                .unwrap()
                .iter()
                .filter(|r| r.user_id == user_id)
                .cloned()
                .collect())
        }
        async fn delete_voiceprint(
            &self,
            user_id: &str,
            voiceprint_id: &str,
        ) -> Result<bool, DbError> {
            let mut vps = self.voiceprints.lock().unwrap();
            let before = vps.len();
            vps.retain(|r| !(r.user_id == user_id && r.voiceprint_id == voiceprint_id));
            Ok(vps.len() != before)
        }
        async fn clear_voiceprints(&self, user_id: &str) -> Result<u64, DbError> {
            let mut vps = self.voiceprints.lock().unwrap();
            let before = vps.len();
            vps.retain(|r| r.user_id != user_id);
            Ok((before - vps.len()) as u64)
        }
    }

    #[test]
    fn blob_roundtrip() {
        let emb = vec![1.0f32, -2.5, 0.0, 3.25];
        let blob = embedding_to_blob(&emb);
        assert_eq!(embedding_from_blob(&blob).unwrap(), emb);
    }

    #[test]
    fn gallery_best_match_respects_threshold() {
        let gallery = VoiceprintGallery::from_entries(vec![VoiceprintEntry {
            voiceprint_id: "a".into(),
            display_name: "Me".into(),
            embedding: vec![1.0, 0.0, 0.0],
        }]);
        let hit = gallery.best_match(&[0.9, 0.1, 0.0], 0.5).unwrap();
        assert_eq!(hit.entry.display_name, "Me");
        assert!(gallery.best_match(&[0.0, 1.0, 0.0], 0.5).is_none());
    }

    #[tokio::test]
    async fn store_enroll_list_delete() {
        let repo = Arc::new(MemMeetingRepo::new());
        let store = VoiceprintStore::new(FakeVoiceprintEncoder::new(8), repo);
        let pcm = vec![0.2f32; 1600];
        let row = store.enroll("u1", "Me", &pcm, 16_000).await.unwrap();
        assert_eq!(row.display_name, "Me");
        let listed = store.list("u1").await.unwrap();
        assert_eq!(listed.len(), 1);
        let gallery = store.load_gallery("u1").await.unwrap();
        assert_eq!(gallery.entries().len(), 1);
        assert!(store.delete("u1", &row.voiceprint_id).await.unwrap());
        assert!(store.list("u1").await.unwrap().is_empty());
    }
}
