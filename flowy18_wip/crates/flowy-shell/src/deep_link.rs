use std::sync::Mutex;

static PENDING: Mutex<Vec<String>> = Mutex::new(Vec::new());

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeepLink {
    Invite { code: String },
    AutomationRun { run_id: String },
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
    PENDING
        .lock()
        .map_or_else(|_| Vec::new(), |mut guard| std::mem::take(&mut *guard))
}

pub fn parse(url: &str) -> Option<DeepLink> {
    parse_with_scheme(url, crate::brand().url_scheme)
}

pub fn parse_with_scheme(url: &str, scheme: &str) -> Option<DeepLink> {
    let prefix = format!("{scheme}://");
    let rest = url.strip_prefix(&prefix)?;
    let rest = rest.trim_start_matches('/');
    if rest.is_empty() || rest.eq_ignore_ascii_case("open") {
        return Some(DeepLink::Open);
    }
    let mut parts = rest.split('/');
    let head = parts.next()?.to_ascii_lowercase();
    if head == "automation-run" {
        let run_id = parts.next()?;
        return (parts.next().is_none() && valid_run_route(run_id)).then(
            || DeepLink::AutomationRun {
                run_id: run_id.to_owned(),
            },
        );
    }
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

pub fn valid_run_route(id: &str) -> bool {
    id.len() == 36
        && id.as_bytes()[14] == b'7'
        && matches!(id.as_bytes()[19], b'8' | b'9' | b'a' | b'b')
        && id.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                matches!(byte, b'0'..=b'9' | b'a'..=b'f')
            }
        })
}

#[cfg(test)]
mod tests {
    use super::{DeepLink, parse_with_scheme};

    #[test]
    fn parses_invite_and_open() {
        let scheme = "flowy";
        assert_eq!(
            parse_with_scheme(&format!("{scheme}://invite/AbC_12"), scheme),
            Some(DeepLink::Invite {
                code: "AbC_12".into()
            })
        );
        assert_eq!(
            parse_with_scheme(&format!("{scheme}://open"), scheme),
            Some(DeepLink::Open)
        );
        assert_eq!(
            parse_with_scheme(&format!("{scheme}://"), scheme),
            Some(DeepLink::Open)
        );
        assert_eq!(
            parse_with_scheme(&format!("{scheme}://invite/"), scheme),
            None
        );
        assert_eq!(
            parse_with_scheme("https://example.com/invite/x", scheme),
            None
        );
    }
}
