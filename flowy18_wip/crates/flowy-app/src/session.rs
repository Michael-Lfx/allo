use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

pub const SESSION_TTL_SECS: u64 = 14 * 24 * 60 * 60;
const FORMAT_VERSION: u32 = 1;
const HASH_ROUNDS: usize = 20_000;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AuthError {
    #[error("请输入有效的邮箱")]
    InvalidEmail,
    #[error("密码至少 8 位")]
    ShortPassword,
    #[error("没有这个账号")]
    UnknownAccount,
    #[error("密码不正确")]
    WrongPassword,
    #[error("无法保存登录状态")]
    Store,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    pub email: String,
    pub display_name: String,
    pub expires_unix: u64,
}

#[derive(Debug)]
pub struct Store {
    root: PathBuf,
}

#[derive(Serialize, Deserialize)]
struct SessionFile {
    version: u32,
    token: String,
    email: String,
    display_name: String,
    issued_unix: u64,
    expires_unix: u64,
}

#[derive(Serialize, Deserialize, Default)]
struct AccountFile {
    version: u32,
    accounts: Vec<AccountRecord>,
}

#[derive(Serialize, Deserialize)]
struct AccountRecord {
    email: String,
    salt_hex: String,
    password_hash_hex: String,
}

impl Store {
    #[must_use]
    pub const fn new(root: PathBuf) -> Self {
        Self { root }
    }

    #[must_use]
    pub fn default_root() -> PathBuf {
        directories::ProjectDirs::from(
            crate::brand::qualifier_tld(),
            crate::brand::qualifier_org(),
            crate::brand::qualifier_app(),
        )
        .map(|dirs| dirs.data_dir().to_path_buf())
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
    }

    #[must_use]
    pub fn load(&self) -> Option<Session> {
        let file: SessionFile = read_json(&self.root.join("session.json"))?;
        if file.version != FORMAT_VERSION || file.token.is_empty() {
            return None;
        }
        let now = unix_now()?;
        if file.expires_unix <= now {
            return None;
        }
        Some(Session {
            email: file.email,
            display_name: file.display_name,
            expires_unix: file.expires_unix,
        })
    }

    #[must_use]
    pub fn has_accounts(&self) -> bool {
        self.read_accounts()
            .is_some_and(|file| !file.accounts.is_empty())
    }

    pub fn sign_in(
        &self,
        email: &str,
        password: &str,
    ) -> Result<Session, AuthError> {
        let email = normalize_email(email);
        if !valid_email(&email) {
            return Err(AuthError::InvalidEmail);
        }
        if password.chars().count() < 8 {
            return Err(AuthError::ShortPassword);
        }

        let mut accounts = self.read_accounts().unwrap_or_default();
        if accounts.version == 0 {
            accounts.version = FORMAT_VERSION;
        }
        if let Some(existing) = accounts
            .accounts
            .iter()
            .find(|account| account.email == email)
        {
            if !password_matches(existing, password) {
                return Err(AuthError::WrongPassword);
            }
        } else if accounts.accounts.is_empty() {
            let record = new_account(&email, password)?;
            accounts.accounts.push(record);
            self.write_accounts(&accounts)?;
        } else {
            return Err(AuthError::UnknownAccount);
        }

        let now = unix_now().ok_or(AuthError::Store)?;
        let session = Session {
            display_name: display_name(&email),
            email: email.clone(),
            expires_unix: now.saturating_add(SESSION_TTL_SECS),
        };
        let file = SessionFile {
            version: FORMAT_VERSION,
            token: random_hex(32)?,
            email,
            display_name: session.display_name.clone(),
            issued_unix: now,
            expires_unix: session.expires_unix,
        };
        write_json(&self.root.join("session.json"), &file)?;
        Ok(session)
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn clear_session(&self) {
        let _ = fs::remove_file(self.root.join("session.json"));
    }

    fn read_accounts(&self) -> Option<AccountFile> {
        read_json(&self.root.join("accounts.json"))
    }

    fn write_accounts(&self, accounts: &AccountFile) -> Result<(), AuthError> {
        write_json(&self.root.join("accounts.json"), accounts)
    }
}

fn normalize_email(email: &str) -> String {
    email.trim().to_lowercase()
}

pub fn valid_email(email: &str) -> bool {
    let Some((local, domain)) = email.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !domain.contains('@')
}

fn display_name(email: &str) -> String {
    let local = email.split('@').next().unwrap_or(email);
    let mut chars = local.chars();
    let Some(first) = chars.next() else {
        return "用户".to_string();
    };
    let mut name = first.to_uppercase().collect::<String>();
    name.extend(chars);
    name
}

fn new_account(
    email: &str,
    password: &str,
) -> Result<AccountRecord, AuthError> {
    let salt = random_bytes(16)?;
    let hash = hash_password(&salt, password);
    Ok(AccountRecord {
        email: email.to_string(),
        salt_hex: hex_encode(&salt),
        password_hash_hex: hex_encode(&hash),
    })
}

fn password_matches(account: &AccountRecord, password: &str) -> bool {
    let Some(salt) = hex_decode(&account.salt_hex) else {
        return false;
    };
    let actual = hex_encode(&hash_password(&salt, password));
    constant_eq(actual.as_bytes(), account.password_hash_hex.as_bytes())
}

fn hash_password(salt: &[u8], password: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(salt);
    hasher.update(password.as_bytes());
    let mut digest: [u8; 32] = hasher.finalize().into();
    for _ in 0..HASH_ROUNDS {
        let mut hasher = Sha256::new();
        hasher.update(salt);
        hasher.update(digest);
        digest = hasher.finalize().into();
    }
    digest
}

fn random_hex(len: usize) -> Result<String, AuthError> {
    Ok(hex_encode(&random_bytes(len)?))
}

fn random_bytes(len: usize) -> Result<Vec<u8>, AuthError> {
    let mut bytes = vec![0_u8; len];
    getrandom::fill(&mut bytes).map_err(|_| AuthError::Store)?;
    Ok(bytes)
}

fn unix_now() -> Option<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs())
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), AuthError> {
    let bytes =
        serde_json::to_vec_pretty(value).map_err(|_| AuthError::Store)?;
    crate::disk::commit(path, &bytes).map_err(|_| AuthError::Store)
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(HEX[usize::from(byte >> 4)]));
        out.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    out
}

