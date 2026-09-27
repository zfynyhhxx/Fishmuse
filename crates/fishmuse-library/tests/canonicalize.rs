use fishmuse_library::{ParsedTags, canonicalize_tags, normalize_search_text};

#[test]
fn normalization_collapses_whitespace_unicode_and_case_without_changing_display_tags() {
    let tags = ParsedTags {
        title: Some("  Cafe\u{301}\tLIVE  ".to_owned()),
        artists: vec!["  ARTIST\nA  ".to_owned()],
        album: Some("  The\r\nAlbum ".to_owned()),
        ..ParsedTags::default()
    };

    let canonical = canonicalize_tags(tags.clone());

    assert_eq!(canonical.display, tags);
    assert_eq!(canonical.search_title.as_deref(), Some("caf\u{e9} live"));
    assert_eq!(canonical.search_artists, ["artist a"]);
    assert_eq!(canonical.search_release.as_deref(), Some("the album"));
    assert_eq!(
        normalize_search_text("  STRASSE\t\u{212b}  "),
        "strasse \u{e5}"
    );
}

#[test]
fn textual_variants_remain_observably_distinct_after_normalization() {
    let values = [
        "Artist A",
        "Artist A feat. B",
        "Song (Original)",
        "Song (Remaster)",
        "Song (Live)",
        "Song (Mono)",
        "Song (Stereo)",
    ];

    let normalized: Vec<_> = values
        .iter()
        .map(|value| normalize_search_text(value))
        .collect();

    for (index, value) in normalized.iter().enumerate() {
        assert!(
            !normalized[..index].contains(value),
            "normalization must not erase identity-bearing text: {value}"
        );
    }
}
