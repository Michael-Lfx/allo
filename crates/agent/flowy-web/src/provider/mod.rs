pub mod duckduckgo;
pub mod extract;
pub mod html_md;
pub mod search;
pub mod ssrf;

pub use duckduckgo::DuckDuckGoSearchProvider;
pub use extract::ExtractProvider;
pub use search::SearchProvider;
