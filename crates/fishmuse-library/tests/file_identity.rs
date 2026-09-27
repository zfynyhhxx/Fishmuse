use std::{fs, path::PathBuf};

use filetime::{FileTime, set_file_mtime};
use fishmuse_library::{FileIdentity, discover_files, normalize_path_bytes};
use tempfile::tempdir;

#[cfg(windows)]
#[test]
fn normalizes_windows_case_long_path_prefixes_and_unc_forms() {
    assert_eq!(
        normalize_path_bytes(PathBuf::from(r"C:\Music\Album\Track.FLAC").as_path()),
        normalize_path_bytes(PathBuf::from(r"\\?\c:\music\album\track.flac").as_path())
    );
    assert_eq!(
        normalize_path_bytes(PathBuf::from(r"\\Server\Share\Music\Track.FLAC").as_path()),
        normalize_path_bytes(PathBuf::from(r"\\?\UNC\server\share\music\track.flac").as_path())
    );
    assert_eq!(
        normalize_path_bytes(PathBuf::from(r"C:\MÜSIC\Track.FLAC").as_path()),
        normalize_path_bytes(PathBuf::from(r"c:\müsic\track.flac").as_path())
    );
}

#[test]
fn quick_fingerprint_distinguishes_same_size_and_mtime_with_different_content() {
    let directory = tempdir().expect("temporary directory");
    let first_path = directory.path().join("first.flac");
    let second_path = directory.path().join("second.flac");
    fs::write(&first_path, b"first-content").expect("first fixture");
    fs::write(&second_path, b"other-content").expect("second fixture");
    let fixed_time = FileTime::from_unix_time(1_800_000_000, 123_000_000);
    set_file_mtime(&first_path, fixed_time).expect("first mtime");
    set_file_mtime(&second_path, fixed_time).expect("second mtime");

    let first = FileIdentity::read(&first_path).expect("first identity");
    let second = FileIdentity::read(&second_path).expect("second identity");

    assert_eq!(first.size, second.size);
    assert_eq!(first.modified_ns, second.modified_ns);
    assert_ne!(first.quick_fingerprint, second.quick_fingerprint);
}

#[test]
fn full_fingerprint_confirms_identical_file_after_move() {
    let directory = tempdir().expect("temporary directory");
    let original = directory.path().join("before.mp3");
    let moved = directory.path().join("nested").join("after.mp3");
    fs::write(&original, b"self-generated identity fixture").expect("fixture");
    let before = FileIdentity::read(&original).expect("identity before move");
    fs::create_dir(directory.path().join("nested")).expect("nested directory");
    fs::rename(&original, &moved).expect("move fixture");
    let after = FileIdentity::read(&moved).expect("identity after move");

    assert_eq!(before.content_fingerprint, after.content_fingerprint);
}

#[cfg(windows)]
#[test]
fn native_non_utf8_path_representation_is_normalized_without_panicking() {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt};

    let path = PathBuf::from(OsString::from_wide(&[
        b'C' as u16,
        b':' as u16,
        b'\\' as u16,
        0xD800,
    ]));
    let normalized = normalize_path_bytes(&path);

    assert!(!normalized.is_empty());
}

#[cfg(unix)]
#[test]
fn scans_a_filename_that_is_not_valid_utf8() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};

    let directory = tempdir().expect("temporary directory");
    let path = directory
        .path()
        .join(OsString::from_vec(b"track-\xFF.flac".to_vec()));
    fs::write(&path, b"fixture").expect("fixture");

    let discovered = discover_files(&[directory.path().to_path_buf()]).expect("discovery");

    assert_eq!(discovered, vec![path]);
}

#[test]
fn discovery_does_not_follow_directory_symlink_loops() {
    let directory = tempdir().expect("temporary directory");
    let album = directory.path().join("album");
    fs::create_dir(&album).expect("album directory");
    let track = album.join("track.flac");
    fs::write(&track, b"fixture").expect("fixture");
    let loop_path = album.join("loop");

    #[cfg(windows)]
    if std::os::windows::fs::symlink_dir(directory.path(), &loop_path).is_err() {
        // Windows installations without Developer Mode cannot create a test symlink.
        return;
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(directory.path(), &loop_path).expect("loop symlink");

    let discovered = discover_files(&[directory.path().to_path_buf()]).expect("discovery");

    assert_eq!(discovered, vec![track]);
}
