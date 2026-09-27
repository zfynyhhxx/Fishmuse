CREATE TABLE users (
    user_id TEXT PRIMARY KEY NOT NULL,
    local_slot INTEGER UNIQUE CHECK (local_slot IS NULL OR local_slot = 1),
    created_at INTEGER NOT NULL
);

CREATE TABLE artists (
    artist_id TEXT PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL REFERENCES users(user_id),
    name TEXT NOT NULL,
    normalized_name TEXT NOT NULL DEFAULT '',
    sort_name TEXT,
    UNIQUE (artist_id, user_id)
);

CREATE TABLE releases (
    release_id TEXT PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL REFERENCES users(user_id),
    title TEXT NOT NULL,
    normalized_title TEXT NOT NULL DEFAULT '',
    release_year INTEGER,
    UNIQUE (release_id, user_id)
);

CREATE TABLE recordings (
    recording_id TEXT PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL REFERENCES users(user_id),
    title TEXT NOT NULL,
    normalized_title TEXT NOT NULL DEFAULT '',
    duration_ms INTEGER CHECK (duration_ms IS NULL OR duration_ms >= 0),
    provenance TEXT NOT NULL DEFAULT 'local_tags',
    UNIQUE (recording_id, user_id)
);

CREATE TABLE tracks (
    track_id TEXT PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL REFERENCES users(user_id),
    recording_id TEXT NOT NULL,
    release_id TEXT,
    title TEXT NOT NULL,
    normalized_title TEXT NOT NULL DEFAULT '',
    disc_number INTEGER CHECK (disc_number IS NULL OR disc_number > 0),
    track_number INTEGER CHECK (track_number IS NULL OR track_number > 0),
    duration_ms INTEGER CHECK (duration_ms IS NULL OR duration_ms >= 0),
    playable INTEGER NOT NULL DEFAULT 0 CHECK (playable IN (0, 1)),
    imported_at INTEGER NOT NULL DEFAULT 0,
    UNIQUE (track_id, user_id),
    FOREIGN KEY (recording_id, user_id) REFERENCES recordings(recording_id, user_id),
    FOREIGN KEY (release_id, user_id) REFERENCES releases(release_id, user_id)
);

CREATE TABLE track_artists (
    user_id TEXT NOT NULL REFERENCES users(user_id),
    track_id TEXT NOT NULL,
    artist_id TEXT NOT NULL,
    role TEXT NOT NULL DEFAULT 'primary',
    position INTEGER NOT NULL DEFAULT 0 CHECK (position >= 0),
    PRIMARY KEY (user_id, track_id, artist_id, role),
    FOREIGN KEY (track_id, user_id) REFERENCES tracks(track_id, user_id),
    FOREIGN KEY (artist_id, user_id) REFERENCES artists(artist_id, user_id)
);

CREATE TABLE media_assets (
    media_asset_id TEXT PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL REFERENCES users(user_id),
    track_id TEXT,
    normalized_path BLOB NOT NULL,
    original_path BLOB NOT NULL,
    content_fingerprint TEXT,
    availability TEXT NOT NULL,
    subsong_index INTEGER CHECK (subsong_index IS NULL OR subsong_index >= 0),
    start_ms INTEGER CHECK (start_ms IS NULL OR start_ms >= 0),
    end_ms INTEGER CHECK (end_ms IS NULL OR end_ms >= 0),
    UNIQUE (media_asset_id, user_id),
    UNIQUE (user_id, normalized_path),
    FOREIGN KEY (track_id, user_id) REFERENCES tracks(track_id, user_id)
);

CREATE INDEX media_assets_fingerprint_idx
    ON media_assets(user_id, content_fingerprint)
    WHERE content_fingerprint IS NOT NULL;

CREATE TABLE local_import_metadata (
    user_id TEXT NOT NULL REFERENCES users(user_id),
    media_asset_id TEXT NOT NULL,
    raw_tags_json TEXT NOT NULL,
    normalized_title TEXT NOT NULL,
    normalized_artists_json TEXT NOT NULL,
    normalized_release TEXT,
    provenance TEXT NOT NULL,
    PRIMARY KEY (user_id, media_asset_id),
    FOREIGN KEY (media_asset_id, user_id)
        REFERENCES media_assets(media_asset_id, user_id) ON DELETE CASCADE
);

