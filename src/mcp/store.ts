import { existsSync } from "node:fs";
import { resolve } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { decodeActivityFile, sampleRecords } from "./fit.js";
import { ensureMcpSchema, refreshWorkoutSearch } from "./schema.js";

type SqlParameter = string | number | null;
type Row = Record<string, unknown>;

export interface TrainingSummaryInput {
  startDate?: string | undefined;
  endDate?: string | undefined;
  workoutTypes?: string[] | undefined;
  groupBy?: "none" | "week" | "month" | "type" | undefined;
}

export interface PlanSessionInput {
  date: string;
  sport: string;
  title: string;
  durationMinutes?: number | undefined;
  targetTss?: number | undefined;
  intensity?: string | undefined;
  description: string;
}

function assertDate(value: string, label: string): void {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(value) || Number.isNaN(Date.parse(`${value}T00:00:00Z`))) {
    throw new Error(`${label} must be a valid YYYY-MM-DD date`);
  }
}

function dateShift(date: string, days: number): string {
  assertDate(date, "date");
  const shifted = new Date(`${date}T00:00:00Z`);
  shifted.setUTCDate(shifted.getUTCDate() + days);
  return shifted.toISOString().slice(0, 10);
}

function ftsQuery(text: string): string {
  const tokens = text.normalize("NFKC").match(/[\p{L}\p{N}]+/gu)?.slice(0, 24) ?? [];
  if (tokens.length === 0) throw new Error("Search query must contain a letter or number");
  return tokens.map((token) => `"${token.replaceAll('"', '""')}"*`).join(" AND ");
}

function round(value: number | null, digits = 2): number | null {
  if (value === null || !Number.isFinite(value)) return null;
  const factor = 10 ** digits;
  return Math.round(value * factor) / factor;
}

