use std::{fs, path::Path};

use filetime::FileTime;
use fishmuse_library::{DiagnosticCode, LoftyTagReader, TagReader};
use tempfile::tempdir;

fn append_chunk(container: &mut Vec<u8>, id: &[u8; 4], data: &[u8]) {
    container.extend_from_slice(id);
    container.extend_from_slice(&(data.len() as u32).to_le_bytes());
    container.extend_from_slice(data);
    if !data.len().is_multiple_of(2) {
        container.push(0);
    }
}

fn generated_minimal_wav() -> Vec<u8> {
    let mut body = b"WAVE".to_vec();
    let mut format = Vec::new();
    format.extend_from_slice(&1_u16.to_le_bytes());
    format.extend_from_slice(&1_u16.to_le_bytes());
    format.extend_from_slice(&8_000_u32.to_le_bytes());
    format.extend_from_slice(&16_000_u32.to_le_bytes());
    format.extend_from_slice(&2_u16.to_le_bytes());
    format.extend_from_slice(&16_u16.to_le_bytes());
    append_chunk(&mut body, b"fmt ", &format);
    append_chunk(&mut body, b"data", &vec![0; 1_600]);

    let mut info = b"INFO".to_vec();
    append_chunk(&mut info, b"INAM", b"Generated Tone\0");
    append_chunk(&mut info, b"IART", b"Fixture Artist\0");
    append_chunk(&mut body, b"LIST", &info);

    let mut wav = b"RIFF".to_vec();
    wav.extend_from_slice(&(body.len() as u32).to_le_bytes());
    wav.extend_from_slice(&body);
    wav
}

fn write_fixture(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).expect("write generated fixture");
}

#[tokio::test]
async fn generated_minimal_wav_is_parsed_without_mutating_read_only_source() {
    let directory = tempdir().expect("temporary directory");
    let path = directory.path().join("generated.wav");
    write_fixture(&path, &generated_minimal_wav());
    let fixed_time = FileTime::from_unix_time(1_800_000_000, 123_000_000);
    filetime::set_file_mtime(&path, fixed_time).expect("set fixture mtime");
    let mut permissions = fs::metadata(&path).expect("fixture metadata").permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&path, permissions).expect("make fixture read-only");
    let bytes_before = fs::read(&path).expect("bytes before");
    let mtime_before = fs::metadata(&path)
        .expect("metadata before")
        .modified()
        .expect("mtime before");

    let parsed = LoftyTagReader
        .read(&path)
        .await
        .expect("parse generated wav");

    assert_eq!(parsed.title.as_deref(), Some("Generated Tone"));
    assert_eq!(parsed.artists, vec!["Fixture Artist"]);
    assert!(parsed.duration_ms.is_some_and(|duration| duration >= 90));
    assert_eq!(fs::read(&path).expect("bytes after"), bytes_before);
    assert_eq!(
        fs::metadata(&path)
            .expect("metadata after")
            .modified()
            .expect("mtime after"),
        mtime_before
    );
    assert!(
        fs::metadata(&path)
            .expect("permissions after")
            .permissions()
            .readonly()
    );
}

#[tokio::test]
async fn malformed_supported_wav_maps_to_invalid_tags() {
    let directory = tempdir().expect("temporary directory");
    let path = directory.path().join("malformed.wav");
    write_fixture(&path, b"RIFF\x04\0\0\0WAVE");

    let failure = LoftyTagReader
        .read(&path)
        .await
        .expect_err("malformed supported media");

    assert_eq!(failure.code, DiagnosticCode::InvalidTags);
}

#[tokio::test]
async fn unknown_media_format_maps_to_unsupported_format() {
    let directory = tempdir().expect("temporary directory");
    let path = directory.path().join("unknown.bin");
    write_fixture(&path, b"self-generated unknown format");

    let failure = LoftyTagReader
        .read(&path)
        .await
        .expect_err("unknown format");

    assert_eq!(failure.code, DiagnosticCode::UnsupportedFormat);
}
