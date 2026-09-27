#![forbid(unsafe_code)]

mod diagnostics;
mod extensions;
mod file_identity;
mod scanner;
mod tag_reader;

pub use diagnostics::DiagnosticCode;
pub use extensions::{ExtensionDisposition, classify_extension};
pub use file_identity::{FileIdentity, QuickFileIdentity, discover_files, normalize_path_bytes};
pub use scanner::{LibraryScanner, ScanProgress, ScanRequest, ScanStatus, ScanSummary};
pub use tag_reader::{LoftyTagReader, ParsedTags, ScanFailure, TagReader};
