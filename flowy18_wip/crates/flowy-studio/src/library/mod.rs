//! Resource browsing with injected data. Demo fixtures belong to examples.

pub mod model;
pub(crate) mod view;

pub use model::{
    Category, LibraryItem, LibraryModel, LibrarySort, LibraryView,
};
pub use view::LibraryPanel;
