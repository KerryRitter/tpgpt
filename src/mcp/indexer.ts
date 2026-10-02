#!/usr/bin/env node
import { existsSync } from "node:fs";
import { resolve } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { ensureMcpSchema, refreshWorkoutSearch } from "./schema.js";
import {
  decodeActivityFile,
  isoField,
  numberField,
  stringField,
  summarizeRecords,
  type DecodedActivity,
  type FitRecord,
} from "./fit.js";

export interface IndexOptions {
  databasePath: string;
  force: boolean;
  limit: number | null;
  timeZone: string;
}

export function parseIndexOptions(argv: string[]): IndexOptions {
  let databasePath = process.env.TRAININGPEAKS_DATABASE ?? "trainingpeaks.sqlite";
  let force = false;
  let limit: number | null = null;
  let timeZone = process.env.TRAININGPEAKS_TIMEZONE ?? "America/Menominee";
  for (let index = 0; index < argv.length; index += 1) {
    const option = argv[index];
    const value = argv[index + 1];
    if (option === "--database" && value) {
      databasePath = value;
      index += 1;
    } else if (option === "--timezone" && value) {
      timeZone = value;
      index += 1;
    } else if (option === "--limit" && value) {
      limit = Number(value);
      if (!Number.isInteger(limit) || limit < 1) throw new Error("--limit must be a positive integer");
      index += 1;
    } else if (option === "--force") {
      force = true;
    } else if (option === "--help" || option === "-h") {
      process.stdout.write("Usage: npm run mcp:index -- [--database PATH] [--timezone IANA_ZONE] [--force] [--limit N]\n");
      process.exit(0);
    } else {
      throw new Error(`Unknown option: ${option}`);
    }
  }
  // Validate now so a typo does not silently shift workout dates.
  new Intl.DateTimeFormat("en-US", { timeZone }).format(new Date());
  return { databasePath: resolve(databasePath), force, limit, timeZone };
}

function localIsoDate(isoTimestamp: string | null, timeZone: string): string | null {
  if (!isoTimestamp) return null;
  const parts = new Intl.DateTimeFormat("en-US", {
    timeZone,
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
  }).formatToParts(new Date(isoTimestamp));
  const lookup = Object.fromEntries(parts.map((part) => [part.type, part.value]));
  return lookup.year && lookup.month && lookup.day ? `${lookup.year}-${lookup.month}-${lookup.day}` : null;
}

function firstNumber(primary: FitRecord | undefined, fallback: FitRecord, ...names: string[]): number | null {
  return numberField(primary, ...names) ?? numberField(fallback, ...names);
}

function firstText(primary: FitRecord | undefined, fallback: FitRecord, ...names: string[]): string | null {
  return stringField(primary, ...names) ?? stringField(fallback, ...names);
}