fn hex_decode(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len() / 2);
    let mut index = 0;
    while index < bytes.len() {
        let hi = from_hex(bytes[index])?;
        let lo = from_hex(bytes[index + 1])?;
        out.push((hi << 4) | lo);
        index += 2;
    }
    Some(out)
}

const fn from_hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn constant_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut diff = 0_u8;
    for (lhs, rhs) in left.iter().zip(right.iter()) {
        diff |= lhs ^ rhs;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::{AuthError, FORMAT_VERSION, SESSION_TTL_SECS, Store};
    use serde_json::json;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_store() -> Store {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let root =
            std::env::temp_dir().join(format!("flowy-auth-{nanos}-{id}"));
        let _ = fs::remove_dir_all(&root);
        Store::new(root)
    }

    #[test]
    fn first_sign_in_creates_a_session_that_loads() {
        let store = temp_store();
        assert!(!store.has_accounts());
        let session =
            store.sign_in("Ada@Example.com", "correct-horse").unwrap();
        assert_eq!(session.email, "ada@example.com");
        assert_eq!(session.display_name, "Ada");
        assert!(
            session.expires_unix - session_issued(&store)
                >= SESSION_TTL_SECS - 2
        );
        let loaded = store.load().unwrap();
        assert_eq!(loaded, session);
        assert!(store.has_accounts());
    }

    #[test]
    fn existing_account_rejects_the_wrong_password_and_unknown_email() {
        let store = temp_store();
        store.sign_in("ada@example.com", "correct-horse").unwrap();
        assert_eq!(
            store.sign_in("ada@example.com", "wrong-password"),
            Err(AuthError::WrongPassword)
        );
        assert_eq!(
            store.sign_in("other@example.com", "correct-horse"),
            Err(AuthError::UnknownAccount)
        );
        store.clear_session();
        assert!(store.load().is_none());
        let again = store.sign_in("ada@example.com", "correct-horse").unwrap();
        assert_eq!(again.email, "ada@example.com");
    }

    #[test]
    fn expired_or_corrupt_session_stays_signed_out() {
        let store = temp_store();
        store.sign_in("ada@example.com", "correct-horse").unwrap();
        let path = store.root.join("session.json");
        let mut value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        value["expires_unix"] = json!(1);
        fs::write(&path, value.to_string()).unwrap();
        assert!(store.load().is_none());

        fs::write(&path, "{").unwrap();
        assert!(store.load().is_none());
        assert_eq!(FORMAT_VERSION, 1);
    }

    #[test]
    fn rejects_bad_input() {
        let store = temp_store();
        assert_eq!(
            store.sign_in("not-an-email", "correct-horse"),
            Err(AuthError::InvalidEmail)
        );
        assert_eq!(
            store.sign_in("ada@example.com", "short"),
            Err(AuthError::ShortPassword)
        );
    }

    fn session_issued(store: &Store) -> u64 {
        let text = fs::read_to_string(store.root.join("session.json")).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        value["issued_unix"].as_u64().unwrap()
    }
}
