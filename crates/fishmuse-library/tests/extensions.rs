use std::path::Path;

use fishmuse_library::{DiagnosticCode, ExtensionDisposition, classify_extension};

#[test]
fn recognizes_the_complete_supported_set_case_insensitively() {
    for extension in [
        "FlAc", "MP3", "m4A", "AaC", "oGg", "OPUS", "Wav", "AIF", "aIfF",
    ] {
        let path = format!("track.{extension}");
        assert_eq!(
            classify_extension(Path::new(&path)),
            ExtensionDisposition::Supported,
            "{extension} should be supported"
        );
    }
}

#[test]
fn cue_is_an_explicit_diagnostic_and_unknown_extensions_are_ignored() {
    assert_eq!(
        classify_extension(Path::new("album.CuE")),
        ExtensionDisposition::Diagnostic(DiagnosticCode::UnsupportedCue)
    );
    assert_eq!(
        classify_extension(Path::new("cover.jpg")),
        ExtensionDisposition::Ignored
    );
    assert_eq!(
        classify_extension(Path::new("README")),
        ExtensionDisposition::Ignored
    );
}
