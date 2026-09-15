use std::collections::HashSet;
use unicode_normalization::{char::is_combining_mark, UnicodeNormalization};

pub const MEMORY_QUERY_MAX_TERMS: usize = 20;
const MEMORY_QUERY_MAX_TERM_CHARS: usize = 64;

// These words describe Alfred's prompt plumbing more often than the user's
// task. Keep the list deliberately small and English-only. Identifiers bypass
// it, and matching still relies on SQLite's unicode61 tokenizer.
const BOILERPLATE_WORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "can", "context", "do", "does", "for", "from",
    "how", "i", "in", "is", "it", "memory", "of", "on", "or", "please", "run", "step", "task",
    "that", "the", "this", "to", "use", "we", "what", "when", "where", "which", "with", "workflow",
    "would", "you", "your",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryQueryTerm {
    /// Literal text passed to FTS inside quotes.
    pub text: String,
    /// Identifiers receive an additional ranking signal and never get removed
    /// as boilerplate.
    pub identifier: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemoryQuery {
    pub terms: Vec<MemoryQueryTerm>,
}

impl MemoryQuery {
    pub fn fts_or_query(&self) -> Option<String> {
        (!self.terms.is_empty()).then(|| {
            self.terms
                .iter()
                .map(Self::term_query)
                .collect::<Vec<_>>()
                .join(" OR ")
        })
    }

    pub fn term_query(term: &MemoryQueryTerm) -> String {
        format!(
            "\"{}\"{}",
            term.text.replace('"', "\"\""),
            if term.identifier { "" } else { "*" }
        )
    }
}

fn is_identifier(value: &str) -> bool {
    let has_letter = value.chars().any(char::is_alphabetic);
    let has_digit = value.chars().any(|character| character.is_ascii_digit());
    let has_separator = value.char_indices().any(|(index, character)| {
        matches!(character, '-' | '_' | '.' | ':' | '/' | '#')
            && index > 0
            && index + character.len_utf8() < value.len()
    });
    (has_letter && has_digit) || (has_separator && (has_letter || has_digit))
}

fn literal_segments(value: &str) -> Vec<String> {
    let mut segments = Vec::new();
    for part in value.split_whitespace() {
        let trimmed = part.trim_matches(|character: char| !character.is_alphanumeric());
        if trimmed.is_empty() {
            continue;
        }
        if is_identifier(trimmed) {
            segments.push(trimmed.to_string());
        } else {
            segments.extend(
                trimmed
                    .split(|character: char| !character.is_alphanumeric())
                    .filter(|segment| !segment.is_empty())
                    .map(str::to_string),
            );
        }
    }
    segments
}

fn normalized_literal(value: &str) -> String {
    value
        .nfkd()
        .filter(|character| !is_combining_mark(*character))
        .flat_map(char::to_lowercase)
        .collect()
}

pub fn contains_exact_identifier(value: &str, identifier: &str) -> bool {
    literal_segments(value)
        .into_iter()
        .filter(|segment| is_identifier(segment))
        .any(|segment| normalized_literal(&segment) == identifier)
}

/// Build a literal-safe disjunctive FTS query from bounded task text.
///
/// SQLite tokenizes quoted terms with unicode61 and removes diacritics in the
/// same way as the memory index. Punctuation-separated identifier components
/// are retained as a quoted phrase, while normal words are deduplicated.
pub fn build_memory_query(value: &str) -> MemoryQuery {
    let mut terms = Vec::new();
    let mut seen = HashSet::new();

    for raw in literal_segments(value) {
        if terms.len() >= MEMORY_QUERY_MAX_TERMS {
            break;
        }
        let bounded = raw
            .chars()
            .take(MEMORY_QUERY_MAX_TERM_CHARS)
            .collect::<String>();
        let identifier = is_identifier(&bounded);
        let normalized = normalized_literal(&bounded);
        if normalized.is_empty()
            || (!identifier
                && (normalized.chars().count() < 2
                    || BOILERPLATE_WORDS.contains(&normalized.as_str())))
            || !seen.insert(normalized.clone())
        {
            continue;
        }
        terms.push(MemoryQueryTerm {
            text: normalized,
            identifier,
        });
    }

    MemoryQuery { terms }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_literal_safe_or_query_and_deduplicates_boilerplate() {
        let query = build_memory_query(r#"Please use release release \"checklist\" OR secret"#);
        assert_eq!(
            query
                .terms
                .iter()
                .map(|term| term.text.as_str())
                .collect::<Vec<_>>(),
            vec!["release", "checklist", "secret"]
        );
        assert_eq!(
            query.fts_or_query().as_deref(),
            Some(r#""release"* OR "checklist"* OR "secret"*"#)
        );
    }

    #[test]
    fn preserves_identifiers_and_unicode_literals() {
        let query = build_memory_query("ERR_MEMORY_017 café 日本語の通知 2026-Q3");
        assert_eq!(query.terms.len(), 4);
        assert!(query.terms[0].identifier);
        assert!(!query.terms[1].identifier);
        assert!(query.terms[3].identifier);
        assert!(query.fts_or_query().unwrap().contains("cafe"));
        assert_eq!(
            MemoryQuery::term_query(&query.terms[0]),
            r#""err_memory_017""#
        );
    }

    #[test]
    fn empty_or_generic_text_abstains() {
        assert!(build_memory_query("please use this context for the task")
            .fts_or_query()
            .is_none());
    }

    #[test]
    fn trailing_punctuation_is_prose_and_diacritics_deduplicate() {
        let query = build_memory_query("Release. cafe café");
        assert_eq!(query.terms.len(), 2);
        assert_eq!(query.terms[0].text, "release");
        assert!(!query.terms[0].identifier);
        assert_eq!(query.terms[1].text, "cafe");
    }

    #[test]
    fn exact_identifier_check_rejects_prefixes() {
        assert!(contains_exact_identifier("Failure ERR12 occurred", "err12"));
        assert!(!contains_exact_identifier(
            "Failure ERR123 occurred",
            "err12",
        ));
        assert!(contains_exact_identifier(
            "Ticket ALFRED-4821.",
            "alfred-4821"
        ));
    }
}
