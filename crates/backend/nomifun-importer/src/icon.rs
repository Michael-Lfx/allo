//! Marketplace icon placement (docs/agent-store/51-export-icons.zh.md §3.2).
//!
//! The marketplace declares **one icon per entry** (`icons/<base>.<ext>` in the
//! market root). That directory is a *sibling* of the entry directory, so
//! `copy_tree` can never carry the icon into the snapshot: a separate step has
//! to place it. This module is that step.
//!
//! Three placements, one per entity kind, each in the address space that entity
//! already uses (doc `51` §3.1) — no new byte face is introduced:
//!
//! | 实体 | 落位 | 字节面 | 导出字段 |
//! |---|---|---|---|
//! | 插件（专家 / 专家团） | `avatars/<icon-basename>` | `import/asset` | `pack.avatar`（已有） |
//! | 单技能 | `assets/icon.<ext>`（与 `SKILL.md` 同级） | `skill/file` | `skills[].icon` |
//! | 单连接器 | `avatars/connectors/<icon-basename>` | `import/asset` | `connectors[].icon` |
//!
//! The skill placement reuses the convention the built-in skills already
//! follow ([`SKILL_ICON_STEM`], mirroring `SKILL_ICON_CANDIDATES` in
//! `nomifun-extension`), which is why a market skill's icon ends up *inside* the
//! skill directory: it then rides `skill/files` → `skill/file` → export for free,
//! and materialises next to `SKILL.md` under the managed skills root.
//!
//! ## Three gates (doc `51` §3.2.2)
//!
//! 1. **Extension whitelist** — the same six the store asset endpoint serves.
//! 2. **512 KiB ceiling**, checked from metadata *before* reading.
//! 3. **Never overwrite** — an existing target means the author (or the package)
//!    already ships that file, and the author's declaration wins (doc `51`
//!    INV-4).
//!
//! A refused icon is a **warning, never a failed import**: an icon is display
//! data, and its absence must not block a package (doc `51` INV-6).

use std::path::{Path, PathBuf};

/// Icon extensions accepted from the market, in the host's probe order
/// (`nomifun-app/src/app_server_store.rs` `ICON_EXTS`).
pub const ICON_EXTENSIONS: [&str; 6] = ["png", "svg", "jpg", "jpeg", "webp", "gif"];

/// Largest market icon this face will copy. Real icons measure 10–100 KB; the
/// ceiling only has to stop "the wrong file" (a whole zip, a video), so it is
/// deliberately tighter than `MAX_SNAPSHOT_ASSET_BYTES` (2 MiB) — the icon face
/// has no use for that budget (doc `51` D9).
pub const MAX_MARKET_ICON_BYTES: u64 = 512 * 1024;

/// Fixed file stem a Skill icon must use: `assets/icon.<ext>`
/// (`SKILL_ICON_CANDIDATES` in `nomifun-extension/src/skill_service.rs`).
pub const SKILL_ICON_DIR: &str = "assets";
pub const SKILL_ICON_STEM: &str = "icon";

/// The extensions that convention accepts, in its probe order.
pub const SKILL_ICON_EXTENSIONS: [&str; 5] = ["png", "webp", "svg", "jpg", "jpeg"];

/// Plugin-level directory a connector icon lives in, so it can never be
/// confused with the package avatar that `pack.avatar` names. Both sit under
/// `avatars/`, which is already on the snapshot asset allowlist.
pub const CONNECTOR_ICON_DIR: &str = "avatars/connectors";

/// Where one market icon belongs inside the snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IconPlacement {
    /// Plugin entry → the package avatar (`avatars/<basename>`), stamped into
    /// the package's own agent / team payloads when they declare none.
    Package,
    /// Single-skill entry → `assets/icon.<ext>` beside `SKILL.md`.
    Skill,
    /// Single-connector entry → `avatars/connectors/<basename>`.
    Connector,
}

