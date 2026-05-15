use std::collections::BTreeMap;

// 3-gram index for spell correction and wildcard queries.
//
// Builds character-level trigrams (NOT byte trigrams) so we never slice
// strings at non-char-boundaries. Even though our tokenizer should produce
// ASCII-only tokens after normalization, defense in depth: never panic on
// a stray Unicode char that slipped through.

/// Build a 3-gram index for a term — break it into 3-character sequences
/// with $ markers, and map each trigram to the list of terms containing it.
pub fn three_gram_index(term: &str, gram_index: &mut BTreeMap<String, Vec<String>>) {
    let padded: Vec<char> = std::iter::once('$')
        .chain(term.chars())
        .chain(std::iter::once('$'))
        .collect();

    // need at least 3 chars to form a trigram; shorter strings are skipped
    if padded.len() < 3 {
        return;
    }

    for window in padded.windows(3) {
        let gram: String = window.iter().collect();
        let term_list = gram_index.entry(gram).or_insert(Vec::new());
        let term_owned = term.to_string();
        if !term_list.contains(&term_owned) {
            term_list.push(term_owned);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_term_produces_expected_trigrams() {
        let mut idx = BTreeMap::new();
        three_gram_index("cat", &mut idx);
        // "$cat$" -> "$ca", "cat", "at$"
        assert!(idx.contains_key("$ca"));
        assert!(idx.contains_key("cat"));
        assert!(idx.contains_key("at$"));
        assert_eq!(idx.len(), 3);
    }

    #[test]
    fn unicode_term_does_not_panic() {
        // Even if a Unicode char slipped past tokenizer normalization,
        // this must never panic. (The actual tokenizer normalizes ﬁ -> fi,
        // so this is defense in depth.)
        let mut idx = BTreeMap::new();
        three_gram_index("signiﬁcant", &mut idx);
        // We don't care about the exact trigrams here — just that it doesn't crash.
        assert!(!idx.is_empty());
    }

    #[test]
    fn short_term_handled_gracefully() {
        let mut idx = BTreeMap::new();
        three_gram_index("a", &mut idx);
        // "$a$" is 3 chars -> 1 trigram
        assert_eq!(idx.len(), 1);
        assert!(idx.contains_key("$a$"));
    }

    #[test]
    fn empty_term_handled_gracefully() {
        let mut idx = BTreeMap::new();
        three_gram_index("", &mut idx);
        // "$$" is 2 chars -> no trigrams, no panic
        assert_eq!(idx.len(), 0);
    }

    #[test]
    fn dedupes_same_term_added_twice() {
        let mut idx = BTreeMap::new();
        three_gram_index("cat", &mut idx);
        three_gram_index("cat", &mut idx);
        // Each trigram should still list "cat" only once
        for terms in idx.values() {
            assert_eq!(terms.iter().filter(|t| *t == "cat").count(), 1);
        }
    }
}