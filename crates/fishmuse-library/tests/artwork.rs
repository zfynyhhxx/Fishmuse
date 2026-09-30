use std::path::Path;

use fishmuse_domain::MediaAssetId;
use fishmuse_library::{
    ArtworkResolver, LocalImport, LocalLibraryImporter, ParsedTags, normalize_path_bytes,
};
use fishmuse_storage::Database;
use tempfile::TempDir;

const MAX_ARTWORK_BYTES: usize = 5 * 1024 * 1024;

#[cfg(windows)]
fn native_path_bytes(path: &Path) -> Vec<u8> {
    use std::os::windows::ffi::OsStrExt;

    path.as_os_str()
        .encode_wide()
        .flat_map(u16::to_le_bytes)
        .collect()
}

#[cfg(unix)]
fn native_path_bytes(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;

    path.as_os_str().as_bytes().to_vec()
}

#[cfg(not(any(unix, windows)))]
fn native_path_bytes(path: &Path) -> Vec<u8> {
    path.as_os_str().to_string_lossy().as_bytes().to_vec()
}

fn jpeg(length: usize, marker: u8) -> Vec<u8> {
    let mut bytes = vec![marker; length.max(4)];
    bytes[..4].copy_from_slice(&[0xff, 0xd8, 0xff, 0xe0]);
    bytes
}

fn png(marker: u8) -> Vec<u8> {
    let mut bytes = vec![marker; 32];
    bytes[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
    bytes
}

fn webp(marker: u8) -> Vec<u8> {
    let mut bytes = vec![marker; 32];
    bytes[..4].copy_from_slice(b"RIFF");
    bytes[8..12].copy_from_slice(b"WEBP");
    bytes
}

fn id3_with_front_cover(art: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    body.push(0); // ISO-8859-1 text encoding
    body.extend_from_slice(b"image/jpeg\0");
    body.push(3); // front cover
    body.push(0); // empty description
    body.extend_from_slice(art);

    let mut frame = Vec::new();
    frame.extend_from_slice(b"APIC");
    frame.extend_from_slice(&(body.len() as u32).to_be_bytes());
    frame.extend_from_slice(&[0, 0]);
    frame.extend_from_slice(&body);

    let size = frame.len() as u32;
    let syncsafe = [
        ((size >> 21) & 0x7f) as u8,
        ((size >> 14) & 0x7f) as u8,
        ((size >> 7) & 0x7f) as u8,
        (size & 0x7f) as u8,
    ];
    let mut file = b"ID3\x03\x00\x00".to_vec();
    file.extend_from_slice(&syncsafe);
    file.extend_from_slice(&frame);
    for _ in 0..2 {
        file.extend_from_slice(&[0xff, 0xfb, 0x90, 0x64]);
        file.extend_from_slice(&[0; 413]);
    }
    file
}

async fn mapped_resolver(
    directory: &TempDir,
    file_name: &str,
    media_bytes: &[u8],
) -> (
    ArtworkResolver,
    fishmuse_domain::UserId,
    fishmuse_domain::TrackId,
) {
    let path = directory.path().join(file_name);
    std::fs::write(&path, media_bytes).expect("write media fixture");
    let database = Database::open_in_memory().await.expect("database");
    let user_id = database.ensure_local_user().await.expect("local user");
    let imported = LocalLibraryImporter::new(database.pool().clone(), user_id)
        .import(LocalImport {
            media_asset_id: MediaAssetId::new(),
            normalized_path: normalize_path_bytes(&path),
            original_path: native_path_bytes(&path),
            content_fingerprint: format!("fixture-{file_name}"),
            fallback_title: "Fixture".to_owned(),
            tags: ParsedTags {
                title: Some("Fixture".to_owned()),
                artists: vec!["Artist".to_owned()],
                ..ParsedTags::default()
            },
        })
        .await
        .expect("import fixture");
    (
        ArtworkResolver::new(database.pool().clone()),
        user_id,
        imported.track_id,
    )
}

#[tokio::test]
async fn embedded_front_art_has_priority_over_directory_candidates() {
    let directory = tempfile::tempdir().unwrap();
    let embedded = jpeg(32, 0x11);
    let (resolver, user, track) =
        mapped_resolver(&directory, "song.mp3", &id3_with_front_cover(&embedded)).await;
    std::fs::write(directory.path().join("cover.png"), png(0x22)).unwrap();

    let artwork = resolver.resolve(user, track).await.unwrap().unwrap();
    assert_eq!(artwork.mime_type, "image/jpeg");
    assert_eq!(artwork.bytes, embedded);
}

#[tokio::test]
async fn directory_names_are_case_insensitive_and_use_cover_folder_front_priority() {
    let directory = tempfile::tempdir().unwrap();
    let (resolver, user, track) = mapped_resolver(&directory, "song.wav", b"not tagged").await;
    std::fs::write(directory.path().join("FRONT.webp"), webp(0x33)).unwrap();
    std::fs::write(directory.path().join("Folder.PNG"), png(0x22)).unwrap();
    let expected = jpeg(32, 0x11);
    std::fs::write(directory.path().join("CoVeR.JpEg"), &expected).unwrap();

    let artwork = resolver.resolve(user, track).await.unwrap().unwrap();
    assert_eq!(artwork.mime_type, "image/jpeg");
    assert_eq!(artwork.bytes, expected);
}

#[tokio::test]
async fn accepts_jpeg_png_and_webp_and_returns_none_when_art_is_absent() {
    for (name, bytes, mime) in [
        ("cover.jpg", jpeg(32, 1), "image/jpeg"),
        ("cover.png", png(2), "image/png"),
        ("cover.webp", webp(3), "image/webp"),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let (resolver, user, track) = mapped_resolver(&directory, "song.wav", b"media").await;
        std::fs::write(directory.path().join(name), bytes).unwrap();
        assert_eq!(
            resolver
                .resolve(user, track)
                .await
                .unwrap()
                .unwrap()
                .mime_type,
            mime
        );
    }

    let directory = tempfile::tempdir().unwrap();
    let (resolver, user, track) = mapped_resolver(&directory, "song.wav", b"media").await;
    assert_eq!(resolver.resolve(user, track).await.unwrap(), None);
}

#[tokio::test]
async fn rejects_unsupported_images_and_enforces_the_exact_five_mib_limit() {
    let unsupported = tempfile::tempdir().unwrap();
    let (resolver, user, track) = mapped_resolver(&unsupported, "song.wav", b"media").await;
    std::fs::write(unsupported.path().join("cover.gif"), b"GIF89a-not-allowed").unwrap();
    assert_eq!(resolver.resolve(user, track).await.unwrap(), None);

    let exact = tempfile::tempdir().unwrap();
    let (resolver, user, track) = mapped_resolver(&exact, "song.wav", b"media").await;
    std::fs::write(exact.path().join("cover.jpg"), jpeg(MAX_ARTWORK_BYTES, 4)).unwrap();
    assert_eq!(
        resolver
            .resolve(user, track)
            .await
            .unwrap()
            .unwrap()
            .bytes
            .len(),
        MAX_ARTWORK_BYTES
    );

    let oversized = tempfile::tempdir().unwrap();
    let (resolver, user, track) = mapped_resolver(&oversized, "song.wav", b"media").await;
    std::fs::write(
        oversized.path().join("cover.jpg"),
        jpeg(MAX_ARTWORK_BYTES + 1, 5),
    )
    .unwrap();
    assert_eq!(resolver.resolve(user, track).await.unwrap(), None);
}