function numberOrNull(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function normalizedRow(row: Row): Row {
  return Object.fromEntries(Object.entries(row).map(([key, value]) => [
    key,
    typeof value === "bigint" ? Number(value) : value,
  ]));
}

function inclusiveDates(start: string, end: string): string[] {
  const dates: string[] = [];
  for (let date = start; date <= end; date = dateShift(date, 1)) dates.push(date);
  return dates;
}

export class TrainingDataStore {
  readonly database: DatabaseSync;
  readonly databasePath: string;

  constructor(databasePath: string) {
    this.databasePath = resolve(databasePath);
    if (!existsSync(this.databasePath)) throw new Error(`TrainingPeaks database does not exist: ${this.databasePath}`);
    this.database = new DatabaseSync(this.databasePath);
    this.database.exec("PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;");
    ensureMcpSchema(this.database);
    refreshWorkoutSearch(this.database);
  }

  close(): void {
    this.database.close();
  }

  overview(): Row {
    const coverage = this.database.prepare(`
      SELECT COUNT(*) AS workouts, MIN(workout_date) AS first_date, MAX(workout_date) AS last_date,
             COUNT(DISTINCT workout_type) AS workout_types
      FROM workouts
    `).get() as Row;
    const files = this.database.prepare(`
      SELECT COUNT(*) AS logical_files, COUNT(DISTINCT blob_id) AS unique_blobs
      FROM files
    `).get() as Row;
    const activities = this.database.prepare(`
      SELECT COUNT(*) AS indexed_files, SUM(fit_valid) AS valid_activity_files,
             SUM(crc_valid) AS integrity_valid_files, SUM(workout_id IS NOT NULL) AS linked_files,
             SUM(record_count) AS fit_records, SUM(lap_count) AS fit_laps
      FROM activity_files
    `).get() as Row;
    const types = this.database.prepare(`
      SELECT workout_type, COUNT(*) AS workouts
      FROM workouts GROUP BY workout_type ORDER BY workouts DESC
    `).all().map((row) => normalizedRow(row as Row));
    const plans = this.database.prepare("SELECT COUNT(*) AS saved_plans FROM training_plans").get() as Row;
    return {
      databasePath: this.databasePath,
      ...normalizedRow(coverage),
      ...normalizedRow(files),
      ...normalizedRow(activities),
      ...normalizedRow(plans),
      workoutTypeCounts: types,
    };
  }

  summarize(input: TrainingSummaryInput): Row {
    const groupBy = input.groupBy ?? "none";
    const conditions: string[] = ["1 = 1"];
    const parameters: SqlParameter[] = [];
    if (input.startDate) {
      assertDate(input.startDate, "startDate");
      conditions.push("workout_date >= ?");
      parameters.push(input.startDate);
    }
    if (input.endDate) {
      assertDate(input.endDate, "endDate");
      conditions.push("workout_date <= ?");
      parameters.push(input.endDate);
    }
    if (input.workoutTypes?.length) {
      conditions.push(`workout_type IN (${input.workoutTypes.map(() => "?").join(", ")})`);
      parameters.push(...input.workoutTypes);
    }

    const periodExpression: Record<NonNullable<TrainingSummaryInput["groupBy"]>, string> = {
      none: "'all'",
      week: "date(workout_date, '-' || ((CAST(strftime('%w', workout_date) AS INTEGER) + 6) % 7) || ' days')",
      month: "strftime('%Y-%m', workout_date)",
      type: "workout_type",
    };
    const sql = `
      SELECT
        ${periodExpression[groupBy]} AS period,
        COUNT(*) AS workout_count,
        SUM(CASE WHEN COALESCE(duration_hours, 0) > 0 OR COALESCE(distance_meters, 0) > 0 THEN 1 ELSE 0 END) AS completed_count,
        ROUND(SUM(COALESCE(duration_hours, 0)), 2) AS duration_hours,
        ROUND(SUM(COALESCE(planned_duration_hours, 0)), 2) AS planned_duration_hours,
        ROUND(SUM(COALESCE(distance_meters, 0)) / 1000.0, 2) AS distance_km,
        ROUND(SUM(COALESCE(planned_distance_meters, 0)) / 1000.0, 2) AS planned_distance_km,
        ROUND(SUM(COALESCE(tss, 0)), 1) AS tss,
        ROUND(AVG(CASE WHEN intensity_factor > 0 THEN intensity_factor END), 3) AS avg_intensity_factor,
        ROUND(AVG(CASE WHEN heart_rate_average > 0 THEN heart_rate_average END), 1) AS avg_heart_rate,
        ROUND(AVG(CASE WHEN power_average > 0 THEN power_average END), 1) AS avg_power,
        ROUND(AVG(CASE WHEN rpe > 0 THEN rpe END), 1) AS avg_rpe
      FROM workout_metrics
      WHERE ${conditions.join(" AND ")}
      ${groupBy === "none" ? "" : "GROUP BY period"}
      ORDER BY period
    `;
    const rows = this.database.prepare(sql).all(...parameters).map((row) => normalizedRow(row as Row));
    return { filters: input, groups: rows };
  }

  searchWorkouts(input: {
    query?: string | undefined;
    startDate?: string | undefined;
    endDate?: string | undefined;
    workoutTypes?: string[] | undefined;
    limit?: number | undefined;
    offset?: number | undefined;
  }): Row {
    const conditions: string[] = ["1 = 1"];
    const parameters: SqlParameter[] = [];
    const usingSearch = Boolean(input.query?.trim());
    if (usingSearch) {
      conditions.push("workout_search MATCH ?");
      parameters.push(ftsQuery(input.query!));
    }
    if (input.startDate) {
      assertDate(input.startDate, "startDate");
      conditions.push("w.workout_date >= ?");
      parameters.push(input.startDate);
    }
    if (input.endDate) {
      assertDate(input.endDate, "endDate");
      conditions.push("w.workout_date <= ?");
      parameters.push(input.endDate);
    }
    if (input.workoutTypes?.length) {
      conditions.push(`w.workout_type IN (${input.workoutTypes.map(() => "?").join(", ")})`);
      parameters.push(...input.workoutTypes);
    }
    const limit = Math.min(100, Math.max(1, input.limit ?? 20));
    const offset = Math.max(0, input.offset ?? 0);
    const source = usingSearch
      ? "workout_search JOIN workout_metrics AS w ON w.id = CAST(workout_search.workout_id AS INTEGER)"
      : "workout_metrics AS w";
    const snippet = usingSearch
      ? "snippet(workout_search, -1, '[', ']', ' … ', 24) AS match_excerpt, bm25(workout_search) AS rank,"
      : "NULL AS match_excerpt, NULL AS rank,";
    const order = usingSearch ? "rank ASC, w.workout_date DESC" : "w.workout_date DESC, w.id DESC";
    const rows = this.database.prepare(`
      SELECT w.id, w.workout_date, w.workout_type, w.title,
             ${snippet}
             ROUND(w.duration_hours, 3) AS duration_hours,
             ROUND(w.distance_meters / 1000.0, 2) AS distance_km,
             ROUND(w.tss, 1) AS tss,
             ROUND(w.intensity_factor, 3) AS intensity_factor,
             w.rpe, w.feeling
      FROM ${source}
      WHERE ${conditions.join(" AND ")}
      ORDER BY ${order}
      LIMIT ? OFFSET ?
    `).all(...parameters, limit, offset).map((row) => normalizedRow(row as Row));
    return { query: input.query ?? null, count: rows.length, limit, offset, workouts: rows };
  }

  getWorkout(workoutId: number): Row {
    const workout = this.database.prepare("SELECT * FROM workout_metrics WHERE id = ?").get(workoutId) as Row | undefined;
    if (!workout) throw new Error(`No workout has ID ${workoutId}`);
    const fields = this.database.prepare(`
      SELECT field_name, value_text FROM workout_fields WHERE workout_id = ? ORDER BY field_name
    `).all(workoutId) as Array<{ field_name: string; value_text: string | null }>;
    const activities = this.database.prepare(`
      SELECT id AS activity_file_id, file_id, relative_path, file_format, fit_valid, crc_valid, start_time,
             sport, sub_sport, total_timer_seconds, total_distance_meters, total_calories,
             avg_heart_rate, max_heart_rate, avg_power, max_power, normalized_power,
             record_count, lap_count, decode_errors
      FROM activity_files WHERE workout_id = ? ORDER BY start_time
    `).all(workoutId).map((row) => normalizedRow(row as Row));
    return {
      workout: normalizedRow(workout),
      originalFields: Object.fromEntries(fields.map((field) => [field.field_name, field.value_text])),
      activityFiles: activities,
    };
  }

  getActivityDetail(fileId: number, maximumSamples: number): Row {
    const decoded = decodeActivityFile(this.database, fileId);
    return {
      fileId,
      relativePath: decoded.relativePath,
      format: decoded.format,
      fitValid: decoded.fitValid,
      crcValid: decoded.crcValid,
      errors: decoded.errors,
      sessions: decoded.sessions,
      laps: decoded.laps.slice(0, 200),
      totalLapCount: decoded.laps.length,
      samples: sampleRecords(decoded.records, Math.min(500, Math.max(0, maximumSamples))),
      totalRecordCount: decoded.records.length,
    };
  }

  personalRecords(input: {
    metric: "distance" | "duration" | "tss" | "average_power" | "max_power" | "average_heart_rate";
    workoutType?: string | undefined;
    startDate?: string | undefined;
    endDate?: string | undefined;
    limit?: number | undefined;
  }): Row {
    const columns = {
      distance: { column: "distance_meters", unit: "meters" },
      duration: { column: "duration_hours", unit: "hours" },
      tss: { column: "tss", unit: "TSS" },
      average_power: { column: "power_average", unit: "watts" },
      max_power: { column: "power_max", unit: "watts" },
      average_heart_rate: { column: "heart_rate_average", unit: "bpm" },
    } as const;
    const selected = columns[input.metric];
    const conditions = [`${selected.column} IS NOT NULL`, `${selected.column} > 0`];
    const parameters: SqlParameter[] = [];
    if (input.workoutType) {
      conditions.push("workout_type = ?");
      parameters.push(input.workoutType);
    }
    if (input.startDate) {
      assertDate(input.startDate, "startDate");
      conditions.push("workout_date >= ?");
      parameters.push(input.startDate);
    }
    if (input.endDate) {
      assertDate(input.endDate, "endDate");
      conditions.push("workout_date <= ?");
      parameters.push(input.endDate);
    }
    const limit = Math.min(50, Math.max(1, input.limit ?? 10));
    const records = this.database.prepare(`
      SELECT id, workout_date, workout_type, title, ${selected.column} AS value,
             duration_hours, ROUND(distance_meters / 1000.0, 2) AS distance_km, tss
      FROM workout_metrics
      WHERE ${conditions.join(" AND ")}
      ORDER BY ${selected.column} DESC
      LIMIT ?
    `).all(...parameters, limit).map((row) => normalizedRow(row as Row));
    return { metric: input.metric, unit: selected.unit, records };
  }

  trainingLoad(endDate: string, days = 84): Row {
    assertDate(endDate, "endDate");
    const visibleDays = Math.min(365, Math.max(7, days));
    const historyStart = dateShift(endDate, -(visibleDays + 84));
    const visibleStart = dateShift(endDate, -(visibleDays - 1));
    const rows = this.database.prepare(`
      SELECT workout_date, SUM(COALESCE(tss, 0)) AS daily_tss,
             SUM(COALESCE(duration_hours, 0)) AS duration_hours
      FROM workout_metrics
      WHERE workout_date BETWEEN ? AND ?
      GROUP BY workout_date
    `).all(historyStart, endDate) as Array<{ workout_date: string; daily_tss: number; duration_hours: number }>;
    const byDate = new Map(rows.map((row) => [row.workout_date, row]));
    let atl = 0;
    let ctl = 0;
    const daily: Row[] = [];
    for (const date of inclusiveDates(historyStart, endDate)) {
      const values = byDate.get(date);
      const tss = values?.daily_tss ?? 0;
      const previousAtl = atl;
      const previousCtl = ctl;
      atl += (tss - atl) / 7;
      ctl += (tss - ctl) / 42;
      if (date >= visibleStart) {
        daily.push({
          date,
          tss: round(tss, 1),
          durationHours: round(values?.duration_hours ?? 0, 2),
          acuteLoad7d: round(atl, 1),
          chronicLoad42d: round(ctl, 1),
          form: round(previousCtl - previousAtl, 1),
        });
      }
    }
    const latest = daily.at(-1) ?? {};
    const weekAgo = daily.at(-8);
    return {
      endDate,
      days: visibleDays,
      methodology: "Exponentially weighted daily TSS with 7-day acute and 42-day chronic time constants; form uses prior-day chronic minus acute load.",
      current: {
        ...latest,
        sevenDayChronicRamp: round(
          numberOrNull(latest.chronicLoad42d) !== null && numberOrNull(weekAgo?.chronicLoad42d) !== null
            ? numberOrNull(latest.chronicLoad42d)! - numberOrNull(weekAgo?.chronicLoad42d)!
            : null,
          1,
        ),
      },
      daily,
    };
  }

  planningContext(asOfDate: string, weeks = 8): Row {
    assertDate(asOfDate, "asOfDate");
    const boundedWeeks = Math.min(26, Math.max(4, weeks));
    const startDate = dateShift(asOfDate, -(boundedWeeks * 7 - 1));
    const weekly = this.summarize({ startDate, endDate: asOfDate, groupBy: "week" });
    const bySport = this.summarize({ startDate, endDate: asOfDate, groupBy: "type" });
    const comments = this.database.prepare(`
      SELECT id, workout_date, workout_type, title,
             substr(athlete_comments, 1, 1500) AS athlete_comments,
             rpe, feeling
      FROM workout_metrics
      WHERE workout_date BETWEEN ? AND ? AND athlete_comments IS NOT NULL
      ORDER BY workout_date DESC LIMIT 12
    `).all(startDate, asOfDate).map((row) => normalizedRow(row as Row));
    const load = this.trainingLoad(asOfDate, Math.min(84, boundedWeeks * 7));
    return {
      asOfDate,
      lookbackWeeks: boundedWeeks,
      startDate,
      weekly: weekly.groups,
      sportBalance: bySport.groups,
      trainingLoad: load.current,
      recentAthleteComments: comments,
      planningGuardrails: [
        "Confirm current goals, event date, available days, equipment, and preferred rest day.",
        "Ask about current pain, injury, illness, or medical restrictions before prescribing intensity.",
        "Use historical completed volume as the starting point and progress conservatively with regular recovery weeks.",
        "Treat unusually negative form, escalating RPE, or repeated adverse comments as reasons to reduce load and seek qualified guidance.",
        "A generated plan is general training guidance, not medical diagnosis or treatment.",
      ],
    };
  }

  planFramework(input: {
    goal: string;
    primarySport: string;
    startDate: string;
    endDate: string;
    daysPerWeek: number;
    targetWeeklyHours?: number | undefined;
  }): Row {
    assertDate(input.startDate, "startDate");
    assertDate(input.endDate, "endDate");
    if (input.endDate < input.startDate) throw new Error("endDate must be on or after startDate");
    const totalDays = Math.floor((Date.parse(`${input.endDate}T00:00:00Z`) - Date.parse(`${input.startDate}T00:00:00Z`)) / 86_400_000) + 1;
    const weekCount = Math.max(1, Math.ceil(totalDays / 7));
    const baselineStart = dateShift(input.startDate, -42);
    const baselineRows = this.summarize({
      startDate: baselineStart,
      endDate: dateShift(input.startDate, -1),
      groupBy: "none",
    }).groups as Row[];
    const baseline = baselineRows[0] ?? {};
    const baselineHours = Math.max(1, (numberOrNull(baseline.duration_hours) ?? 0) / 6);
    const baselineTss = Math.max(50, (numberOrNull(baseline.tss) ?? 0) / 6);
    const requestedTarget = input.targetWeeklyHours ?? baselineHours * 1.1;
    const weeks: Row[] = [];
    let previousHours = baselineHours;
    let peakHours = baselineHours;
    for (let index = 0; index < weekCount; index += 1) {
      const number = index + 1;
      const remaining = weekCount - number;
      let phase = "base";
      if (weekCount >= 6 && remaining <= 1) phase = "taper";
      else if (number > Math.ceil(weekCount * 0.7)) phase = "peak";
      else if (number > Math.ceil(weekCount * 0.4)) phase = "build";
      const recovery = number % 4 === 0 && phase !== "taper";
      let hours = Math.min(requestedTarget, previousHours * 1.07);
      if (recovery) hours = previousHours * 0.78;
      if (phase === "taper") hours = peakHours * (remaining === 1 ? 0.75 : 0.55);
      if (!recovery && phase !== "taper") peakHours = Math.max(peakHours, hours);
      const tssPerHour = baselineTss / baselineHours;
      const start = dateShift(input.startDate, index * 7);
      weeks.push({
        week: number,
        startDate: start,
        endDate: [dateShift(start, 6), input.endDate].sort()[0],
        phase,
        recoveryWeek: recovery,
        targetHours: round(hours, 1),
        targetTss: round(hours * tssPerHour, 0),
        trainingDays: input.daysPerWeek,
      });
      previousHours = recovery ? peakHours : hours;
    }
    return {
      goal: input.goal,
      primarySport: input.primarySport,
      startDate: input.startDate,
      endDate: input.endDate,
      baseline: { weeklyHours: round(baselineHours, 1), weeklyTss: round(baselineTss, 0), lookbackStart: baselineStart },
      requestedTargetWeeklyHours: input.targetWeeklyHours ?? null,
      weeks,
      note: "This is a volume/load scaffold. Use get_planning_context and workout history to turn it into sessions, preserve recovery, and adapt for health constraints.",
    };
  }

  savePlan(input: {
    planId?: number | undefined;
    name: string;
    goal: string;
    startDate: string;
    endDate: string;
    status?: "draft" | "active" | "completed" | "archived" | undefined;
    notes?: string | undefined;
    sessions: PlanSessionInput[];
  }): Row {
    assertDate(input.startDate, "startDate");
    assertDate(input.endDate, "endDate");
    if (input.endDate < input.startDate) throw new Error("endDate must be on or after startDate");
    input.sessions.forEach((session, index) => {
      assertDate(session.date, `sessions[${index}].date`);
      if (session.date < input.startDate || session.date > input.endDate) {
        throw new Error(`sessions[${index}].date is outside the plan date range`);
      }
    });
    const timestamp = new Date().toISOString();
    this.database.exec("BEGIN IMMEDIATE");
    try {
      let planId = input.planId;
      if (planId) {
        const exists = this.database.prepare("SELECT id FROM training_plans WHERE id = ?").get(planId);
        if (!exists) throw new Error(`No training plan has ID ${planId}`);
        this.database.prepare(`
          UPDATE training_plans SET name = ?, goal = ?, start_date = ?, end_date = ?,
            status = ?, notes = ?, updated_at = ? WHERE id = ?
        `).run(input.name, input.goal, input.startDate, input.endDate, input.status ?? "draft", input.notes ?? null, timestamp, planId);
        this.database.prepare("DELETE FROM planned_sessions WHERE plan_id = ?").run(planId);
      } else {
        const result = this.database.prepare(`
          INSERT INTO training_plans (name, goal, start_date, end_date, status, notes, created_at, updated_at)
          VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        `).run(input.name, input.goal, input.startDate, input.endDate, input.status ?? "draft", input.notes ?? null, timestamp, timestamp);
        planId = Number(result.lastInsertRowid);
      }
      const insertSession = this.database.prepare(`
        INSERT INTO planned_sessions (
          plan_id, session_date, sport, title, duration_minutes, target_tss,
          intensity, description, sort_order
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
      `);
      input.sessions.forEach((session, index) => insertSession.run(
        planId!, session.date, session.sport, session.title, session.durationMinutes ?? null,
        session.targetTss ?? null, session.intensity ?? null, session.description, index,
      ));
      this.database.exec("COMMIT");
      return this.getPlan(planId!);
    } catch (error) {
      this.database.exec("ROLLBACK");
      throw error;
    }
  }

  listPlans(status?: string): Row {
    const plans = status
      ? this.database.prepare(`
          SELECT p.*, COUNT(s.id) AS session_count
          FROM training_plans p LEFT JOIN planned_sessions s ON s.plan_id = p.id
          WHERE p.status = ? GROUP BY p.id ORDER BY p.updated_at DESC
        `).all(status)
      : this.database.prepare(`
          SELECT p.*, COUNT(s.id) AS session_count
          FROM training_plans p LEFT JOIN planned_sessions s ON s.plan_id = p.id
          GROUP BY p.id ORDER BY p.updated_at DESC
        `).all();
    return { plans: plans.map((row) => normalizedRow(row as Row)) };
  }

  getPlan(planId: number): Row {
    const plan = this.database.prepare("SELECT * FROM training_plans WHERE id = ?").get(planId) as Row | undefined;
    if (!plan) throw new Error(`No training plan has ID ${planId}`);
    const sessions = this.database.prepare(`
      SELECT id, session_date AS date, sport, title, duration_minutes, target_tss, intensity, description
      FROM planned_sessions WHERE plan_id = ? ORDER BY sort_order
    `).all(planId).map((row) => normalizedRow(row as Row));
    return { plan: normalizedRow(plan), sessions };
  }
}
