import type { DatabaseSync } from "node:sqlite";

export function ensureMcpSchema(database: DatabaseSync): void {
  database.exec(`
    PRAGMA foreign_keys = ON;

    CREATE TABLE IF NOT EXISTS mcp_metadata (
      key TEXT PRIMARY KEY,
      value TEXT NOT NULL,
      updated_at TEXT NOT NULL
    );

    CREATE VIRTUAL TABLE IF NOT EXISTS workout_search USING fts5(
      workout_id UNINDEXED,
      workout_date UNINDEXED,
      workout_type,
      title,
      description,
      athlete_comments,
      coach_comments,
      tokenize = 'porter unicode61'
    );

    CREATE VIEW IF NOT EXISTS workout_metrics AS
    SELECT
      w.id,
      w.athlete_id,
      w.workout_date,
      w.title,
      w.workout_type,
      w.description,
      w.raw_json,
      w.source_file_id,
      CAST(NULLIF(MAX(CASE WHEN f.field_name = 'TimeTotalInHours' THEN f.value_text END), '') AS REAL) AS duration_hours,
      CAST(NULLIF(MAX(CASE WHEN f.field_name = 'PlannedDuration' THEN f.value_text END), '') AS REAL) AS planned_duration_hours,
      CAST(NULLIF(MAX(CASE WHEN f.field_name = 'DistanceInMeters' THEN f.value_text END), '') AS REAL) AS distance_meters,
      CAST(NULLIF(MAX(CASE WHEN f.field_name = 'PlannedDistanceInMeters' THEN f.value_text END), '') AS REAL) AS planned_distance_meters,
      CAST(NULLIF(MAX(CASE WHEN f.field_name = 'TSS' THEN f.value_text END), '') AS REAL) AS tss,
      CAST(NULLIF(MAX(CASE WHEN f.field_name = 'IF' THEN f.value_text END), '') AS REAL) AS intensity_factor,
      CAST(NULLIF(MAX(CASE WHEN f.field_name = 'HeartRateAverage' THEN f.value_text END), '') AS REAL) AS heart_rate_average,
      CAST(NULLIF(MAX(CASE WHEN f.field_name = 'HeartRateMax' THEN f.value_text END), '') AS REAL) AS heart_rate_max,
      CAST(NULLIF(MAX(CASE WHEN f.field_name = 'PowerAverage' THEN f.value_text END), '') AS REAL) AS power_average,
      CAST(NULLIF(MAX(CASE WHEN f.field_name = 'PowerMax' THEN f.value_text END), '') AS REAL) AS power_max,
      CAST(NULLIF(MAX(CASE WHEN f.field_name = 'CadenceAverage' THEN f.value_text END), '') AS REAL) AS cadence_average,
      CAST(NULLIF(MAX(CASE WHEN f.field_name = 'Energy' THEN f.value_text END), '') AS REAL) AS energy,
      CAST(NULLIF(MAX(CASE WHEN f.field_name = 'Rpe' THEN f.value_text END), '') AS REAL) AS rpe,
      CAST(NULLIF(MAX(CASE WHEN f.field_name = 'Feeling' THEN f.value_text END), '') AS REAL) AS feeling,
      CAST(NULLIF(MAX(CASE WHEN f.field_name = 'VelocityAverage' THEN f.value_text END), '') AS REAL) AS velocity_average,
      NULLIF(MAX(CASE WHEN f.field_name = 'AthleteComments' THEN f.value_text END), '') AS athlete_comments,
      NULLIF(MAX(CASE WHEN f.field_name = 'CoachComments' THEN f.value_text END), '') AS coach_comments
    FROM workouts AS w
    LEFT JOIN workout_fields AS f ON f.workout_id = w.id
    GROUP BY w.id;

    CREATE TABLE IF NOT EXISTS activity_files (
      id INTEGER PRIMARY KEY,
      file_id INTEGER NOT NULL UNIQUE REFERENCES files(id) ON DELETE CASCADE,
      workout_id INTEGER REFERENCES workouts(id) ON DELETE SET NULL,
      relative_path TEXT NOT NULL,
      file_format TEXT NOT NULL DEFAULT 'FIT',
      fit_valid INTEGER NOT NULL,
      crc_valid INTEGER NOT NULL,
      start_time TEXT,
      local_date TEXT,
      sport TEXT,
      sub_sport TEXT,
      total_elapsed_seconds REAL,
      total_timer_seconds REAL,
      total_distance_meters REAL,
      total_calories REAL,
      total_ascent_meters REAL,
      avg_heart_rate REAL,
      max_heart_rate REAL,
      avg_power REAL,
      max_power REAL,
      normalized_power REAL,
      avg_cadence REAL,
      max_cadence REAL,
      avg_speed_mps REAL,
      max_speed_mps REAL,
      record_count INTEGER NOT NULL DEFAULT 0,
      lap_count INTEGER NOT NULL DEFAULT 0,
      decode_errors TEXT,
      summary_json TEXT,
      indexed_at TEXT NOT NULL
    );

    CREATE INDEX IF NOT EXISTS activity_files_date_idx ON activity_files(local_date, sport);
    CREATE INDEX IF NOT EXISTS activity_files_workout_idx ON activity_files(workout_id);

    CREATE TABLE IF NOT EXISTS activity_laps (
      activity_file_id INTEGER NOT NULL REFERENCES activity_files(id) ON DELETE CASCADE,
      lap_index INTEGER NOT NULL,
      start_time TEXT,
      elapsed_seconds REAL,
      timer_seconds REAL,
      distance_meters REAL,
      avg_heart_rate REAL,
      max_heart_rate REAL,
      avg_power REAL,
      max_power REAL,
      avg_cadence REAL,
      max_cadence REAL,
      avg_speed_mps REAL,
      max_speed_mps REAL,
      ascent_meters REAL,
      calories REAL,
      intensity TEXT,
      raw_json TEXT NOT NULL,
      PRIMARY KEY (activity_file_id, lap_index)
    );

    CREATE TABLE IF NOT EXISTS training_plans (
      id INTEGER PRIMARY KEY,
      name TEXT NOT NULL,
      goal TEXT NOT NULL,
      start_date TEXT NOT NULL,
      end_date TEXT NOT NULL,
      status TEXT NOT NULL DEFAULT 'draft' CHECK (status IN ('draft', 'active', 'completed', 'archived')),
      notes TEXT,
      created_at TEXT NOT NULL,
      updated_at TEXT NOT NULL
    );

    CREATE TABLE IF NOT EXISTS planned_sessions (
      id INTEGER PRIMARY KEY,
      plan_id INTEGER NOT NULL REFERENCES training_plans(id) ON DELETE CASCADE,
      session_date TEXT NOT NULL,
      sport TEXT NOT NULL,
      title TEXT NOT NULL,
      duration_minutes REAL,
      target_tss REAL,
      intensity TEXT,
      description TEXT NOT NULL,
      sort_order INTEGER NOT NULL,
      UNIQUE (plan_id, sort_order)
    );

    CREATE INDEX IF NOT EXISTS planned_sessions_date_idx ON planned_sessions(plan_id, session_date);

    -- These tables are derived indexes. Clean up any stale children left by an
    -- interrupted re-index or by opening an older database with FK checks off.
    DELETE FROM activity_laps
    WHERE NOT EXISTS (
      SELECT 1 FROM activity_files WHERE activity_files.id = activity_laps.activity_file_id
    );
    DELETE FROM planned_sessions
    WHERE NOT EXISTS (
      SELECT 1 FROM training_plans WHERE training_plans.id = planned_sessions.plan_id
    );
  `);

  const activityColumns = database.prepare("PRAGMA table_info(activity_files)").all() as Array<{ name: string }>;
  if (!activityColumns.some((column) => column.name === "file_format")) {
    database.exec("ALTER TABLE activity_files ADD COLUMN file_format TEXT NOT NULL DEFAULT 'FIT'");
  }
}

