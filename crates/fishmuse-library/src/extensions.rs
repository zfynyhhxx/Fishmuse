use std::path::Path;

use crate::DiagnosticCode;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExtensionDisposition {
    Supported,
    Diagnostic(DiagnosticCode),
    Ignored,
}

#[must_use]
pub fn classify_extension(path: &Path) -> ExtensionDisposition {
    let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
        return ExtensionDisposition::Ignored;
    };

    if extension.eq_ignore_ascii_case("cue") {
        return ExtensionDisposition::Diagnostic(DiagnosticCode::UnsupportedCue);
    }
    if [
        "flac", "mp3", "m4a", "aac", "ogg", "opus", "wav", "aif", "aiff",
    ]
    .iter()
    .any(|supported| extension.eq_ignore_ascii_case(supported))
    {
        ExtensionDisposition::Supported
    } else {
        ExtensionDisposition::Ignored
    }
}
