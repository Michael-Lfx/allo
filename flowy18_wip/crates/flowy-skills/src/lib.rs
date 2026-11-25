//! Local skill catalog, cache, and installer.

mod archive;
mod catalog;
mod id;
mod install;
mod market;
mod official;
mod plan;
mod registry;
mod scan;
mod shelf;
mod store;
mod transport;
mod verdict;

pub use archive::{
    ArchiveError, DownloadBody, GitHubHandoff, classify_download,
    content_hash_matches, extract_github_skill, extract_skill_zip,
    github_archive_allowed, sha256_hex,
};
pub(crate) use archive::{collect_tree_hashes, normalize_relative};
pub use catalog::{
    CatalogError, CatalogPage, Category, parse_categories, parse_search_page,
    parse_skill_page,
};
pub use id::{IdError, SkillId, UNVERSIONED, parse_skill_id};
pub use install::{
    InstallError, InstallEvent, InstallOrigin, InstallOutcome, InstallProgress,
    InstallRecord, InstallStep, SkillInstaller, UpdateCheck, VerifiedFile,
    VerifyReceipt, accept_verify_body, advance, begin_community_install,
    cleanup_staging, download_url, install_dir, parse_verify_body, verify_url,
};
pub use market::{
    CachedShelf, MarketError, ShelfWindow, SkillMarket,
    category_cache_reusable, trending_cache_reusable,
};
pub use official::{
    OfficialError, OfficialSkill, bundled_markdown, bundled_official,
    parse_official, seed_official,
};
pub use plan::{
    CATEGORY_CARD_CAP, CATEGORY_TTL_SECS, Fetch, MAX_PACKAGE_BYTES,
    PAGE_LIMIT_CATEGORY, PAGE_LIMIT_SEARCH, PAGE_LIMIT_TRENDING, PlanError,
    SEARCH_CARD_CAP, SEARCH_TTL_SECS, TRENDING_CARD_CAP, category_url,
    percent_encode, search_url, trending_url,
};
pub use registry::{
    InstalledCatalog, InstalledSkill, RegistryError, escape_xml,
    format_available_skills, format_skill_invocation, load_installed,
    project_installed, prompt_lines,
};
pub use scan::{ScanError, scan_tree};
pub use shelf::{Card, keep_passing};
pub use store::{Store, StoreError};
pub use transport::{
    CatalogTransport, HttpBytes, HttpText, InstallTransport, NetTransport,
    TransportError, retry_delay,
};
pub use verdict::{VerdictError, apply_verdicts, passing_ids, verdict_body};