export function refreshWorkoutSearch(database: DatabaseSync, force = false): boolean {
  const source = database.prepare(`
    SELECT COUNT(*) AS count, COALESCE(MAX(updated_at), '') AS newest FROM workouts
  `).get() as { count: number; newest: string };
  const signature = `${source.count}:${source.newest}`;
  const current = database.prepare(`
    SELECT value FROM mcp_metadata WHERE key = 'workout_search_signature'
  `).get() as { value: string } | undefined;
  if (!force && current?.value === signature) return false;

  database.exec("BEGIN IMMEDIATE");
  try {
    database.exec("DELETE FROM workout_search");
    database.exec(`
      INSERT INTO workout_search (
        workout_id, workout_date, workout_type, title, description, athlete_comments, coach_comments
      )
      SELECT
        id, workout_date, workout_type, title, description,
        COALESCE(athlete_comments, ''), COALESCE(coach_comments, '')
      FROM workout_metrics
    `);
    database.prepare(`
      INSERT INTO mcp_metadata (key, value, updated_at) VALUES ('workout_search_signature', ?, ?)
      ON CONFLICT (key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at
    `).run(signature, new Date().toISOString());
    database.exec("COMMIT");
    return true;
  } catch (error) {
    database.exec("ROLLBACK");
    throw error;
  }
}
