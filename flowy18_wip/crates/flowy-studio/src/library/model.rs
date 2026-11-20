//! Library data and local view state. Data is supplied by the host; this module
//! never supplies demonstration records to the production application.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    #[default]
    All,
    Document,
    Website,
    Slides,
    Game,
    Table,
    Image,
    Video,
    Audio,
    MobileApp,
    Other,
}

impl Category {
    pub const ALL: [Self; 11] = [
        Self::All,
        Self::Document,
        Self::Website,
        Self::Slides,
        Self::Game,
        Self::Table,
        Self::Image,
        Self::Video,
        Self::Audio,
        Self::MobileApp,
        Self::Other,
    ];

    pub const fn key(self) -> &'static str {
        match self {
            Self::All => "library.category.all",
            Self::Document => "library.category.document",
            Self::Website => "library.category.website",
            Self::Slides => "library.category.slides",
            Self::Game => "library.category.game",
            Self::Table => "library.category.table",
            Self::Image => "library.category.image",
            Self::Video => "library.category.video",
            Self::Audio => "library.category.audio",
            Self::MobileApp => "library.category.mobile_app",
            Self::Other => "library.category.other",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryItem {
    pub id: String,
    pub title: String,
    pub description: String,
    pub category: Category,
    /// ISO calendar date, kept separate from the local display time.
    pub date: String,
    /// Local time in the sortable 24-hour `HH:MM` format.
    pub time: String,
    #[serde(default)]
    pub pending: bool,
    #[serde(default)]
    pub preview: Option<String>,
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

#[derive(Clone, Debug, Default)]
pub struct LibraryModel {
    pub items: Vec<LibraryItem>,
    pub category: Category,
    pub sort: LibrarySort,
    pub view: LibraryView,
    favorites: BTreeSet<String>,
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
            if let Some(group) = groups
                .iter_mut()
                .find(|group| group.date.as_deref() == Some(item.date.as_str()))
            {
                group.items.push(item);
            } else {
                groups.push(LibraryGroup {
                    date: Some(item.date.clone()),
                    items: vec![item],
                });
            }
        }
        groups
    }

    /// Search and category selection compose without changing source records.
    /// Timestamp ties always use the stable identifier, independent of input
    /// order and the selected sort direction.
    pub fn filtered(&self, query: &str) -> Vec<LibraryItem> {
        let query = query.trim().to_lowercase();
        let mut items: Vec<_> = self
            .items
            .iter()
            .filter(|item| {
                self.category == Category::All || item.category == self.category
            })
            .filter(|item| {
                if query.is_empty() {
                    return true;
                }
                let searchable = format!("{} {}", item.title, item.description)
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
                    .date
                    .cmp(&left.date)
                    .then_with(|| right.time.cmp(&left.time))
                    .then_with(|| left.id.cmp(&right.id))
            }),
            LibrarySort::Oldest => items.sort_by(|left, right| {
                left.date
                    .cmp(&right.date)
                    .then_with(|| left.time.cmp(&right.time))
                    .then_with(|| left.id.cmp(&right.id))
            }),
            LibrarySort::Name => items.sort_by_cached_key(|item| {
                (item.title.to_lowercase(), item.id.clone())
            }),
        }
        items
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let previous_len = self.items.len();
        self.items.retain(|item| item.id != id);
        self.favorites.remove(id);
        previous_len != self.items.len()
    }

    pub fn is_favorite(&self, id: &str) -> bool {
        self.favorites.contains(id)
    }

    /// Toggle one existing item's favorite state and return its new value.
    pub fn toggle_favorite(&mut self, id: &str) -> Option<bool> {
        if !self.items.iter().any(|item| item.id == id) {
            return None;
        }
        if self.favorites.remove(id) {
            Some(false)
        } else {
            self.favorites.insert(id.to_owned());
            Some(true)
        }
    }