function indexDecoded(database: DatabaseSync, decoded: DecodedActivity, timeZone: string): number {
  const primary = decoded.sessions[0] ?? decoded.laps[0];
  const recordSummary = summarizeRecords(decoded.records);
  const startTime = isoField(primary, "startTime", "timestamp") ?? isoField(recordSummary, "startTime");
  const localDate = localIsoDate(startTime, timeZone);
  const timestamp = new Date().toISOString();
  const errors = decoded.errors.join("\n") || null;
  const summaryJson = JSON.stringify({
    sessions: decoded.sessions,
    fileIds: decoded.fileIds,
  });

  database.prepare(`
    INSERT INTO activity_files (
      file_id, relative_path, file_format, fit_valid, crc_valid, start_time, local_date, sport, sub_sport,
      total_elapsed_seconds, total_timer_seconds, total_distance_meters, total_calories,
      total_ascent_meters, avg_heart_rate, max_heart_rate, avg_power, max_power,
      normalized_power, avg_cadence, max_cadence, avg_speed_mps, max_speed_mps,
      record_count, lap_count, decode_errors, summary_json, indexed_at
    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    ON CONFLICT (file_id) DO UPDATE SET
      relative_path = excluded.relative_path,
      file_format = excluded.file_format,
      fit_valid = excluded.fit_valid,
      crc_valid = excluded.crc_valid,
      start_time = excluded.start_time,
      local_date = excluded.local_date,
      sport = excluded.sport,
      sub_sport = excluded.sub_sport,
      total_elapsed_seconds = excluded.total_elapsed_seconds,
      total_timer_seconds = excluded.total_timer_seconds,
      total_distance_meters = excluded.total_distance_meters,
      total_calories = excluded.total_calories,
      total_ascent_meters = excluded.total_ascent_meters,
      avg_heart_rate = excluded.avg_heart_rate,
      max_heart_rate = excluded.max_heart_rate,
      avg_power = excluded.avg_power,
      max_power = excluded.max_power,
      normalized_power = excluded.normalized_power,
      avg_cadence = excluded.avg_cadence,
      max_cadence = excluded.max_cadence,
      avg_speed_mps = excluded.avg_speed_mps,
      max_speed_mps = excluded.max_speed_mps,
      record_count = excluded.record_count,
      lap_count = excluded.lap_count,
      decode_errors = excluded.decode_errors,
      summary_json = excluded.summary_json,
      indexed_at = excluded.indexed_at
  `).run(
    decoded.fileId,
    decoded.relativePath,
    decoded.format,
    decoded.fitValid ? 1 : 0,
    decoded.crcValid ? 1 : 0,
    startTime,
    localDate,
    firstText(primary, recordSummary, "sport"),
    firstText(primary, recordSummary, "subSport"),
    firstNumber(primary, recordSummary, "totalElapsedTime"),
    firstNumber(primary, recordSummary, "totalTimerTime"),
    firstNumber(primary, recordSummary, "totalDistance"),
    firstNumber(primary, recordSummary, "totalCalories"),
    firstNumber(primary, recordSummary, "totalAscent"),
    firstNumber(primary, recordSummary, "avgHeartRate"),
    firstNumber(primary, recordSummary, "maxHeartRate"),
    firstNumber(primary, recordSummary, "avgPower"),
    firstNumber(primary, recordSummary, "maxPower"),
    firstNumber(primary, recordSummary, "normalizedPower", "normalizedPower2"),
    firstNumber(primary, recordSummary, "avgCadence"),
    firstNumber(primary, recordSummary, "maxCadence"),
    firstNumber(primary, recordSummary, "enhancedAvgSpeed", "avgSpeed"),
    firstNumber(primary, recordSummary, "enhancedMaxSpeed", "maxSpeed"),
    decoded.records.length,
    decoded.laps.length,
    errors,
    summaryJson,
    timestamp,
  );

  const activity = database.prepare("SELECT id FROM activity_files WHERE file_id = ?").get(decoded.fileId) as { id: number };
  database.prepare("DELETE FROM activity_laps WHERE activity_file_id = ?").run(activity.id);
  const insertLap = database.prepare(`
    INSERT INTO activity_laps (
      activity_file_id, lap_index, start_time, elapsed_seconds, timer_seconds, distance_meters,
      avg_heart_rate, max_heart_rate, avg_power, max_power, avg_cadence, max_cadence,
      avg_speed_mps, max_speed_mps, ascent_meters, calories, intensity, raw_json
    ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
  `);
  decoded.laps.forEach((lap, index) => {
    insertLap.run(
      activity.id,
      index,
      isoField(lap, "startTime", "timestamp"),
      numberField(lap, "totalElapsedTime"),
      numberField(lap, "totalTimerTime"),
      numberField(lap, "totalDistance"),
      numberField(lap, "avgHeartRate"),
      numberField(lap, "maxHeartRate"),
      numberField(lap, "avgPower"),
      numberField(lap, "maxPower"),
      numberField(lap, "avgCadence"),
      numberField(lap, "maxCadence"),
      numberField(lap, "enhancedAvgSpeed", "avgSpeed"),
      numberField(lap, "enhancedMaxSpeed", "maxSpeed"),
      numberField(lap, "totalAscent"),
      numberField(lap, "totalCalories"),
      stringField(lap, "intensity"),
      JSON.stringify(lap),
    );
  });
  return activity.id;
}

function dateDistance(left: string, right: string): number {
  return Math.abs((Date.parse(`${left}T00:00:00Z`) - Date.parse(`${right}T00:00:00Z`)) / 86_400_000);
}

function dateShift(date: string, days: number): string {
  const shifted = new Date(`${date}T00:00:00Z`);
  shifted.setUTCDate(shifted.getUTCDate() + days);
  return shifted.toISOString().slice(0, 10);
}

function expectedWorkoutTypes(sport: string | null): Set<string> {
  const mapping: Record<string, string[]> = {
    running: ["Run"],
    cycling: ["Bike", "MTB"],
    swimming: ["Swim"],
    walking: ["Walk"],
    rowing: ["Rowing"],
    training: ["Strength", "Other", "X-Train"],
    fitnessEquipment: ["Strength", "Other", "X-Train", "Rowing"],
  };
  return new Set(sport ? mapping[sport] ?? [] : []);
}

function relativeDifference(left: number | null, right: number | null): number {
  if (!left || !right) return 0;
  return Math.abs(left - right) / Math.max(left, right, 1);
}

