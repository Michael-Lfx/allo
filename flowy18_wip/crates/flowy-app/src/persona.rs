use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Profile {
    pub nickname: String,
    pub occupation: String,
    pub about: String,
    pub instructions: String,
    pub knowledge: Vec<Knowledge>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Knowledge {
    pub name: String,
    pub usage: String,
    pub body: String,
}

#[derive(Serialize, Deserialize)]
struct File {
    version: u32,
    #[serde(default)]
    nickname: String,
    #[serde(default)]
    occupation: String,
    #[serde(default)]
    about: String,
    #[serde(default)]
    instructions: String,
    #[serde(default)]
    knowledge: Vec<Knowledge>,
}

const VERSION: u32 = 1;
pub const NAME_MAX: usize = 120;
pub const ABOUT_MAX: usize = 2000;
pub const INSTRUCTIONS_MAX: usize = 3000;
pub const KNOWLEDGE_MAX: usize = 2000;

pub struct Patch {
    pub nickname: Option<String>,
    pub occupation: Option<String>,
    pub about: Option<String>,
    pub instructions: Option<String>,
    pub labeled: bool,
}

pub fn load(root: &Path) -> Profile {
    bounded(&read_file(root).unwrap_or_default())
}

pub fn save(root: &Path, profile: &Profile) {
    let profile = bounded(profile);
    let file = File {
        version: VERSION,
        nickname: profile.nickname,
        occupation: profile.occupation,
        about: profile.about,
        instructions: profile.instructions,
        knowledge: profile.knowledge,
    };
    let Ok(bytes) = serde_json::to_vec_pretty(&file) else {
        return;
    };
    let _ = crate::disk::commit(&root.join("profile.json"), &bytes);
}

fn read_file(root: &Path) -> Option<Profile> {
    let text = fs::read_to_string(root.join("profile.json")).ok()?;
    let file: File = serde_json::from_str(&text).ok()?;
    if file.version != VERSION {
        return None;
    }
    Some(Profile {
        nickname: file.nickname,
        occupation: file.occupation,
        about: file.about,
        instructions: file.instructions,
        knowledge: file.knowledge,
    })
}

fn bounded(profile: &Profile) -> Profile {
    Profile {
        nickname: limited(&profile.nickname, NAME_MAX),
        occupation: limited(&profile.occupation, NAME_MAX),
        about: limited(&profile.about, ABOUT_MAX),
        instructions: limited(&profile.instructions, INSTRUCTIONS_MAX),
        knowledge: profile
            .knowledge
            .iter()
            .map(|item| Knowledge {
                name: limited(&item.name, NAME_MAX),
                usage: limited(&item.usage, NAME_MAX),
                body: limited(&item.body, KNOWLEDGE_MAX),
            })
            .collect(),
    }
}

pub fn limited(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

pub fn read_patch(
    text: &str,
    nickname: &str,
    occupation: &str,
    about: &str,
    instructions: &str,
    none: &str,
) -> Patch {
    let mut patch = Patch {
        nickname: None,
        occupation: None,
        about: None,
        instructions: None,
        labeled: false,
    };
    for line in text.lines() {
        let line = line.trim();
        if let Some(value) = value_after(line, nickname) {
            patch.labeled = true;
            patch.nickname = keep(value, none);
        } else if let Some(value) = value_after(line, occupation) {
            patch.labeled = true;
            patch.occupation = keep(value, none);
        } else if let Some(value) = value_after(line, about) {
            patch.labeled = true;
            patch.about = keep(value, none);
        } else if let Some(value) = value_after(line, instructions) {
            patch.labeled = true;
            patch.instructions = keep(value, none);
        }
    }
    if !patch.labeled {
        let trimmed = text.trim();
        if !trimmed.is_empty() && !is_none(trimmed, none) {
            patch.about = Some(trimmed.to_string());
        }
    }
    patch
}

fn value_after<'a>(line: &'a str, label: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(label)?.trim_start();
    let rest = rest.strip_prefix(':').or_else(|| rest.strip_prefix('：'))?;
    Some(rest.trim())
}

fn keep(value: &str, none: &str) -> Option<String> {
    if is_none(value, none) {
        None
    } else {
        Some(value.to_string())
    }
}

fn is_none(value: &str, none: &str) -> bool {
    let value = value.trim().trim_matches(is_quote);
    if value.is_empty() {
        return true;
    }
    let bare = none.trim().trim_end_matches(['.', '。']);
    let got = value.trim_end_matches(['.', '。']);
    value == none || got == bare
}

const fn is_quote(ch: char) -> bool {
    ch == '"' || ch == '“' || ch == '”' || ch == '\''
}

#[cfg(test)]
mod tests {
    use super::{
        ABOUT_MAX, KNOWLEDGE_MAX, Knowledge, Profile, load, read_patch, save,
    };

    #[test]
    fn labeled_reply_fills_fields_and_skips_none() {
        let patch = read_patch(
            "昵称: Ada\n职业: 无可用信息。\n关于我: 分析师\n自定义指令: 用 Python",
            "昵称",
            "职业",
            "关于我",
            "自定义指令",
            "无可用信息。",
        );
        assert!(patch.labeled);
        assert_eq!(patch.nickname.as_deref(), Some("Ada"));
        assert_eq!(patch.occupation, None);
        assert_eq!(patch.about.as_deref(), Some("分析师"));
        assert_eq!(patch.instructions.as_deref(), Some("用 Python"));
    }

    #[test]
    fn plain_reply_becomes_about() {
        let patch = read_patch(
            "我住在上海",
            "昵称",
            "职业",
            "关于我",
            "自定义指令",
            "无可用信息。",
        );
        assert!(!patch.labeled);
        assert_eq!(patch.about.as_deref(), Some("我住在上海"));
    }

    #[test]
    fn fullwidth_colon_and_quoted_none_are_read() {
        let patch = read_patch(
            "昵称：\"无可用信息。\"\n职业：产品设计师",
            "昵称",
            "职业",
            "关于我",
            "自定义指令",
            "无可用信息。",
        );
        assert!(patch.labeled);
        assert_eq!(patch.nickname, None);
        assert_eq!(patch.occupation.as_deref(), Some("产品设计师"));
    }

    #[test]
    fn profile_roundtrip_trims_to_the_limit() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let dir = std::env::temp_dir().join(format!("flowy-profile-{nanos}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let profile = Profile {
            about: "字".repeat(ABOUT_MAX + 5),
            knowledge: vec![Knowledge {
                name: "规格".to_string(),
                usage: String::new(),
                body: "字".repeat(KNOWLEDGE_MAX + 3),
            }],
            ..Profile::default()
        };
        save(&dir, &profile);
        let loaded = load(&dir);
        assert_eq!(loaded.about.chars().count(), ABOUT_MAX);
        assert_eq!(loaded.knowledge[0].body.chars().count(), KNOWLEDGE_MAX);
        assert_eq!(loaded.knowledge[0].name, "规格");
        let _ = std::fs::remove_dir_all(dir);
    }
}