    /// Empty names are rejected so a rename cannot make a row unidentifiable.
    pub fn rename(&mut self, id: &str, title: &str) -> bool {
        let title = title.trim();
        if title.is_empty() {
            return false;
        }
        let Some(item) = self.items.iter_mut().find(|item| item.id == id)
        else {
            return false;
        };
        item.title = title.to_string();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Category, LibraryItem, LibraryModel, LibrarySort, LibraryView,
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
        }
    }

    fn ids(items: &[LibraryItem]) -> Vec<&str> {
        items.iter().map(|item| item.id.as_str()).collect()
    }

    #[test]
    fn category_and_case_insensitive_search_compose() {
        let mut model = LibraryModel::new(vec![
            item("hero", "Dew & Stone Hero.jpg", Category::Image, "09:58"),
            item("notes", "Dew & Stone notes", Category::Document, "09:58"),
            item("sport", "sport.jpg", Category::Image, "09:58"),
        ]);
        model.category = Category::Image;
        assert_eq!(ids(&model.filtered("  DEW 照片  ")), vec!["hero"]);
        assert_eq!(model.filtered("missing").len(), 0);
        assert_eq!(model.items.len(), 3);
    }

    #[test]
    fn every_sort_is_deterministic_and_keeps_view_state() {
        let mut old = item("old", "Zebra", Category::Image, "20:00");
        old.date = "2026-11-03".into();
        let mut model = LibraryModel::new(vec![
            item("b", "beta", Category::Image, "09:58"),
            old,
            item("late", "Gamma", Category::Website, "17:22"),
            item("a", "Alpha", Category::Image, "09:58"),
        ]);
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
    fn name_sort_does_not_split_one_date_around_another() {
        let mut later = item("chris", "Chris", Category::Image, "09:00");
        later.date = "2026-11-04".into();
        let mut earlier = item("brian", "Brian", Category::Image, "09:00");
        earlier.date = "2026-11-03".into();
        let mut first = item("aaron", "Aaron", Category::Image, "09:00");
        first.date = "2026-11-04".into();
        let mut model = LibraryModel::new(vec![later, earlier, first]);
        model.sort = LibrarySort::Name;
        let named = model.groups("");
        assert_eq!(named.len(), 1);
        assert_eq!(ids(&named[0].items), vec!["aaron", "brian", "chris"]);
        model.sort = LibrarySort::Newest;
        let dated = model.groups("");
        assert_eq!(dated.len(), 2);
        assert_eq!(dated[0].date.as_deref(), Some("2026-11-04"));
        assert_eq!(ids(&dated[0].items), vec!["aaron", "chris"]);
        assert_eq!(dated[1].date.as_deref(), Some("2026-11-03"));
    }

    #[test]
    fn rename_and_remove_apply_to_the_selected_record_only() {
        let mut model = LibraryModel::new(vec![
            item("a", "first.jpg", Category::Image, "09:58"),
            item("b", "second.jpg", Category::Image, "09:58"),
        ]);
        assert!(!model.rename("a", "  "));
        assert!(!model.rename("absent", "name"));
        assert!(model.rename("a", " renamed.jpg "));
        assert_eq!(model.items[0].title, "renamed.jpg");
        assert_eq!(ids(&model.filtered("renamed")), vec!["a"]);
        assert_eq!(model.toggle_favorite("a"), Some(true));
        assert!(model.is_favorite("a"));
        assert_eq!(model.toggle_favorite("absent"), None);
        assert!(model.remove("a"));
        assert!(!model.is_favorite("a"));
        assert!(!model.remove("a"));
        assert_eq!(ids(&model.filtered("")), vec!["b"]);
    }

    #[test]
    fn example_fixture_covers_categories_and_reference_date_counts() {
        let records: Vec<LibraryItem> = serde_json::from_str(include_str!(
            "../../examples/fixtures/library.json"
        ))
        .expect("isolated example fixture must parse");
        for (date, expected) in [
            ("2026-11-04", 4),
            ("2026-11-03", 1),
            ("2026-11-02", 6),
            ("2026-10-28", 1),
            ("2026-10-20", 12),
        ] {
            assert_eq!(
                records.iter().filter(|item| item.date == date).count(),
                expected
            );
        }
        for category in Category::ALL.into_iter().skip(1) {
            assert!(records.iter().any(|item| item.category == category));
        }
        let ids: std::collections::BTreeSet<_> =
            records.iter().map(|item| item.id.as_str()).collect();
        assert_eq!(ids.len(), records.len());
        assert_eq!(LibraryModel::default().items.len(), 0);
    }
}
