//! Host-provided conversation artifacts and local library view state.
//!
//! This UI model deliberately contains no filesystem paths. The host owns
//! artifact storage and resolves preview/export operations by stable ID.

use serde::{Deserialize, Serialize};

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    #[default]
    All,
    Document,
    Slides,
    Table,
    Image,
    Video,
    Audio,
    Other,
    // Kept for reading old isolated example fixtures. They are not included
    // in the production filter tabs because they imply runnable artifacts.
    Website,
    Game,
    MobileApp,
}

impl Category {
    pub const ALL: [Self; 8] = [
        Self::All,
        Self::Document,
        Self::Slides,
        Self::Table,
        Self::Image,
        Self::Video,
        Self::Audio,
        Self::Other,
    ];

    pub const fn key(self) -> &'static str {
        match self {
            Self::All => "library.category.all",
            Self::Document => "library.category.document",
            Self::Slides => "library.category.slides",
            Self::Table => "library.category.table",
            Self::Image => "library.category.image",
            Self::Video => "library.category.video",
            Self::Audio => "library.category.audio",
            Self::Other => "library.category.other",
            Self::Website => "library.category.website",
            Self::Game => "library.category.game",
            Self::MobileApp => "library.category.mobile_app",
        }
    }
}

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactAvailability {
    #[default]
    Available,
    Missing,
    Corrupt,
    Deleted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryItem {
    /// Stable host-issued artifact ID shared by chat, library, and preview.
    pub id: String,
    pub title: String,
    /// The originating request, shown as secondary context and searched.
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub category: Category,
    /// Calendar/time labels prepared by the host for the current locale.
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub time: String,
    #[serde(default)]
    pub pending: bool,
    /// Optional preview message for unsupported or unavailable content.
    #[serde(default)]
    pub preview: Option<String>,
    #[serde(default)]
    pub size_bytes: Option<u64>,
    #[serde(default)]
    pub extension: String,
    #[serde(default)]
    pub media_type: Option<String>,
    #[serde(default)]
    pub delivered_at_ms: u64,
    #[serde(default)]
    pub delivered_date: String,
    #[serde(default)]
    pub delivered_time: String,
    #[serde(default)]
    pub source_session_id: String,
    #[serde(default)]
    pub source_title: String,
    #[serde(default)]
    pub source_deleted: bool,
    #[serde(default)]
    pub source_unavailable: bool,
    #[serde(default)]
    pub favorite: bool,
    #[serde(default)]
    pub trashed: bool,
    #[serde(default)]
    pub availability: ArtifactAvailability,
}

impl LibraryItem {
    pub fn inline_preview_supported(&self) -> bool {
        matches!(
            self.extension
                .trim_start_matches('.')
                .to_ascii_lowercase()
                .as_str(),
            "jpg" | "jpeg" | "png" | "webp" | "pdf" | "pptx"
        )
    }

    pub fn default_open_supported(&self) -> bool {
        flowy_agent::can_open_with_default_app(&format!(
            "artifact.{}",
            self.extension.trim_start_matches('.'),
        )) && self.availability == ArtifactAvailability::Available
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LibraryView {
    #[default]
    List,
    Grid,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LibrarySort {
    #[default]
    Newest,
    Oldest,
    Name,
}

impl LibrarySort {
    pub const fn key(self) -> &'static str {
        match self {
            Self::Newest => "library.sort.newest",
            Self::Oldest => "library.sort.oldest",
            Self::Name => "library.sort.name",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibraryGroup {
    /// `None` when the active sort does not group by date.
    pub date: Option<String>,
    pub items: Vec<LibraryItem>,
}

/// Disk usage measured by the host. Pending cleanup bytes are already part
/// of `stored_bytes` and must not be added to `charged_bytes` again.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibraryUsage {
    pub stored_bytes: u64,
    pub staging_bytes: u64,
    pub view_bytes: u64,
    pub pending_delete_bytes: u64,
    pub stored_items: usize,
    pub max_items: usize,
    pub max_bytes: u64,
}

impl LibraryUsage {
    pub const fn charged_bytes(&self) -> u64 {
        self.stored_bytes
            .saturating_add(self.staging_bytes)
            .saturating_add(self.view_bytes)
    }
}

#[derive(Clone, Debug, Default)]
pub struct LibraryModel {
    pub items: Vec<LibraryItem>,
    pub category: Category,
    pub sort: LibrarySort,
    pub view: LibraryView,
    pub favorites_only: bool,
    pub showing_trash: bool,
}

impl LibraryModel {
    pub fn new(items: Vec<LibraryItem>) -> Self {
        Self {
            items,
            ..Self::default()
        }
    }

    /// Time sorts keep one section per calendar date. Name sort is a single
    /// list so the same date is not split around other names.
    pub fn groups(&self, query: &str) -> Vec<LibraryGroup> {
        let items = self.filtered(query);
        if items.is_empty() {
            return Vec::new();
        }
        if self.sort == LibrarySort::Name {
            return vec![LibraryGroup { date: None, items }];
        }
        let mut groups: Vec<LibraryGroup> = Vec::new();
        for item in items {
            let date = if item.delivered_date.is_empty() {
                item.date.as_str()
            } else {
                item.delivered_date.as_str()
            };
            if let Some(group) = groups
                .iter_mut()
                .find(|group| group.date.as_deref() == Some(date))
            {
                group.items.push(item);
            } else {
                groups.push(LibraryGroup {
                    date: Some(date.to_owned()),
                    items: vec![item],
                });
            }
        }
        groups
    }

    /// Search, category, favorite, and trash filters compose without changing
    /// the host-owned source records.
    pub fn filtered(&self, query: &str) -> Vec<LibraryItem> {
        let query = query.trim().to_lowercase();
        let mut items: Vec<_> = self
            .items
            .iter()
            .filter(|item| item.availability != ArtifactAvailability::Deleted)
            .filter(|item| item.trashed == self.showing_trash)
            .filter(|item| !self.favorites_only || item.favorite)
            .filter(|item| {
                self.category == Category::All || item.category == self.category
            })
            .filter(|item| {
                if query.is_empty() {
                    return true;
                }
                let searchable = format!(
                    "{} {} {}",
                    item.title, item.description, item.source_title
                )
                .to_lowercase();
                query
                    .split_whitespace()
                    .all(|token| searchable.contains(token))
            })
            .cloned()
            .collect();
        match self.sort {
            LibrarySort::Newest => items.sort_by(|left, right| {
                right
                    .delivered_at_ms
                    .cmp(&left.delivered_at_ms)
                    .then_with(|| left.id.cmp(&right.id))
            }),
            LibrarySort::Oldest => items.sort_by(|left, right| {
                left.delivered_at_ms
                    .cmp(&right.delivered_at_ms)
                    .then_with(|| left.id.cmp(&right.id))
            }),
            LibrarySort::Name => items.sort_by_cached_key(|item| {
                (item.title.to_lowercase(), item.id.clone())
            }),
        }
        items
    }

    pub fn is_favorite(&self, id: &str) -> bool {
        self.items.iter().any(|item| item.id == id && item.favorite)
    }

    /// Apply a host-confirmed preference to the matching source record.
    pub fn set_favorite(&mut self, id: &str, favorite: bool) -> bool {
        let Some(item) = self.items.iter_mut().find(|item| item.id == id)
        else {
            return false;
        };
        item.favorite = favorite;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ArtifactAvailability, Category, LibraryItem, LibraryModel, LibrarySort,
        LibraryView,
    };

    fn item(
        id: &str,
        title: &str,
        category: Category,
        time: &str,
    ) -> LibraryItem {
        LibraryItem {
            id: id.into(),
            title: title.into(),
            description: "制作产品发布照片".into(),
            category,
            date: "2026-11-04".into(),
            time: time.into(),
            pending: false,
            preview: None,
            size_bytes: None,
            extension: String::new(),
            media_type: None,
            delivered_at_ms: 0,
            delivered_date: "2026-11-04".into(),
            delivered_time: time.into(),
            source_session_id: format!("session-{id}"),
            source_title: String::new(),
            source_deleted: false,
            source_unavailable: false,
            favorite: false,
            trashed: false,
            availability: ArtifactAvailability::Available,
        }
    }

    fn ids(items: &[LibraryItem]) -> Vec<&str> {
        items.iter().map(|item| item.id.as_str()).collect()
    }

    #[test]
    fn category_search_and_source_title_filters_compose() {
        let mut hero =
            item("hero", "Dew & Stone Hero.jpg", Category::Image, "09:58");
        hero.source_title = "Product launch brief".into();
        let mut model = LibraryModel::new(vec![
            hero,
            item("notes", "Dew & Stone notes", Category::Document, "09:58"),
            item("sport", "sport.jpg", Category::Image, "09:58"),
        ]);
        model.category = Category::Image;
        assert_eq!(ids(&model.filtered("  DEW  ")), vec!["hero"]);
        assert_eq!(ids(&model.filtered("launch")), vec!["hero"]);
        assert_eq!(model.filtered("missing").len(), 0);
        assert_eq!(model.items.len(), 3);
    }

    #[test]
    fn timestamp_sort_is_deterministic_and_keeps_view_state() {
        let mut old = item("old", "Zebra", Category::Image, "20:00");
        old.delivered_at_ms = 100;
        let mut late = item("late", "Gamma", Category::Other, "17:22");
        late.delivered_at_ms = 300;
        let mut b = item("b", "beta", Category::Image, "09:58");
        b.delivered_at_ms = 200;
        let mut a = item("a", "Alpha", Category::Image, "09:58");
        a.delivered_at_ms = 200;
        let mut model = LibraryModel::new(vec![b, old, late, a]);
        model.view = LibraryView::Grid;
        assert_eq!(ids(&model.filtered("")), vec!["late", "a", "b", "old"]);
        model.sort = LibrarySort::Oldest;
        assert_eq!(ids(&model.filtered("")), vec!["old", "a", "b", "late"]);
        model.sort = LibrarySort::Name;
        assert_eq!(ids(&model.filtered("")), vec!["a", "b", "late", "old"]);
        assert_eq!(model.view, LibraryView::Grid);
        assert_eq!(model.groups("").len(), 1);
        assert!(model.groups("")[0].date.is_none());
    }

    #[test]
    fn favorite_and_trash_filters_use_host_state() {
        let mut favorite = item("a", "first.jpg", Category::Image, "09:58");
        favorite.favorite = true;
        let mut trashed = item("b", "second.jpg", Category::Image, "09:58");
        trashed.trashed = true;
        let mut model = LibraryModel::new(vec![favorite, trashed]);
        model.favorites_only = true;
        assert_eq!(ids(&model.filtered("")), vec!["a"]);
        model.showing_trash = true;
        model.favorites_only = false;
        assert_eq!(ids(&model.filtered("")), vec!["b"]);
        assert!(model.set_favorite("a", false));
        assert!(!model.is_favorite("a"));
        assert!(!model.set_favorite("missing", true));
    }

    #[test]
    fn tombstones_remain_in_metadata_but_never_reappear_in_library() {
        let mut deleted =
            item("deleted", "report.csv", Category::Table, "12:00");
        deleted.availability = ArtifactAvailability::Deleted;
        deleted.favorite = true;
        deleted.trashed = true;
        let mut model = LibraryModel::new(vec![deleted]);
        assert!(model.filtered("").is_empty());
        model.showing_trash = true;
        assert!(model.filtered("").is_empty());
        assert_eq!(model.items.len(), 1);
    }

    #[test]
    fn script_and_unknown_extensions_cannot_launch_default_application() {
        let mut record = item("script", "run.ps1", Category::Other, "12:00");
        record.extension = "ps1".into();
        assert!(!record.default_open_supported());
        record.extension = "unknown".into();
        assert!(!record.default_open_supported());
        record.extension = "pdf".into();
        assert!(record.default_open_supported());
        record.availability = ArtifactAvailability::Missing;
        assert!(!record.default_open_supported());
    }

    #[test]
    fn source_metadata_and_availability_default_for_old_fixture() {
        let records: Vec<LibraryItem> = serde_json::from_str(include_str!(
            "../../examples/fixtures/library.json"
        ))
        .expect("isolated example fixture must parse");
        let known = records
            .iter()
            .find(|item| item.id == "1104-byd-presentation")
            .expect("the isolated legacy fixture has its known presentation");
        assert_eq!(known.category, Category::Slides);
        assert_eq!(known.availability, ArtifactAvailability::Available);
        assert!(records.iter().all(|item| item.source_session_id.is_empty()));
        assert_eq!(LibraryModel::default().items.len(), 0);
    }
}