function matchWorkouts(database: DatabaseSync): number {
  const activities = database.prepare(`
    SELECT id, local_date, sport, total_timer_seconds, total_distance_meters
    FROM activity_files
    WHERE fit_valid = 1 AND local_date IS NOT NULL
  `).all() as Array<{
    id: number;
    local_date: string;
    sport: string;
    total_timer_seconds: number | null;
    total_distance_meters: number | null;
  }>;
  const workoutRows = database.prepare(`
    SELECT id, workout_date, workout_type, duration_hours, distance_meters
    FROM workout_metrics
  `).all() as Array<{
    id: number;
    workout_date: string;
    workout_type: string;
    duration_hours: number | null;
    distance_meters: number | null;
  }>;
  const workoutsByDate = new Map<string, typeof workoutRows>();
  for (const workout of workoutRows) {
    const group = workoutsByDate.get(workout.workout_date) ?? [];
    group.push(workout);
    workoutsByDate.set(workout.workout_date, group);
  }
  const update = database.prepare("UPDATE activity_files SET workout_id = ? WHERE id = ?");
  let matched = 0;
  database.exec("BEGIN IMMEDIATE");
  try {
    for (const activity of activities) {
      const rows = [dateShift(activity.local_date, -1), activity.local_date, dateShift(activity.local_date, 1)]
        .flatMap((date) => workoutsByDate.get(date) ?? []);
      const expected = expectedWorkoutTypes(activity.sport);
      const ranked = rows.map((workout) => ({
        workout,
        score:
          dateDistance(activity.local_date, workout.workout_date) * 20 +
          (expected.size > 0 && !expected.has(workout.workout_type) ? 12 : 0) +
          relativeDifference(activity.total_timer_seconds, workout.duration_hours ? workout.duration_hours * 3600 : null) * 5 +
          relativeDifference(activity.total_distance_meters, workout.distance_meters) * 5,
      })).sort((left, right) => left.score - right.score);
      const best = ranked[0];
      if (best && best.score < 20) {
        update.run(best.workout.id, activity.id);
        matched += 1;
      }
    }
    database.exec("COMMIT");
  } catch (error) {
    database.exec("ROLLBACK");
    throw error;
  }
  return matched;
}

export async function indexActivities(options: IndexOptions, log: (message: string) => void = (message) => process.stdout.write(`${message}\n`)): Promise<void> {
  if (!existsSync(options.databasePath)) throw new Error(`Database does not exist: ${options.databasePath}`);
  const database = new DatabaseSync(options.databasePath);
  ensureMcpSchema(database);
  const searchChanged = refreshWorkoutSearch(database, options.force);
  const where = options.force
    ? "WHERE f.extension IN ('gz', 'fit', 'tcx', 'gpx')"
    : "WHERE f.extension IN ('gz', 'fit', 'tcx', 'gpx') AND af.file_id IS NULL";
  const limit = options.limit ? ` LIMIT ${options.limit}` : "";
  const files = database.prepare(`
    SELECT f.id FROM files AS f
    LEFT JOIN activity_files AS af ON af.file_id = f.id
    ${where}
    ORDER BY f.id${limit}
  `).all() as Array<{ id: number }>;

  log(`Workout search index: ${searchChanged ? "rebuilt" : "current"}. Activity files to index: ${files.length}.`);
  let decodedCount = 0;
  let invalidCount = 0;
  let errorCount = 0;
  for (const [index, file] of files.entries()) {
    let decoded: DecodedActivity;
    try {
      decoded = decodeActivityFile(database, file.id);
      if (!decoded.fitValid || decoded.errors.length > 0) invalidCount += 1;
    } catch (error) {
      errorCount += 1;
      const message = error instanceof Error ? error.message : String(error);
      const path = database.prepare("SELECT relative_path FROM files WHERE id = ?").get(file.id) as { relative_path: string };
      decoded = {
        fileId: file.id,
        relativePath: path.relative_path,
        format: "UNKNOWN",
        fitValid: false,
        crcValid: false,
        errors: [message],
        sessions: [],
        laps: [],
        records: [],
        fileIds: [],
      };
    }
    database.exec("BEGIN IMMEDIATE");
    try {
      indexDecoded(database, decoded, options.timeZone);
      database.exec("COMMIT");
    } catch (error) {
      database.exec("ROLLBACK");
      throw error;
    }
    decodedCount += 1;
    if ((index + 1) % 50 === 0 || index + 1 === files.length) {
      log(`Indexed ${index + 1}/${files.length} files.`);
    }
  }

  const matched = matchWorkouts(database);
  const totals = database.prepare(`
    SELECT COUNT(*) AS indexed, SUM(fit_valid) AS valid, SUM(workout_id IS NOT NULL) AS linked,
           SUM(record_count) AS records, SUM(lap_count) AS laps
    FROM activity_files
  `).get();
  database.close();
  log(JSON.stringify({ decoded: decodedCount, invalid: invalidCount, errors: errorCount, matchedThisPass: matched, totals }));
}
