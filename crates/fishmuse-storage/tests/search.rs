use fishmuse_domain::{ArtistId, RecordingId, ReleaseId, TrackId, UserId};
use fishmuse_storage::{Database, SqliteLibraryRepository};
use uuid::Uuid;

struct TrackFixture<'a> {
    id: &'a str,
    title: &'a str,
    artist: &'a str,
    release: &'a str,
    imported_at: i64,
}

fn track_id(value: &str) -> TrackId {
    TrackId::try_from_uuid(Uuid::parse_str(value).expect("fixture UUID")).expect("fixture UUID v7")
}

async fn insert_user(database: &Database) -> UserId {
    let user = UserId::new();
    sqlx::query("INSERT INTO users(user_id, local_slot, created_at) VALUES (?, NULL, ?)")
        .bind(user.as_uuid().to_string())
        .bind(1_800_000_000_i64)
        .execute(database.pool())
        .await
        .expect("user");
    user
}

async fn insert_track(database: &Database, user: UserId, fixture: &TrackFixture<'_>) -> TrackId {
    let recording = RecordingId::new();
    let release = ReleaseId::new();
    let artist = ArtistId::new();
    let track = track_id(fixture.id);
    sqlx::query("INSERT INTO recordings(recording_id, user_id, title, normalized_title) VALUES (?, ?, ?, ?)")
        .bind(recording.as_uuid().to_string())
        .bind(user.as_uuid().to_string())
        .bind(fixture.title)
        .bind(fixture.title.to_lowercase())
        .execute(database.pool())
        .await
        .expect("recording");
    sqlx::query(
        "INSERT INTO releases(release_id, user_id, title, normalized_title) VALUES (?, ?, ?, ?)",
    )
    .bind(release.as_uuid().to_string())
    .bind(user.as_uuid().to_string())
    .bind(fixture.release)
    .bind(fixture.release.to_lowercase())
    .execute(database.pool())
    .await
    .expect("release");
    sqlx::query(
        "INSERT INTO artists(artist_id, user_id, name, normalized_name) VALUES (?, ?, ?, ?)",
    )
    .bind(artist.as_uuid().to_string())
    .bind(user.as_uuid().to_string())
    .bind(fixture.artist)
    .bind(fixture.artist.to_lowercase())
    .execute(database.pool())
    .await
    .expect("artist");
    sqlx::query("INSERT INTO tracks(track_id, user_id, recording_id, release_id, title, normalized_title, playable, imported_at) VALUES (?, ?, ?, ?, ?, ?, 1, ?)")
        .bind(track.as_uuid().to_string())
        .bind(user.as_uuid().to_string())
        .bind(recording.as_uuid().to_string())
        .bind(release.as_uuid().to_string())
        .bind(fixture.title)
        .bind(fixture.title.to_lowercase())
        .bind(fixture.imported_at)
        .execute(database.pool())
        .await
        .expect("track");
    sqlx::query(
        "INSERT INTO track_artists(user_id, track_id, artist_id, position) VALUES (?, ?, ?, 0)",
    )
    .bind(user.as_uuid().to_string())
    .bind(track.as_uuid().to_string())
    .bind(artist.as_uuid().to_string())
    .execute(database.pool())
    .await
    .expect("track artist");
    track
}

#[tokio::test]
async fn searches_title_artist_release_and_combined_filters() {
    let database = Database::open_in_memory().await.expect("database");
    let user = database.ensure_local_user().await.expect("local user");
    let expected = insert_track(
        &database,
        user,
        &TrackFixture {
            id: "018f0000-0000-7000-8000-000000000001",
            title: "Northern Ocean",
            artist: "Alpha Ensemble",
            release: "Deep Sea",
            imported_at: 1,
        },
    )
    .await;
    insert_track(
        &database,
        user,
        &TrackFixture {
            id: "018f0000-0000-7000-8000-000000000002",
            title: "Mountain",
            artist: "Beta Ensemble",
            release: "High Land",
            imported_at: 2,
        },
    )
    .await;
    let repository = SqliteLibraryRepository::new(database.pool().clone(), user);

    for (text, artist, release) in [
        ("Northern", None, None),
        ("Alpha", None, None),
        ("Deep", None, None),
        ("Ocean", Some("alpha"), Some("deep")),
    ] {
        let results = repository
            .search_tracks(text, artist, release, 20, 0)
            .await
            .expect("search");
        assert_eq!(results.len(), 1, "query {text}");
        assert_eq!(results[0].id, expected, "query {text}");
    }
}