CREATE TABLE recording_possible_matches (
    user_id TEXT NOT NULL REFERENCES users(user_id),
    recording_id TEXT NOT NULL,
    candidate_recording_id TEXT NOT NULL,
    reason TEXT NOT NULL,
    PRIMARY KEY (user_id, recording_id, candidate_recording_id),
    FOREIGN KEY (recording_id, user_id)
        REFERENCES recordings(recording_id, user_id) ON DELETE CASCADE,
    FOREIGN KEY (candidate_recording_id, user_id)
        REFERENCES recordings(recording_id, user_id) ON DELETE CASCADE,
    CHECK (recording_id <> candidate_recording_id)
);

CREATE TABLE media_roots (
    media_root_id TEXT PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL REFERENCES users(user_id),
    normalized_path BLOB NOT NULL,
    original_path BLOB NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    UNIQUE (media_root_id, user_id),
    UNIQUE (user_id, normalized_path)
);

CREATE TABLE scan_runs (
    scan_run_id TEXT PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL REFERENCES users(user_id),
    media_root_id TEXT,
    status TEXT NOT NULL,
    started_at INTEGER NOT NULL,
    completed_at INTEGER,
    UNIQUE (scan_run_id, user_id),
    FOREIGN KEY (media_root_id, user_id) REFERENCES media_roots(media_root_id, user_id)
);

CREATE TABLE scan_diagnostics (
    diagnostic_id TEXT PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL REFERENCES users(user_id),
    scan_run_id TEXT NOT NULL,
    path BLOB,
    code TEXT NOT NULL,
    message TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (scan_run_id, user_id) REFERENCES scan_runs(scan_run_id, user_id)
);

CREATE TABLE listening_events (
    listen_id TEXT PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL REFERENCES users(user_id),
    track_id TEXT NOT NULL,
    started_at INTEGER NOT NULL,
    listened_ms INTEGER NOT NULL CHECK (listened_ms >= 0),
    completed INTEGER NOT NULL CHECK (completed IN (0, 1)),
    FOREIGN KEY (track_id, user_id) REFERENCES tracks(track_id, user_id)
);

CREATE INDEX listening_events_recent_idx ON listening_events(user_id, started_at DESC);

CREATE TABLE conversations (
    conversation_id TEXT PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL REFERENCES users(user_id),
    title TEXT,
    created_at INTEGER NOT NULL,
    UNIQUE (conversation_id, user_id)
);

CREATE TABLE conversation_messages (
    message_id TEXT PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL REFERENCES users(user_id),
    conversation_id TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('user', 'assistant', 'tool', 'system')),
    content TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    FOREIGN KEY (conversation_id, user_id)
        REFERENCES conversations(conversation_id, user_id) ON DELETE CASCADE
);

CREATE INDEX conversation_messages_order_idx
    ON conversation_messages(user_id, conversation_id, created_at, message_id);

CREATE TABLE app_settings (
    user_id TEXT NOT NULL REFERENCES users(user_id),
    key TEXT NOT NULL,
    value_json TEXT NOT NULL,
    PRIMARY KEY (user_id, key)
);

CREATE TABLE ai_usage_ledger (
    usage_id TEXT PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL REFERENCES users(user_id),
    conversation_id TEXT,
    model TEXT NOT NULL,
    input_tokens INTEGER NOT NULL CHECK (input_tokens >= 0),
    output_tokens INTEGER NOT NULL CHECK (output_tokens >= 0),
    cost_microunits INTEGER NOT NULL CHECK (cost_microunits >= 0),
    created_at INTEGER NOT NULL,
    FOREIGN KEY (conversation_id, user_id) REFERENCES conversations(conversation_id, user_id)
);

CREATE TABLE applied_operations (
    operation_id TEXT PRIMARY KEY NOT NULL,
    user_id TEXT NOT NULL REFERENCES users(user_id),
    operation_kind TEXT NOT NULL,
    result_json TEXT NOT NULL,
    completed_at INTEGER NOT NULL
);

CREATE VIRTUAL TABLE library_fts USING fts5(
    user_id UNINDEXED,
    track_id UNINDEXED,
    title,
    normalized_title UNINDEXED,
    artist,
    release_title,
    tokenize = 'unicode61'
);

CREATE TRIGGER tracks_library_fts_insert AFTER INSERT ON tracks BEGIN
    INSERT INTO library_fts(user_id, track_id, title, normalized_title, artist, release_title)
    SELECT new.user_id, new.track_id, new.title, COALESCE(NULLIF(new.normalized_title, ''), lower(trim(new.title))), '', COALESCE(releases.title, '')
    FROM (SELECT 1)
    LEFT JOIN releases ON releases.user_id = new.user_id AND releases.release_id = new.release_id;
