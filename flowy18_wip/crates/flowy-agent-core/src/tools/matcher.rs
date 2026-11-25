//! A small dependency-free glob matcher supporting `*`, `?`, and `**`.
//!
//! - `*` matches any sequence of characters except `/`
//! - `?` matches exactly one character except `/`
//! - `**` matches any sequence of characters, including `/`

#[derive(Clone, Copy)]
enum GlobToken {
    Literal(char),
    AnyChar,
    Star,
    DoubleStar,
    DoubleStarSlash,
}

fn tokenize_glob(pattern: &str) -> Vec<GlobToken> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut tokens = Vec::with_capacity(chars.len());
    let mut i = 0_usize;
    while i < chars.len() {
        match chars[i] {
            '*' if i + 1 < chars.len() && chars[i + 1] == '*' => {
                // Collapse consecutive `*` runs (`***` -> `**`) and fold a
                // following `/` into `DoubleStarSlash` so `**/*.rs` matches
                // both zero and multiple directory segments.
                while i < chars.len() && chars[i] == '*' {
                    i += 1;
                }
                if i < chars.len() && chars[i] == '/' {
                    i += 1;
                    tokens.push(GlobToken::DoubleStarSlash);
                } else {
                    tokens.push(GlobToken::DoubleStar);
                }
            }
            '*' => {
                i += 1;
                if !matches!(tokens.last(), Some(GlobToken::Star)) {
                    tokens.push(GlobToken::Star);
                }
            }
            '?' => {
                i += 1;
                tokens.push(GlobToken::AnyChar);
            }
            c => {
                i += 1;
                tokens.push(GlobToken::Literal(c));
            }
        }
    }
    tokens
}

/// Match a glob `pattern` against a `/`-separated `path`.
///
/// Runs in polynomial time `O(|pattern| * |path|)` using iterative 2-row DP
/// (`O(|path|)` heap, `O(1)` stack) without backtracking or stack overflow.
pub fn glob_match(pattern: &str, path: &str) -> bool {
    let tokens = tokenize_glob(pattern);
    let s: Vec<char> = path.chars().collect();
    let n = s.len();
    let mut cur = vec![false; n + 1];
    let mut next = vec![false; n + 1];
    cur[0] = true;

    for token in tokens {
        next.fill(false);
        match token {
            GlobToken::Literal(c) => {
                for si in 0..n {
                    if cur[si] && s[si] == c {
                        next[si + 1] = true;
                    }
                }
            }
            GlobToken::AnyChar => {
                for si in 0..n {
                    if cur[si] && s[si] != '/' {
                        next[si + 1] = true;
                    }
                }
            }
            GlobToken::Star => {
                for si in 0..=n {
                    if cur[si] {
                        next[si] = true;
                    }
                }
                for si in 0..n {
                    if next[si] && s[si] != '/' {
                        next[si + 1] = true;
                    }
                }
            }
            GlobToken::DoubleStar => {
                let mut active = false;
                for si in 0..=n {
                    active |= cur[si];
                    if active {
                        next[si] = true;
                    }
                }
            }
            GlobToken::DoubleStarSlash => {
                // Zero-segment match (`**/*.rs` matches `lib.rs`) OR consume
                // any prefix ending at a `/` boundary (`src/lib.rs`).
                let mut active = false;
                for si in 0..n {
                    if cur[si] {
                        next[si] = true;
                        active = true;
                    }
                    if active && s[si] == '/' {
                        next[si + 1] = true;
                    }
                }
                if cur[n] {
                    next[n] = true;
                }
            }
        }
        std::mem::swap(&mut cur, &mut next);
    }
    cur[n]
}

#[cfg(test)]
mod tests {
    use super::glob_match;

    #[test]
    fn matches_basic_patterns() {
        assert!(glob_match("*.rs", "lib.rs"));
        assert!(!glob_match("*.rs", "lib.rs.bak"));
        assert!(
            !glob_match("*.rs", "src/lib.rs"),
            "* must not cross separators"
        );
        assert!(glob_match("**/*.rs", "src/lib.rs"));
        assert!(glob_match("**/*.rs", "lib.rs"));
        assert!(!glob_match("**/foo.rs", "barfoo.rs"));
        assert!(glob_match("src/**", "src/a/b/c"));
        assert!(glob_match("?.txt", "a.txt"));
        assert!(!glob_match("?.txt", "ab.txt"));
        assert!(glob_match("docs/*.md", "docs/readme.md"));
    }

    #[test]
    fn adversarial_multi_star_pattern_finishes_in_polynomial_time() {
        let pattern = "*a*a*a*a*a*a*a*a*a*a*a*a*b";
        let input = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        assert!(!glob_match(pattern, input));
        let deep_pattern = "**a**a**a**a**a**a**a**b";
        assert!(!glob_match(deep_pattern, "a/a/a/a/a/a/a/a/a/a/a/a/a/a/a/a"));
        // Long pattern must never overflow stack on small worker threads.
        let long_pattern = "*".repeat(25_000);
        assert!(glob_match(&long_pattern, "file.txt"));
    }
}