#[tokio::test]
async fn escapes_fts_syntax_and_never_reads_another_users_tracks() {
    let database = Database::open_in_memory().await.expect("database");
    let user = database.ensure_local_user().await.expect("local user");
    let other = insert_user(&database).await;
    insert_track(
        &database,
        other,
        &TrackFixture {
            id: "018f0000-0000-7000-8000-000000000010",
            title: "Private Ocean",
            artist: "Other",
            release: "Other",
            imported_at: 1,
        },
    )
    .await;
    let repository = SqliteLibraryRepository::new(database.pool().clone(), user);

    let results = repository
        .search_tracks("\" OR * - NEAR()", None, None, 20, 0)
        .await
        .expect("special characters are data, not FTS syntax");

    assert!(results.is_empty());
    assert!(
        repository
            .search_tracks("Ocean", None, None, 20, 0)
            .await
            .expect("isolated search")
            .is_empty()
    );
}

#[tokio::test]
async fn empty_query_uses_recent_import_order_and_limits_are_defaulted_and_capped() {
    let database = Database::open_in_memory().await.expect("database");
    let user = database.ensure_local_user().await.expect("local user");
    for index in 0..105_u32 {
        insert_track(
            &database,
            user,
            &TrackFixture {
                id: &format!("018f0000-0000-7000-8000-{index:012x}"),
                title: &format!("Synthetic {index:03}"),
                artist: "Benchmark Artist",
                release: "Benchmark Release",
                imported_at: i64::from(index),
            },
        )
        .await;
    }
    let repository = SqliteLibraryRepository::new(database.pool().clone(), user);

    let defaulted = repository
        .search_tracks("", None, None, 0, 0)
        .await
        .expect("default limit");
    let capped = repository
        .search_tracks("", None, None, u32::MAX, 0)
        .await
        .expect("capped limit");
    let second_page = repository
        .search_tracks("", None, None, 100, 100)
        .await
        .expect("second page");

    assert_eq!(defaulted.len(), 20);
    assert_eq!(defaulted[0].title, "Synthetic 104");
    assert_eq!(defaulted[19].title, "Synthetic 085");
    assert_eq!(capped.len(), 100);
    assert_eq!(second_page.len(), 5);
    assert_eq!(second_page[0].title, "Synthetic 004");
    assert_eq!(second_page[4].title, "Synthetic 000");
}

#[tokio::test]
async fn ranking_is_exact_then_prefix_then_bm25_with_stable_artist_title_and_id_ties() {
    let database = Database::open_in_memory().await.expect("database");
    let user = database.ensure_local_user().await.expect("local user");
    let fixtures = [
        TrackFixture {
            id: "018f0000-0000-7000-8000-000000000021",
            title: "Ocean",
            artist: "Zed",
            release: "Sea",
            imported_at: 1,
        },
        TrackFixture {
            id: "018f0000-0000-7000-8000-000000000022",
            title: "Ocean",
            artist: "Able",
            release: "Sea",
            imported_at: 2,
        },
        TrackFixture {
            id: "018f0000-0000-7000-8000-000000000023",
            title: "Ocean Drive",
            artist: "Able",
            release: "Sea",
            imported_at: 3,
        },
        TrackFixture {
            id: "018f0000-0000-7000-8000-000000000024",
            title: "Blue",
            artist: "Ocean",
            release: "Sea",
            imported_at: 4,
        },
    ];
    for fixture in &fixtures {
        insert_track(&database, user, fixture).await;
    }
    let repository = SqliteLibraryRepository::new(database.pool().clone(), user);

    let results = repository
        .search_tracks("Ocean", None, None, 20, 0)
        .await
        .expect("ranked search");
    let ordered: Vec<_> = results.iter().map(|track| track.id).collect();

    assert_eq!(
        ordered,
        [
            track_id("018f0000-0000-7000-8000-000000000022"),
            track_id("018f0000-0000-7000-8000-000000000021"),
            track_id("018f0000-0000-7000-8000-000000000023"),
            track_id("018f0000-0000-7000-8000-000000000024"),
        ]
    );
}
