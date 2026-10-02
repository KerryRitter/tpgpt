import { mkdirSync, readFileSync } from "node:fs";
import { basename, extname, resolve } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { createHash } from "node:crypto";
import type { DateWindow, ExportKind, WorkoutRow } from "./types.js";

interface ExportIdentity {
  athleteId: string;
  kind: ExportKind;
  window: DateWindow;
}

interface StoredFile {
  id: number;
  sha256: string;
  size: number;
}

export interface ImportCounts {
  files: number;
  workouts: number;
}

function now(): string {
  return new Date().toISOString();
}

function asNumber(value: number | bigint): number {
  const converted = Number(value);
  if (!Number.isSafeInteger(converted)) throw new Error(`SQLite ID is outside the safe integer range: ${value}`);
  return converted;
}

function normalizedKey(key: string): string {
  return key.toLowerCase().replace(/[^a-z0-9]/g, "");
}

function stringValue(value: unknown): string | null {
  if (value === undefined || value === null) return null;
  if (typeof value === "string") return value.trim() || null;
  if (typeof value === "number" || typeof value === "boolean" || typeof value === "bigint") return String(value);
  return JSON.stringify(value);
}

function findValue(row: WorkoutRow, aliases: string[]): string | null {
  const wanted = new Set(aliases.map(normalizedKey));
  for (const [key, value] of Object.entries(row)) {
    if (wanted.has(normalizedKey(key))) return stringValue(value);
  }
  return null;
}

