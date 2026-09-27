use unicode_normalization::UnicodeNormalization;

use crate::ParsedTags;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalTags {
    pub display: ParsedTags,
    pub search_title: Option<String>,
    pub search_artists: Vec<String>,
    pub search_release: Option<String>,
}

#[must_use]
pub fn canonicalize_tags(display: ParsedTags) -> CanonicalTags {
    CanonicalTags {
        search_title: display.title.as_deref().map(normalize_search_text),
        search_artists: display
            .artists
            .iter()
            .map(|artist| normalize_search_text(artist))
            .collect(),
        search_release: display.album.as_deref().map(normalize_search_text),
        display,
    }
}

#[must_use]
pub fn normalize_search_text(value: &str) -> String {
    value
        .nfkc()
        .flat_map(char::to_lowercase)
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
