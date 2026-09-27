use fishmuse_domain::{ListenId, ListenSummary, MediaAssetId, UserId};
use fishmuse_library::{
    LibraryQueryPort, LocalImport, LocalLibraryImporter, ParsedTags, SearchQuery,
};
use fishmuse_storage::{
    Database, ListeningRepository, SqliteLibraryRepository, SqliteListeningRepository,
};
use time::OffsetDateTime;

#[tokio::test]
async fn query_port_delegates_search_item_and_recent_listens_without_crossing_user_scope() {
    let database = Database::open_in_memory().await.expect("database");
    let user = database.ensure_local_user().await.expect("local user");
    let importer = LocalLibraryImporter::new(database.pool().clone(), user);
    let imported = importer
        .import(LocalImport {
            media_asset_id: MediaAssetId::new(),
            normalized_path: b"song.flac".to_vec(),
            original_path: b"Song.flac".to_vec(),
            content_fingerprint: "fingerprint".to_owned(),
            tags: ParsedTags {
                title: Some("Ocean Song".to_owned()),
                artists: vec!["Artist".to_owned()],
                ..ParsedTags::default()
            },
        })
        .await
        .expect("import");
    SqliteListeningRepository::new(database.pool().clone(), user)
        .append(&ListenSummary {
            id: ListenId::new(),
            track_id: imported.track_id,
            started_at: OffsetDateTime::from_unix_timestamp(1_800_000_000).expect("time"),
            listened_ms: 1_000,
            completed: false,
        })
        .await
        .expect("listen");
    let repository = SqliteLibraryRepository::new(database.pool().clone(), user);

    let search = LibraryQueryPort::search(
        &repository,
        user,
        SearchQuery {
            text: "Ocean".to_owned(),
            ..SearchQuery::default()
        },
    )
    .await
    .expect("search");
    let item = LibraryQueryPort::get_item(&repository, user, imported.track_id)
        .await
        .expect("item")
        .expect("present");
    let listens = LibraryQueryPort::recent_listens(&repository, user, 0)
        .await
        .expect("listens");

    assert_eq!(search.len(), 1);
    assert_eq!(search[0].id, imported.track_id);
    assert_eq!(item.track.id, imported.track_id);
    assert_eq!(listens.len(), 1);
    assert_eq!(SearchQuery::default().limit, 20);
    assert!(
        LibraryQueryPort::search(&repository, UserId::new(), SearchQuery::default())
            .await
            .is_err()
    );
}
