//! Case-insensitive subsequence matching for the command palette and search
//! fields.
//!
//! The score rewards matches at the start of a word and consecutive runs, so
//! "sf" ranks "Save flow" above "Send final report". The algorithm is small
//! and deterministic, which keeps the palette's ordering stable while typing.

/// Returns a relevance score when every whitespace-separated token in `query`
/// appears in `candidate` in order. Higher scores are better.
#[must_use]
pub fn fuzzy_score(query: &str, candidate: &str) -> Option<i32> {
    let query = query.trim();
    if query.is_empty() {
        return Some(0);
    }

    let haystack: Vec<char> = candidate.chars().collect();
    let lower: Vec<char> =
        haystack.iter().flat_map(|ch| ch.to_lowercase()).collect();
    if lower.len() != haystack.len() {
        // Rare Unicode case expansions: compare through the lowercase string
        // instead of pretending one char maps to one lowercase char.
        return fuzzy_score_unicode(query, &lower);
    }

    let mut cursor = 0;
    let mut score = 0;
    for token in query.split_whitespace() {
        let mut matched = false;
        let mut previous_match: Option<usize> = None;
        for (index, needle) in
            token.chars().flat_map(|ch| ch.to_lowercase()).enumerate()
        {
            let mut found = None;
            for position in cursor..haystack.len() {
                if lower[position] == needle {
                    found = Some(position);
                    break;
                }
            }
            let Some(position) = found else {
                return None;
            };

            let mut bonus = 1;
            if token.chars().count() == token.chars().count() && index == 0 {
                bonus += 3;
            }
            if position == 0 || !haystack[position - 1].is_alphanumeric() {
                bonus += 6;
            }
            if previous_match == Some(position.wrapping_sub(1)) {
                bonus += 4;
            }
            if haystack[position] == needle {
                bonus += 1;
            }
            score += bonus;
            previous_match = Some(position);
            cursor = position + 1;
            matched = true;
        }
        if !matched {
            return None;
        }
    }

    Some(score)
}

fn fuzzy_score_unicode(query: &str, haystack: &[char]) -> Option<i32> {
    let joined: String = haystack.iter().collect();
    let lower = joined.to_lowercase();
    let mut cursor = 0;
    let mut score = 0;
    for token in query.split_whitespace() {
        let token = token.to_lowercase();
        let mut first = true;
        for needle in token.chars() {
            let rest = lower.get(cursor..)?;
            let offset = rest.find(needle)?;
            let position = cursor + offset;
            score += 1;
            if first {
                score += 3;
                first = false;
            }
            if position == 0
                || !joined[..position]
                    .chars()
                    .last()
                    .is_some_and(char::is_alphanumeric)
            {
                score += 6;
            }
            cursor = position + needle.len_utf8();
        }
    }
    Some(score)
}

/// Returns whether `query` matches `candidate` at all.
#[must_use]
pub fn fuzzy_matches(query: &str, candidate: &str) -> bool {
    fuzzy_score(query, candidate).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_query_matches_everything() {
        assert_eq!(fuzzy_score("", "anything"), Some(0));
        assert_eq!(fuzzy_score("   ", "anything"), Some(0));
    }

    #[test]
    fn prefix_matches_rank_above_scattered_matches() {
        let prefix = fuzzy_score("save", "Save flow").unwrap();
        let scattered = fuzzy_score("save", "Send a very early file").unwrap();
        assert!(prefix > scattered);
    }

    #[test]
    fn matching_is_case_insensitive() {
        assert!(fuzzy_matches("NEW FLOW", "New flow"));
        assert!(fuzzy_matches("newflow", "New flow"));
    }

    #[test]
    fn every_token_must_match_in_order() {
        assert!(fuzzy_matches("fit view", "Fit view to content"));
        assert!(!fuzzy_matches("view fit", "Fit view to content"));
        assert!(!fuzzy_matches("xyz", "Fit view to content"));
    }

    #[test]
    fn punctuation_creates_a_word_bonus() {
        let punctuation = fuzzy_score("sf", "Save flow").unwrap();
        let middle = fuzzy_score("sf", "Asdf").unwrap();
        assert!(punctuation > middle);
    }
}
