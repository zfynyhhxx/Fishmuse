#![forbid(unsafe_code)]

mod canonicalize;
mod diagnostics;
mod extensions;
mod file_identity;
mod importer;
mod query;
mod scanner;
mod tag_reader;

pub use canonicalize::{CanonicalTags, canonicalize_tags, normalize_search_text};
pub use diagnostics::DiagnosticCode;
pub use extensions::{ExtensionDisposition, classify_extension};
pub use file_identity::{FileIdentity, QuickFileIdentity, discover_files, normalize_path_bytes};
pub use importer::{ImportOutcome, LocalImport, LocalLibraryImporter};
pub use query::{LibraryQueryPort, SearchQuery};
pub use scanner::{LibraryScanner, ScanProgress, ScanRequest, ScanStatus, ScanSummary};
pub use tag_reader::{LoftyTagReader, ParsedTags, ScanFailure, TagReader};
