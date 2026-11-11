use std::sync::Mutex;

static PENDING: Mutex<Vec<String>> = Mutex::new(Vec::new());

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeepLink {
    Invite { code: String },
    Open,
}

pub fn enqueue(urls: Vec<String>) {
    if urls.is_empty() {
        return;
    }
    if let Ok(mut guard) = PENDING.lock() {
        guard.extend(urls);
    }
}

pub fn take_pending() -> Vec<String> {
    PENDING.lock().map_or_else(|_| Vec::new(), |mut guard| {
        std::mem::take(&mut *guard)
    })
}

pub fn parse(url: &str) -> Option<DeepLink> {
    let scheme = crate::brand::url_scheme();
    let prefix = format!("{scheme}://");
    let rest = url.strip_prefix(&prefix)?;
    let rest = rest.trim_start_matches('/');
    if rest.is_empty() || rest.eq_ignore_ascii_case("open") {
        return Some(DeepLink::Open);
    }
    let mut parts = rest.split('/');
    let head = parts.next()?.to_ascii_lowercase();
    if head == "invite" {
        let code = parts.next()?.trim();
        if code.is_empty() || parts.next().is_some() {
            return None;
        }
        if !code
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
        {
            return None;
        }
        return Some(DeepLink::Invite {
            code: code.to_owned(),
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{DeepLink, parse};

    #[test]
    fn parses_invite_and_open() {
        let scheme = crate::brand::url_scheme();
        assert_eq!(
            parse(&format!("{scheme}://invite/AbC_12")),
            Some(DeepLink::Invite {
                code: "AbC_12".into()
            })
        );
        assert_eq!(
            parse(&format!("{scheme}://open")),
            Some(DeepLink::Open)
        );
        assert_eq!(parse(&format!("{scheme}://")), Some(DeepLink::Open));
        assert_eq!(parse(&format!("{scheme}://invite/")), None);
        assert_eq!(parse("https://example.com/invite/x"), None);
    }
}