impl IconPlacement {
    /// The component kinds whose payload carries this icon's path — the payload
    /// keys the export face reads back (`51` §3.3.2).
    pub fn stamp_kinds(self) -> &'static [&'static str] {
        match self {
            Self::Package => &[crate::models::KIND_AGENT, crate::models::KIND_TEAM],
            Self::Skill => &[crate::models::KIND_SKILL],
            Self::Connector => &[crate::models::KIND_CONNECTOR],
        }
    }
}

/// Where this entry's marketplace icon belongs, or `None` when the entry holds
/// **several** components.
///
/// `icons/` carries one icon per *entry*, so it can only be attributed to an
/// entry that is one thing: a plugin package, one skill directory, one
/// connector directory. A `marketplace.json` declaring N skills, or a
/// `connectors.json` declaring N connectors, has no honest mapping — copying
/// the same icon to every component would invent an attribution (doc `51`
/// INV-3, D10).
pub fn icon_placement(
    parsed: &crate::manifest::ParsedManifest,
    kind: crate::models::SourceKind,
) -> Option<IconPlacement> {
    use crate::manifest::ParsedManifest;
    use crate::models::SourceKind;

    match (parsed, kind) {
        (ParsedManifest::Plugin(manifest), SourceKind::CodeBuddyPlugin) => {
            // The author already declared the package avatar, so the market icon
            // is a fallback that is not needed — and an unneeded fallback would
            // only add a file nothing references to the snapshot and to
            // `pack.assets` (INV-4 read strictly, doc `51` §3.2.2).
            manifest.avatar.is_none().then_some(IconPlacement::Package)
        }
        (ParsedManifest::SingleSkill(_), SourceKind::WorkBuddySkillMarket) => {
            Some(IconPlacement::Skill)
        }
        (ParsedManifest::Cli(..), SourceKind::WorkBuddyCliConnector)
        | (ParsedManifest::Mcp(..), SourceKind::WorkBuddyMcpConnector) => {
            Some(IconPlacement::Connector)
        }
        // Multi-component bundles — and any source kind that never has an icon
        // (the skill / connector *market* manifests) — place nothing.
        _ => None,
    }
}

/// One icon copied into the snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacedIcon {
    /// Snapshot-relative, POSIX-separated (`avatars/x.png` — or, for a skill,
    /// `assets/icon.png`, which is also skill-directory relative because the
    /// skill directory *is* the snapshot here).
    pub relative: String,
    /// Single-file sha256, lowercase hex — the same rule as `copy_tree`.
    pub digest_hex: String,
}

/// Result of trying to place a market icon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IconOutcome {
    /// Nothing to place: the request carried no icon, or the entry is a
    /// multi-component bundle (`None` placement).
    Absent,
    /// Copied at this path.
    Placed(PlacedIcon),
    /// Refused; the string is the reason, meant for the import's warnings.
    Refused(String),
}