END;

CREATE TRIGGER tracks_library_fts_update AFTER UPDATE OF user_id, title, normalized_title, release_id ON tracks BEGIN
    DELETE FROM library_fts WHERE user_id = old.user_id AND track_id = old.track_id;
    INSERT INTO library_fts(user_id, track_id, title, normalized_title, artist, release_title)
    SELECT new.user_id, new.track_id, new.title, COALESCE(NULLIF(new.normalized_title, ''), lower(trim(new.title))),
           COALESCE((SELECT group_concat(artists.name, ' ') FROM track_artists JOIN artists ON artists.user_id = track_artists.user_id AND artists.artist_id = track_artists.artist_id WHERE track_artists.user_id = new.user_id AND track_artists.track_id = new.track_id ORDER BY track_artists.position), ''),
           COALESCE(releases.title, '')
    FROM (SELECT 1)
    LEFT JOIN releases ON releases.user_id = new.user_id AND releases.release_id = new.release_id;
END;

CREATE TRIGGER tracks_library_fts_delete AFTER DELETE ON tracks BEGIN
    DELETE FROM library_fts WHERE user_id = old.user_id AND track_id = old.track_id;
END;

CREATE TRIGGER track_artists_library_fts_insert AFTER INSERT ON track_artists BEGIN
    UPDATE library_fts
    SET artist = COALESCE((SELECT group_concat(artists.name, ' ') FROM track_artists JOIN artists ON artists.user_id = track_artists.user_id AND artists.artist_id = track_artists.artist_id WHERE track_artists.user_id = new.user_id AND track_artists.track_id = new.track_id ORDER BY track_artists.position), '')
    WHERE user_id = new.user_id AND track_id = new.track_id;
END;

CREATE TRIGGER track_artists_library_fts_delete AFTER DELETE ON track_artists BEGIN
    UPDATE library_fts
    SET artist = COALESCE((SELECT group_concat(artists.name, ' ') FROM track_artists JOIN artists ON artists.user_id = track_artists.user_id AND artists.artist_id = track_artists.artist_id WHERE track_artists.user_id = old.user_id AND track_artists.track_id = old.track_id ORDER BY track_artists.position), '')
    WHERE user_id = old.user_id AND track_id = old.track_id;
END;

CREATE TRIGGER track_artists_library_fts_update AFTER UPDATE OF user_id, track_id, artist_id, position ON track_artists BEGIN
    UPDATE library_fts
    SET artist = COALESCE((SELECT group_concat(artists.name, ' ') FROM track_artists JOIN artists ON artists.user_id = track_artists.user_id AND artists.artist_id = track_artists.artist_id WHERE track_artists.user_id = old.user_id AND track_artists.track_id = old.track_id ORDER BY track_artists.position), '')
    WHERE user_id = old.user_id AND track_id = old.track_id;
    UPDATE library_fts
    SET artist = COALESCE((SELECT group_concat(artists.name, ' ') FROM track_artists JOIN artists ON artists.user_id = track_artists.user_id AND artists.artist_id = track_artists.artist_id WHERE track_artists.user_id = new.user_id AND track_artists.track_id = new.track_id ORDER BY track_artists.position), '')
    WHERE user_id = new.user_id AND track_id = new.track_id;
END;

CREATE TRIGGER artists_library_fts_update AFTER UPDATE OF name ON artists BEGIN
    UPDATE library_fts
    SET artist = COALESCE((SELECT group_concat(all_artists.name, ' ') FROM track_artists JOIN artists AS all_artists ON all_artists.user_id = track_artists.user_id AND all_artists.artist_id = track_artists.artist_id WHERE track_artists.user_id = new.user_id AND track_artists.track_id = library_fts.track_id ORDER BY track_artists.position), '')
    WHERE user_id = new.user_id
      AND track_id IN (SELECT track_id FROM track_artists WHERE user_id = new.user_id AND artist_id = new.artist_id);
END;

CREATE TRIGGER releases_library_fts_update AFTER UPDATE OF title ON releases BEGIN
    UPDATE library_fts SET release_title = new.title
    WHERE user_id = new.user_id
      AND track_id IN (SELECT track_id FROM tracks WHERE user_id = new.user_id AND release_id = new.release_id);
END;

CREATE INDEX releases_user_title_idx ON releases(user_id, title);
CREATE INDEX tracks_user_title_idx ON tracks(user_id, title);
