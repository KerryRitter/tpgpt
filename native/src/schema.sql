
      CREATE TABLE IF NOT EXISTS export_jobs (
        id INTEGER PRIMARY KEY,
        athlete_id TEXT NOT NULL,
        export_kind TEXT NOT NULL CHECK (export_kind IN ('workouts', 'files')),
        window_start TEXT NOT NULL,
        window_end TEXT NOT NULL,
        request_url TEXT NOT NULL,
        status TEXT NOT NULL CHECK (status IN ('started', 'complete', 'failed')),
        archive_sha256 TEXT,
        started_at TEXT NOT NULL,
        completed_at TEXT,
        error TEXT,
        file_count INTEGER NOT NULL DEFAULT 0,
        workout_count INTEGER NOT NULL DEFAULT 0,
        UNIQUE (athlete_id, export_kind, window_start, window_end)
      );

      CREATE TABLE IF NOT EXISTS file_blobs (
        id INTEGER PRIMARY KEY,
        sha256 TEXT NOT NULL UNIQUE,
        byte_size INTEGER NOT NULL,
        content BLOB NOT NULL
      );

      CREATE TABLE IF NOT EXISTS files (
        id INTEGER PRIMARY KEY,
        athlete_id TEXT NOT NULL,
        relative_path TEXT NOT NULL,
        file_name TEXT NOT NULL,
        extension TEXT NOT NULL,
        mime_type TEXT NOT NULL,
        blob_id INTEGER NOT NULL REFERENCES file_blobs(id),
        first_seen_at TEXT NOT NULL,
        last_seen_at TEXT NOT NULL,
        UNIQUE (athlete_id, relative_path, blob_id)
      );

      CREATE TABLE IF NOT EXISTS export_files (
        export_id INTEGER NOT NULL REFERENCES export_jobs(id) ON DELETE CASCADE,
        file_id INTEGER NOT NULL REFERENCES files(id),
        PRIMARY KEY (export_id, file_id)
      );

      CREATE TABLE IF NOT EXISTS workouts (
        id INTEGER PRIMARY KEY,
        athlete_id TEXT NOT NULL,
        stable_key TEXT NOT NULL,
        external_workout_id TEXT,
        workout_date TEXT,
        title TEXT,
        workout_type TEXT,
        description TEXT,
        raw_json TEXT NOT NULL,
        source_file_id INTEGER REFERENCES files(id),
        source_row_number INTEGER,
        first_seen_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        UNIQUE (athlete_id, stable_key)
      );

      CREATE TABLE IF NOT EXISTS workout_fields (
        workout_id INTEGER NOT NULL REFERENCES workouts(id) ON DELETE CASCADE,
        field_name TEXT NOT NULL,
        value_text TEXT,
        PRIMARY KEY (workout_id, field_name)
      );

      CREATE TABLE IF NOT EXISTS workout_exports (
        workout_id INTEGER NOT NULL REFERENCES workouts(id) ON DELETE CASCADE,
        export_id INTEGER NOT NULL REFERENCES export_jobs(id) ON DELETE CASCADE,
        PRIMARY KEY (workout_id, export_id)
      );

      CREATE INDEX IF NOT EXISTS workouts_date_idx ON workouts(athlete_id, workout_date);
      CREATE INDEX IF NOT EXISTS workouts_type_idx ON workouts(athlete_id, workout_type);
      CREATE INDEX IF NOT EXISTS files_name_idx ON files(athlete_id, file_name);