function canonicalJson(value: unknown): string {
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(",")}]`;
  if (value !== null && typeof value === "object") {
    return `{${Object.entries(value as Record<string, unknown>)
      .sort(([left], [right]) => left.localeCompare(right))
      .map(([key, child]) => `${JSON.stringify(key)}:${canonicalJson(child)}`)
      .join(",")}}`;
  }
  return JSON.stringify(value) ?? "null";
}

function workoutDetails(row: WorkoutRow): {
  stableKey: string;
  externalId: string | null;
  workoutDate: string | null;
  title: string | null;
  workoutType: string | null;
  description: string | null;
  rawJson: string;
} {
  const externalId = findValue(row, ["WorkoutId", "WorkoutPk", "WorkoutKey", "WorkoutGuid", "Id"]);
  const rawJson = canonicalJson(row);
  return {
    stableKey: externalId
      ? `id:${externalId}`
      : `sha256:${createHash("sha256").update(rawJson).digest("hex")}`,
    externalId,
    workoutDate: findValue(row, ["WorkoutDay", "WorkoutDate", "StartDate", "StartTime", "Date"]),
    title: findValue(row, ["WorkoutTitle", "Title", "Name"]),
    workoutType: findValue(row, ["WorkoutType", "Sport", "Type"]),
    description: findValue(row, ["WorkoutDescription", "Description", "Notes"]),
    rawJson,
  };
}

function mimeType(filePath: string): string {
  switch (extname(filePath).toLowerCase()) {
    case ".csv": return "text/csv";
    case ".json": return "application/json";
    case ".fit": return "application/vnd.ant.fit";
    case ".gpx": return "application/gpx+xml";
    case ".tcx": return "application/vnd.garmin.tcx+xml";
    case ".xml": return "application/xml";
    case ".zip": return "application/zip";
    default: return "application/octet-stream";
  }
}

export class TrainingPeaksDatabase {
  readonly database: DatabaseSync;
  private transactionOpen = false;

  constructor(databasePath: string) {
    mkdirSync(resolve(databasePath, ".."), { recursive: true });
    this.database = new DatabaseSync(databasePath);
    this.database.exec("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL; PRAGMA busy_timeout = 5000;");
    this.migrate();
  }

  private migrate(): void {
    this.database.exec(`
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
    `);
  }

  close(): void {
    this.database.close();
  }

  isComplete(identity: ExportIdentity): boolean {
    const row = this.database.prepare(`
      SELECT status FROM export_jobs
      WHERE athlete_id = ? AND export_kind = ? AND window_start = ? AND window_end = ?
    `).get(identity.athleteId, identity.kind, identity.window.start, identity.window.end) as { status?: string } | undefined;
    return row?.status === "complete";
  }

  startExport(identity: ExportIdentity, requestUrl: string): number {
    const timestamp = now();
    this.database.prepare(`
      INSERT INTO export_jobs (
        athlete_id, export_kind, window_start, window_end, request_url, status, started_at
      ) VALUES (?, ?, ?, ?, ?, 'started', ?)
      ON CONFLICT (athlete_id, export_kind, window_start, window_end) DO UPDATE SET
        request_url = excluded.request_url,
        status = 'started',
        archive_sha256 = NULL,
        started_at = excluded.started_at,
        completed_at = NULL,
        error = NULL,
        file_count = 0,
        workout_count = 0
    `).run(identity.athleteId, identity.kind, identity.window.start, identity.window.end, requestUrl, timestamp);

    const row = this.database.prepare(`
      SELECT id FROM export_jobs
      WHERE athlete_id = ? AND export_kind = ? AND window_start = ? AND window_end = ?
    `).get(identity.athleteId, identity.kind, identity.window.start, identity.window.end) as { id: number | bigint };
    return asNumber(row.id);
  }

  failExport(exportId: number, error: unknown): void {
    const message = error instanceof Error ? error.message : String(error);
    this.database.prepare(`
      UPDATE export_jobs SET status = 'failed', completed_at = ?, error = ? WHERE id = ?
    `).run(now(), message.slice(0, 4000), exportId);
  }

  begin(): void {
    this.database.exec("BEGIN IMMEDIATE");
    this.transactionOpen = true;
  }

  commit(): void {
    this.database.exec("COMMIT");
    this.transactionOpen = false;
  }

  rollback(): void {
    if (!this.transactionOpen) return;
    try {
      this.database.exec("ROLLBACK");
    } finally {
      this.transactionOpen = false;
    }
  }

  resetExportLinks(exportId: number): void {
    this.database.prepare("DELETE FROM export_files WHERE export_id = ?").run(exportId);
    this.database.prepare("DELETE FROM workout_exports WHERE export_id = ?").run(exportId);
  }

  storeFile(athleteId: string, exportId: number, relativePath: string, absolutePath: string): StoredFile {
    const content = readFileSync(absolutePath);
    const sha256 = createHash("sha256").update(content).digest("hex");
    const timestamp = now();

    this.database.prepare(`
      INSERT INTO file_blobs (sha256, byte_size, content) VALUES (?, ?, ?)
      ON CONFLICT (sha256) DO NOTHING
    `).run(sha256, content.byteLength, content);
    const blob = this.database.prepare("SELECT id FROM file_blobs WHERE sha256 = ?").get(sha256) as { id: number | bigint };
    const blobId = asNumber(blob.id);

    this.database.prepare(`
      INSERT INTO files (
        athlete_id, relative_path, file_name, extension, mime_type, blob_id, first_seen_at, last_seen_at
      ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
      ON CONFLICT (athlete_id, relative_path, blob_id) DO UPDATE SET last_seen_at = excluded.last_seen_at
    `).run(
      athleteId,
      relativePath,
      basename(relativePath),
      extname(relativePath).toLowerCase().replace(/^\./, ""),
      mimeType(relativePath),
      blobId,
      timestamp,
      timestamp,
    );
    const file = this.database.prepare(`
      SELECT id FROM files WHERE athlete_id = ? AND relative_path = ? AND blob_id = ?
    `).get(athleteId, relativePath, blobId) as { id: number | bigint };
    const fileId = asNumber(file.id);
    this.database.prepare(`
      INSERT INTO export_files (export_id, file_id) VALUES (?, ?)
      ON CONFLICT DO NOTHING
    `).run(exportId, fileId);

    return { id: fileId, sha256, size: content.byteLength };
  }

  storeWorkout(
    athleteId: string,
    exportId: number,
    sourceFileId: number,
    sourceRowNumber: number,
    row: WorkoutRow,
  ): number {
    const details = workoutDetails(row);
    const timestamp = now();
    this.database.prepare(`
      INSERT INTO workouts (
        athlete_id, stable_key, external_workout_id, workout_date, title, workout_type,
        description, raw_json, source_file_id, source_row_number, first_seen_at, updated_at
      ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
      ON CONFLICT (athlete_id, stable_key) DO UPDATE SET
        external_workout_id = excluded.external_workout_id,
        workout_date = excluded.workout_date,
        title = excluded.title,
        workout_type = excluded.workout_type,
        description = excluded.description,
        raw_json = excluded.raw_json,
        source_file_id = excluded.source_file_id,
        source_row_number = excluded.source_row_number,
        updated_at = excluded.updated_at
    `).run(
      athleteId,
      details.stableKey,
      details.externalId,
      details.workoutDate,
      details.title,
      details.workoutType,
      details.description,
      details.rawJson,
      sourceFileId,
      sourceRowNumber,
      timestamp,
      timestamp,
    );

    const workout = this.database.prepare(`
      SELECT id FROM workouts WHERE athlete_id = ? AND stable_key = ?
    `).get(athleteId, details.stableKey) as { id: number | bigint };
    const workoutId = asNumber(workout.id);

    this.database.prepare("DELETE FROM workout_fields WHERE workout_id = ?").run(workoutId);
    const insertField = this.database.prepare(`
      INSERT INTO workout_fields (workout_id, field_name, value_text) VALUES (?, ?, ?)
    `);
    for (const [fieldName, value] of Object.entries(row)) {
      insertField.run(workoutId, fieldName, stringValue(value));
    }
    this.database.prepare(`
      INSERT INTO workout_exports (workout_id, export_id) VALUES (?, ?)
      ON CONFLICT DO NOTHING
    `).run(workoutId, exportId);
    return workoutId;
  }

  completeExport(exportId: number, archiveSha256: string, counts: ImportCounts): void {
    this.database.prepare(`
      UPDATE export_jobs SET
        status = 'complete', archive_sha256 = ?, completed_at = ?, error = NULL,
        file_count = ?, workout_count = ?
      WHERE id = ?
    `).run(archiveSha256, now(), counts.files, counts.workouts, exportId);
  }
}