/// Copy `icon` into `snapshot_dir` at the location `placement` dictates.
///
/// `icon` is the marketplace's absolute icon path (resolved by the marketplace
/// layer, doc `51` D5). `None` means "this entry has no icon".
pub fn place_market_icon(
    icon: Option<&Path>,
    snapshot_dir: &Path,
    placement: IconPlacement,
) -> IconOutcome {
    let Some(icon) = icon else {
        return IconOutcome::Absent;
    };
    let Some(ext) = icon
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
    else {
        return IconOutcome::Refused(format!(
            "市场图标没有扩展名，已跳过：{}",
            icon.display()
        ));
    };
    if !ICON_EXTENSIONS.contains(&ext.as_str()) {
        return IconOutcome::Refused(format!(
            "市场图标扩展名不在白名单（png/svg/jpg/jpeg/webp/gif），已跳过：{}",
            icon.display()
        ));
    }
    // Measured from metadata, before any read: an oversized file is refused
    // rather than loaded and then rejected (same shape as the asset face).
    let Ok(metadata) = std::fs::symlink_metadata(icon) else {
        return IconOutcome::Refused(format!("市场图标不可读，已跳过：{}", icon.display()));
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return IconOutcome::Refused(format!(
            "市场图标不是普通文件（符号链接或目录），已跳过：{}",
            icon.display()
        ));
    }
    if metadata.len() > MAX_MARKET_ICON_BYTES {
        return IconOutcome::Refused(format!(
            "市场图标超过 {} KiB 上限（{} 字节），已跳过：{}",
            MAX_MARKET_ICON_BYTES / 1024,
            metadata.len(),
            icon.display()
        ));
    }

    let target = match placement {
        IconPlacement::Package => PathBuf::from("avatars").join(file_name(icon)),
        IconPlacement::Skill => PathBuf::from(SKILL_ICON_DIR).join(format!("{SKILL_ICON_STEM}.{ext}")),
        IconPlacement::Connector => PathBuf::from("avatars")
            .join("connectors")
            .join(file_name(icon)),
    };
    let relative = target.to_string_lossy().replace('\\', "/");
    let destination = snapshot_dir.join(&target);
    // INV-4: the package (or the skill) already ships this file — the author's
    // declaration wins and nothing is overwritten.
    if destination.exists() {
        return IconOutcome::Refused(format!(
            "快照内已存在 {relative}，保留既有文件，市场图标未覆盖"
        ));
    }
    let Ok(bytes) = std::fs::read(icon) else {
        return IconOutcome::Refused(format!("市场图标不可读，已跳过：{}", icon.display()));
    };
    if let Some(parent) = destination.parent() {
        if let Err(error) = std::fs::create_dir_all(parent) {
            return IconOutcome::Refused(format!(
                "无法创建 {}：{error}",
                parent.display()
            ));
        }
    }
    if let Err(error) = std::fs::write(&destination, &bytes) {
        return IconOutcome::Refused(format!(
            "无法写入 {relative}：{error}"
        ));
    }
    IconOutcome::Placed(PlacedIcon {
        relative,
        digest_hex: crate::digest::sha256_hex(&bytes),
    })
}

/// The icon's own file name (`x.png`), the name both the package and the
/// connector placements keep so the file stays recognisable in the export.
fn file_name(icon: &Path) -> String {
    icon.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "market-icon.png".to_owned())
}

