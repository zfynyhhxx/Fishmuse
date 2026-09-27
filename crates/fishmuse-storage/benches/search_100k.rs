use std::{fs, time::Instant};

use fishmuse_storage::{Database, SqliteLibraryRepository};

const TRACK_COUNT: usize = 100_000;

struct QueryCase {
    text: &'static str,
    artist: Option<&'static str>,
    release: Option<&'static str>,
}

#[tokio::main]
async fn main() {
    let database = Database::open_in_memory()
        .await
        .expect("benchmark database");
    let user = database.ensure_local_user().await.expect("local user");
    let user_text = user.as_uuid().to_string();
    seed(&database, &user_text).await;
    let repository = SqliteLibraryRepository::new(database.pool().clone(), user);
    let queries = representative_queries();

    for query in &queries {
        repository
            .search_tracks(query.text, query.artist, query.release, 20)
            .await
            .expect("warm search");
    }

    let mut elapsed_ms = Vec::with_capacity(queries.len());
    for query in &queries {
        let started = Instant::now();
        repository
            .search_tracks(query.text, query.artist, query.release, 20)
            .await
            .expect("measured search");
        elapsed_ms.push(started.elapsed().as_secs_f64() * 1_000.0);
    }
    elapsed_ms.sort_by(f64::total_cmp);
    let p95 = elapsed_ms[28];
    let report = format!(
        "FishMuse FTS5 search baseline\nsynthetic_tracks={TRACK_COUNT}\ncache=warm\nqueries={}\np95_ms={p95:.3}\ntarget_ms=200\n",
        queries.len()
    );
    let report_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/search_100k-report.txt");
    fs::write(&report_path, &report).expect("write target-only report");
    print!("{report}");
    assert!(p95 < 200.0, "P95 {p95:.3} ms exceeded 200 ms target");
}

async fn seed(database: &Database, user: &str) {
    let mut transaction = database.pool().begin().await.expect("seed transaction");
    for trigger in [
        "tracks_library_fts_insert",
        "tracks_library_fts_update",
        "tracks_library_fts_delete",
        "track_artists_library_fts_insert",
        "track_artists_library_fts_delete",
        "artists_library_fts_update",
        "releases_library_fts_update",
    ] {
        sqlx::query(&format!("DROP TRIGGER {trigger}"))
            .execute(&mut *transaction)
            .await
            .expect("disable per-row FTS projection while seeding synthetic data");
    }
    sqlx::query(
        "WITH RECURSIVE n(value) AS (VALUES(0) UNION ALL SELECT value + 1 FROM n WHERE value < 99) INSERT INTO releases(release_id, user_id, title, normalized_title) SELECT printf('018f0000-0000-7001-8000-%012x', value), ?, printf('Synthetic Release %03d', value), printf('synthetic release %03d', value) FROM n",
    )
    .bind(user)
    .execute(&mut *transaction)
    .await
    .expect("releases");
    sqlx::query(
        "WITH RECURSIVE n(value) AS (VALUES(0) UNION ALL SELECT value + 1 FROM n WHERE value < 99) INSERT INTO artists(artist_id, user_id, name, normalized_name) SELECT printf('018f0000-0000-7002-8000-%012x', value), ?, printf('Synthetic Artist %03d', value), printf('synthetic artist %03d', value) FROM n",
    )
    .bind(user)
    .execute(&mut *transaction)
    .await
    .expect("artists");
    sqlx::query(
        "WITH RECURSIVE n(value) AS (VALUES(0) UNION ALL SELECT value + 1 FROM n WHERE value < 99999) INSERT INTO recordings(recording_id, user_id, title, normalized_title, duration_ms, provenance) SELECT printf('018f0000-0000-7003-8000-%012x', value), ?, printf('Synthetic Track %06d', value), printf('synthetic track %06d', value), 180000 + (value % 1000), 'synthetic_benchmark' FROM n",
    )
    .bind(user)
    .execute(&mut *transaction)
    .await
    .expect("recordings");
    sqlx::query(
        "WITH RECURSIVE n(value) AS (VALUES(0) UNION ALL SELECT value + 1 FROM n WHERE value < 99999) INSERT INTO tracks(track_id, user_id, recording_id, release_id, title, normalized_title, duration_ms, playable, imported_at) SELECT printf('018f0000-0000-7004-8000-%012x', value), ?, printf('018f0000-0000-7003-8000-%012x', value), printf('018f0000-0000-7001-8000-%012x', value % 100), printf('Synthetic Track %06d', value), printf('synthetic track %06d', value), 180000 + (value % 1000), 1, value FROM n",
    )
    .bind(user)
    .execute(&mut *transaction)
    .await
    .expect("tracks");
    sqlx::query(
        "WITH RECURSIVE n(value) AS (VALUES(0) UNION ALL SELECT value + 1 FROM n WHERE value < 99999) INSERT INTO track_artists(user_id, track_id, artist_id, position) SELECT ?, printf('018f0000-0000-7004-8000-%012x', value), printf('018f0000-0000-7002-8000-%012x', value % 100), 0 FROM n",
    )
    .bind(user)
    .execute(&mut *transaction)
    .await
    .expect("track artists");
    sqlx::query(
        "INSERT INTO library_fts(user_id, track_id, title, artist, release_title) SELECT tracks.user_id, tracks.track_id, tracks.title, artists.name, releases.title FROM tracks JOIN track_artists ON track_artists.user_id = tracks.user_id AND track_artists.track_id = tracks.track_id JOIN artists ON artists.user_id = track_artists.user_id AND artists.artist_id = track_artists.artist_id JOIN releases ON releases.user_id = tracks.user_id AND releases.release_id = tracks.release_id WHERE tracks.user_id = ?",
    )
    .bind(user)
    .execute(&mut *transaction)
    .await
    .expect("bulk FTS projection");
    transaction.commit().await.expect("commit seed");
}