/// Probe a skill's own directory for `assets/icon.<ext>` and return the
/// **skill-directory-relative** path when one is there.
///
/// Mirrors `SKILL_ICON_CANDIDATES` in `nomifun-extension` — the same five
/// extensions, the same order — so the host and the extension layer can never
/// disagree about which file is a skill's icon.
///
/// `skill_dir` is the directory the component's `relative_path` points into
/// (`skills/<slug>/SKILL.md` → `skills/<slug>`). When that directory does not
/// exist in the snapshot, the entry is a **single-skill** one whose snapshot root
/// *is* the skill directory (see `InstallerService::materialize_skills`), so the
/// root is probed instead. The root is deliberately **not** probed as a fallback
/// for a package: a plugin's own `assets/` is not any one skill's, and handing it
/// to every skill would invent an attribution (docs `51` INV-3).
pub fn skill_icon_in_snapshot(snapshot_dir: &Path, skill_dir: &str) -> Option<String> {
    let declared = snapshot_dir.join(skill_dir);
    let base = if !skill_dir.is_empty() && declared.is_dir() {
        declared
    } else {
        snapshot_dir.to_path_buf()
    };
    for ext in SKILL_ICON_EXTENSIONS {
        let relative = format!("{SKILL_ICON_DIR}/{SKILL_ICON_STEM}.{ext}");
        if base.join(&relative).is_file() {
            return Some(relative);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn write(path: &Path, bytes: &[u8]) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, bytes).unwrap();
    }

    /// TC-IM-301: a plugin icon lands at `avatars/<basename>` and reports the
    /// digest `copy_tree` would have computed for the same bytes.
    #[test]
    fn places_a_package_icon_under_avatars() {
        let dir = temp();
        let icon = dir.path().join("market/icons/fbsir.svg");
        write(&icon, b"<svg/>");
        let snapshot = dir.path().join("snap");
        fs::create_dir_all(&snapshot).unwrap();

        let IconOutcome::Placed(placed) =
            place_market_icon(Some(&icon), &snapshot, IconPlacement::Package)
        else {
            panic!("expected a placed icon");
        };
        assert_eq!(placed.relative, "avatars/fbsir.svg");
        assert_eq!(placed.digest_hex, crate::digest::sha256_hex(b"<svg/>"));
        assert_eq!(fs::read(snapshot.join("avatars/fbsir.svg")).unwrap(), b"<svg/>");
    }

    /// TC-IM-303: a skill icon takes the fixed `assets/icon.<ext>` name so
    /// `SKILL_ICON_CANDIDATES` / `/api/skills/{id}/icon` find it.
    #[test]
    fn places_a_skill_icon_as_assets_icon() {
        let dir = temp();
        let icon = dir.path().join("market/icons/tencent-docs.svg");
        write(&icon, b"<svg>skill</svg>");
        let snapshot = dir.path().join("snap");
        fs::create_dir_all(&snapshot).unwrap();

        let IconOutcome::Placed(placed) =
            place_market_icon(Some(&icon), &snapshot, IconPlacement::Skill)
        else {
            panic!("expected a placed icon");
        };
        assert_eq!(placed.relative, "assets/icon.svg");
        assert!(snapshot.join("assets/icon.svg").is_file());
    }

    /// A connector icon is namespaced under `avatars/connectors/`, so it can
    /// never be mistaken for the package avatar.
    #[test]
    fn places_a_connector_icon_under_its_own_directory() {
        let dir = temp();
        let icon = dir.path().join("market/icons/fenbi.png");
        write(&icon, b"png-bytes");
        let snapshot = dir.path().join("snap");
        fs::create_dir_all(&snapshot).unwrap();

        let IconOutcome::Placed(placed) =
            place_market_icon(Some(&icon), &snapshot, IconPlacement::Connector)
        else {
            panic!("expected a placed icon");
        };
        assert_eq!(placed.relative, "avatars/connectors/fenbi.png");
    }

    /// TC-IM-305 (a): a non-whitelisted extension is refused, not imported.
    #[test]
    fn refuses_an_extension_outside_the_whitelist() {
        let dir = temp();
        let icon = dir.path().join("market/icons/x.bmp");
        write(&icon, b"bmp");
        let snapshot = dir.path().join("snap");
        fs::create_dir_all(&snapshot).unwrap();

        let outcome = place_market_icon(Some(&icon), &snapshot, IconPlacement::Package);
        assert!(matches!(outcome, IconOutcome::Refused(_)), "{outcome:?}");
        assert!(!snapshot.join("avatars").exists());
    }

    /// TC-IM-305 (b): an oversized icon is refused from metadata, before read.
    #[test]
    fn refuses_an_oversized_icon() {
        let dir = temp();
        let icon = dir.path().join("market/icons/huge.png");
        write(&icon, &vec![0u8; MAX_MARKET_ICON_BYTES as usize + 1]);
        let snapshot = dir.path().join("snap");
        fs::create_dir_all(&snapshot).unwrap();

        let outcome = place_market_icon(Some(&icon), &snapshot, IconPlacement::Package);
        match outcome {
            IconOutcome::Refused(reason) => assert!(reason.contains("上限"), "{reason}"),
            other => panic!("expected refusal, got {other:?}"),
        }
    }

    /// TC-IM-305 (c) / INV-4: an existing target wins — the market icon never
    /// overwrites what the package already ships.
    #[test]
    fn never_overwrites_an_existing_file() {
        let dir = temp();
        let icon = dir.path().join("market/icons/team.png");
        write(&icon, b"market-bytes");
        let snapshot = dir.path().join("snap");
        write(&snapshot.join("avatars/team.png"), b"author-bytes");

        let outcome = place_market_icon(Some(&icon), &snapshot, IconPlacement::Package);
        assert!(matches!(outcome, IconOutcome::Refused(_)), "{outcome:?}");
        assert_eq!(
            fs::read(snapshot.join("avatars/team.png")).unwrap(),
            b"author-bytes",
            "the author's file must survive untouched"
        );
    }

    /// A skill that already ships `assets/icon.png` keeps it (the author's own
    /// icon is the better one), and a `.svg` market icon does not even collide
    /// with it — different extension, different file.
    #[test]
    fn a_skills_own_icon_wins_but_another_extension_still_lands() {
        let dir = temp();
        let icon = dir.path().join("market/icons/skill.svg");
        write(&icon, b"market-svg");
        let snapshot = dir.path().join("snap");
        write(&snapshot.join("assets/icon.png"), b"author-png");

        let IconOutcome::Placed(placed) =
            place_market_icon(Some(&icon), &snapshot, IconPlacement::Skill)
        else {
            panic!("expected the svg to land next to the author's png");
        };
        assert_eq!(placed.relative, "assets/icon.svg");
        assert_eq!(fs::read(snapshot.join("assets/icon.png")).unwrap(), b"author-png");

        // The same extension, on the other hand, is a collision.
        let png = dir.path().join("market/icons/other.png");
        write(&png, b"market-png");
        assert!(matches!(
            place_market_icon(Some(&png), &snapshot, IconPlacement::Skill),
            IconOutcome::Refused(_)
        ));
    }

    /// No icon supplied is not a refusal: the caller has nothing to warn about.
    #[test]
    fn an_absent_icon_is_not_a_warning() {
        let dir = temp();
        let snapshot = dir.path().join("snap");
        fs::create_dir_all(&snapshot).unwrap();
        assert_eq!(
            place_market_icon(None, &snapshot, IconPlacement::Package),
            IconOutcome::Absent
        );
    }

    /// Uppercase extensions are normalised, so `ICON.PNG` is not a surprise
    /// refusal on a case-insensitive market export.
    #[test]
    fn normalises_the_extension_case() {
        let dir = temp();
        let icon = dir.path().join("market/icons/ICON.PNG");
        write(&icon, b"png");
        let snapshot = dir.path().join("snap");
        fs::create_dir_all(&snapshot).unwrap();

        let IconOutcome::Placed(placed) =
            place_market_icon(Some(&icon), &snapshot, IconPlacement::Skill)
        else {
            panic!("expected a placed icon");
        };
        assert_eq!(placed.relative, "assets/icon.png");
    }

    /// A **package's own** skill keeps the icon it ships at
    /// `skills/<slug>/assets/icon.*`; the probe must report it as a
    /// skill-directory-relative path (the `skill/file` coordinate system).
    #[test]
    fn a_packages_skill_finds_the_icon_in_its_own_directory() {
        let dir = temp();
        let snapshot = dir.path().join("snap");
        write(&snapshot.join("skills/alpha/assets/icon.webp"), b"webp-bytes");

        assert_eq!(
            skill_icon_in_snapshot(&snapshot, "skills/alpha").as_deref(),
            Some("assets/icon.webp")
        );
        assert_eq!(
            skill_icon_in_snapshot(&snapshot, "skills/beta"),
            None,
            "a skill without its own icon must not borrow its sibling's"
        );
    }

    /// The snapshot root is probed **only** when the declared skill directory is
    /// absent — the single-skill shape, whose root *is* the skill directory.
    /// For a package, a root `assets/` belongs to nobody: handing it to every
    /// skill would invent an attribution (INV-3).
    #[test]
    fn the_snapshot_root_is_probed_only_for_a_single_skill_entry() {
        let dir = temp();
        let snapshot = dir.path().join("snap");
        write(&snapshot.join("assets/icon.png"), b"root-icon");
        write(&snapshot.join("skills/alpha/SKILL.md"), b"# alpha");

        assert_eq!(
            skill_icon_in_snapshot(&snapshot, "skills/alpha"),
            None,
            "a package skill must not inherit the package's root assets/"
        );
        assert_eq!(
            skill_icon_in_snapshot(&snapshot, "").as_deref(),
            Some("assets/icon.png"),
            "a single-skill snapshot root is the skill directory"
        );
        assert_eq!(
            skill_icon_in_snapshot(&snapshot, "missing-dir").as_deref(),
            Some("assets/icon.png"),
            "an absent directory means the single-skill shape"
        );
    }
}