fn representative_queries() -> [QueryCase; 30] {
    [
        QueryCase {
            text: "Synthetic Track 000000",
            artist: None,
            release: None,
        },
        QueryCase {
            text: "Synthetic Track 099999",
            artist: None,
            release: None,
        },
        QueryCase {
            text: "Synthetic Track 050",
            artist: None,
            release: None,
        },
        QueryCase {
            text: "Track 001",
            artist: None,
            release: None,
        },
        QueryCase {
            text: "Track 010",
            artist: None,
            release: None,
        },
        QueryCase {
            text: "Track 020",
            artist: None,
            release: None,
        },
        QueryCase {
            text: "Track 030",
            artist: None,
            release: None,
        },
        QueryCase {
            text: "Track 040",
            artist: None,
            release: None,
        },
        QueryCase {
            text: "Track 050",
            artist: None,
            release: None,
        },
        QueryCase {
            text: "Track 060",
            artist: None,
            release: None,
        },
        QueryCase {
            text: "Track 070",
            artist: None,
            release: None,
        },
        QueryCase {
            text: "Track 080",
            artist: None,
            release: None,
        },
        QueryCase {
            text: "Track 090",
            artist: None,
            release: None,
        },
        QueryCase {
            text: "Artist 000",
            artist: None,
            release: None,
        },
        QueryCase {
            text: "Artist 025",
            artist: None,
            release: None,
        },
        QueryCase {
            text: "Artist 050",
            artist: None,
            release: None,
        },
        QueryCase {
            text: "Artist 075",
            artist: None,
            release: None,
        },
        QueryCase {
            text: "Release 000",
            artist: None,
            release: None,
        },
        QueryCase {
            text: "Release 025",
            artist: None,
            release: None,
        },
        QueryCase {
            text: "Release 050",
            artist: None,
            release: None,
        },
        QueryCase {
            text: "Release 075",
            artist: None,
            release: None,
        },
        QueryCase {
            text: "Track 007",
            artist: Some("artist 007"),
            release: None,
        },
        QueryCase {
            text: "Track 042",
            artist: Some("artist 042"),
            release: Some("release 042"),
        },
        QueryCase {
            text: "Track 042",
            artist: Some("artist 042"),
            release: Some("release 042"),
        },
        QueryCase {
            text: "Track 099",
            artist: Some("artist 099"),
            release: Some("release 099"),
        },
        QueryCase {
            text: "\" OR * - NEAR()",
            artist: None,
            release: None,
        },
        QueryCase {
            text: "",
            artist: None,
            release: None,
        },
        QueryCase {
            text: "",
            artist: Some("artist 011"),
            release: None,
        },
        QueryCase {
            text: "",
            artist: None,
            release: Some("release 022"),
        },
        QueryCase {
            text: "Track 099",
            artist: Some("artist 099"),
            release: Some("release 099"),
        },
    ]
}
