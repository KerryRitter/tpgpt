#!/usr/bin/env node
import { resolve } from "node:path";
import { McpServer } from "@modelcontextprotocol/server";
import { serveStdio } from "@modelcontextprotocol/server/stdio";
import * as z from "zod/v4";
import { TrainingDataStore } from "./store.js";

const isoDate = z.string().regex(/^\d{4}-\d{2}-\d{2}$/, "Use YYYY-MM-DD");
const workoutTypes = z.array(z.string().min(1)).max(20).optional();

function result(data: Record<string, unknown>) {
  return {
    content: [{ type: "text" as const, text: JSON.stringify(data, null, 2) }],
    structuredContent: data,
  };
}

function databasePathFromArgs(argv: string[]): string {
  let databasePath = process.env.TRAININGPEAKS_DATABASE ?? "trainingpeaks.sqlite";
  for (let index = 0; index < argv.length; index += 1) {
    if (argv[index] === "--database" && argv[index + 1]) {
      databasePath = argv[index + 1]!;
      index += 1;
    }
  }
  return resolve(databasePath);
}

export function createTrainingPeaksServer(databasePath: string): McpServer {
  const store = new TrainingDataStore(databasePath);
  const server = new McpServer(
    { name: "trainingpeaks-local", version: "1.0.0" },
    {
      instructions: [
        "This server contains the athlete's private TrainingPeaks history and locally saved training plans.",
        "Use exact analytics tools for totals, trends, comparisons, records, and training load; do not estimate values that tools can calculate.",
        "Use search_workouts for narrative recall across titles, descriptions, athlete comments, and coach comments.",
        "Before designing or revising a plan, call get_planning_context and draft_plan_framework, then ask about goals, current health/injury constraints, availability, equipment, preferences, and event date when missing.",
        "Treat generated plans as general training guidance rather than diagnosis or treatment. Recommend qualified medical guidance for pain, injury, illness, or other clinical concerns.",
        "Only call save_training_plan after the user asks to save or approves the proposed plan.",
      ].join(" "),
    },
  );

  server.registerTool(
    "get_database_overview",
    {
      title: "Training database overview",
      description: "Get workout coverage, counts, available workout types, FIT indexing status, and saved-plan count. Use this to orient a new conversation.",
      inputSchema: z.object({}),
      annotations: { readOnlyHint: true },
    },
    async () => result(store.overview()),
  );

  server.registerTool(
    "search_workouts",
    {
      title: "Search workouts",
      description: "Find workouts by words or themes in titles, descriptions, athlete comments, and coach comments, with optional date/type filters. Also use without query to list recent workouts.",
      inputSchema: z.object({
        query: z.string().max(500).optional(),
        startDate: isoDate.optional(),
        endDate: isoDate.optional(),
        workoutTypes,
        limit: z.number().int().min(1).max(100).default(20),
        offset: z.number().int().min(0).default(0),
      }),
      annotations: { readOnlyHint: true },
    },
    async (input) => result(store.searchWorkouts(input)),
  );

  server.registerTool(
    "get_workout",
    {
      title: "Get complete workout",
      description: "Retrieve one workout's complete exported fields, comments, metrics, and linked FIT activity files. Use an ID returned by search_workouts or a records tool.",
      inputSchema: z.object({ workoutId: z.number().int().positive() }),
      annotations: { readOnlyHint: true },
    },
    async ({ workoutId }) => result(store.getWorkout(workoutId)),
  );

  server.registerTool(
    "summarize_training",
    {
      title: "Summarize training",
      description: "Calculate exact workout counts, completed volume, planned volume, distance, TSS, intensity, heart rate, power, and RPE for a date range, optionally grouped by week, month, or workout type.",
      inputSchema: z.object({
        startDate: isoDate.optional(),
        endDate: isoDate.optional(),
        workoutTypes,
        groupBy: z.enum(["none", "week", "month", "type"]).default("none"),
      }),
      annotations: { readOnlyHint: true },
    },
    async (input) => result(store.summarize(input)),
  );

  server.registerTool(
    "compare_training_periods",
    {
      title: "Compare training periods",
      description: "Calculate exact side-by-side summaries for two periods. Use for year-over-year, block-to-block, before/after, or recent-vs-baseline comparisons.",
      inputSchema: z.object({
        periodA: z.object({ label: z.string().min(1), startDate: isoDate, endDate: isoDate }),
        periodB: z.object({ label: z.string().min(1), startDate: isoDate, endDate: isoDate }),
        workoutTypes,
      }),
      annotations: { readOnlyHint: true },
    },
    async ({ periodA, periodB, workoutTypes: types }) => result({
      periodA: {
        label: periodA.label,
        ...store.summarize({ startDate: periodA.startDate, endDate: periodA.endDate, workoutTypes: types, groupBy: "none" }),
      },
      periodB: {
        label: periodB.label,
        ...store.summarize({ startDate: periodB.startDate, endDate: periodB.endDate, workoutTypes: types, groupBy: "none" }),
      },
    }),
  );

  server.registerTool(
    "get_training_load",
    {
      title: "Get training load",
      description: "Calculate daily TSS, 7-day acute load, 42-day chronic load, form, and recent chronic-load ramp. Useful for fatigue/context, not medical diagnosis.",
      inputSchema: z.object({
        endDate: isoDate,
        days: z.number().int().min(7).max(365).default(84),
      }),
      annotations: { readOnlyHint: true },
    },
    async ({ endDate, days }) => result(store.trainingLoad(endDate, days)),
  );

  server.registerTool(
    "get_personal_records",
    {
      title: "Find personal records",
      description: "Rank workouts by distance, duration, TSS, power, or heart rate with optional sport/date filters.",
      inputSchema: z.object({
        metric: z.enum(["distance", "duration", "tss", "average_power", "max_power", "average_heart_rate"]),
        workoutType: z.string().optional(),
        startDate: isoDate.optional(),
        endDate: isoDate.optional(),
        limit: z.number().int().min(1).max(50).default(10),
      }),
      annotations: { readOnlyHint: true },
    },
    async (input) => result(store.personalRecords(input)),
  );

  server.registerTool(
    "get_activity_detail",
    {
      title: "Decode FIT activity detail",
      description: "Decode a stored FIT file on demand and return sessions, laps, and evenly sampled time-series records. Obtain fileId from get_workout. Set maximumSamples to 0 when only sessions/laps are needed.",
      inputSchema: z.object({
        fileId: z.number().int().positive(),
        maximumSamples: z.number().int().min(0).max(500).default(100),
      }),
      annotations: { readOnlyHint: true },
    },
    async ({ fileId, maximumSamples }) => result(store.getActivityDetail(fileId, maximumSamples)),
  );

  server.registerTool(
    "get_planning_context",
    {
      title: "Get training-plan context",
      description: "Get recent weekly volume, sport balance, training load, athlete comments, and planning guardrails. Always call this before creating or substantially revising a workout plan.",
      inputSchema: z.object({
        asOfDate: isoDate,
        lookbackWeeks: z.number().int().min(4).max(26).default(8),
      }),
      annotations: { readOnlyHint: true },
    },
    async ({ asOfDate, lookbackWeeks }) => result(store.planningContext(asOfDate, lookbackWeeks)),
  );

  server.registerTool(
    "draft_plan_framework",
    {
      title: "Draft plan load framework",
      description: "Create a conservative week-by-week phase, hours, and TSS scaffold using the athlete's previous six weeks. Call after get_planning_context, then use the model to design individual sessions around goals and constraints.",
      inputSchema: z.object({
        goal: z.string().min(1).max(1000),
        primarySport: z.string().min(1).max(100),
        startDate: isoDate,
        endDate: isoDate,
        daysPerWeek: z.number().int().min(1).max(7),
        targetWeeklyHours: z.number().positive().max(40).optional(),
      }),
      annotations: { readOnlyHint: true },
    },
    async (input) => result(store.planFramework(input)),
  );

  const planSession = z.object({
    date: isoDate,
    sport: z.string().min(1).max(100),
    title: z.string().min(1).max(300),
    durationMinutes: z.number().nonnegative().max(1440).optional(),
    targetTss: z.number().nonnegative().max(1000).optional(),
    intensity: z.string().max(100).optional(),
    description: z.string().min(1).max(10_000),
  });
  server.registerTool(
    "save_training_plan",
    {
      title: "Save training plan",
      description: "Save a user-approved plan locally, or replace the sessions of an existing plan when planId is supplied. Do not call until the user asks to save or approves the plan.",
      inputSchema: z.object({
        planId: z.number().int().positive().optional(),
        name: z.string().min(1).max(300),
        goal: z.string().min(1).max(2000),
        startDate: isoDate,
        endDate: isoDate,
        status: z.enum(["draft", "active", "completed", "archived"]).default("draft"),
        notes: z.string().max(10_000).optional(),
        sessions: z.array(planSession).max(500),
      }),
      annotations: { readOnlyHint: false, destructiveHint: true },
    },
    async (input) => result(store.savePlan(input)),
  );

  server.registerTool(
    "list_training_plans",
    {
      title: "List saved training plans",
      description: "List locally saved plans and session counts, optionally filtered by status.",
      inputSchema: z.object({
        status: z.enum(["draft", "active", "completed", "archived"]).optional(),
      }),
      annotations: { readOnlyHint: true },
    },
    async ({ status }) => result(store.listPlans(status)),
  );

  server.registerTool(
    "get_training_plan",
    {
      title: "Get saved training plan",
      description: "Retrieve a saved plan and all its sessions.",
      inputSchema: z.object({ planId: z.number().int().positive() }),
      annotations: { readOnlyHint: true },
    },
    async ({ planId }) => result(store.getPlan(planId)),
  );

  server.registerResource(
    "training-database-overview",
    "trainingpeaks://overview",
    { title: "TrainingPeaks database overview", mimeType: "application/json" },
    async (uri) => ({ contents: [{ uri: uri.href, mimeType: "application/json", text: JSON.stringify(store.overview(), null, 2) }] }),
  );

  server.registerResource(
    "training-data-guide",
    "trainingpeaks://guide",
    { title: "Training data and planning guide", mimeType: "text/markdown" },
    async (uri) => ({
      contents: [{
        uri: uri.href,
        mimeType: "text/markdown",
        text: [
          "# Training data guide",
          "",
          "Workout summary metrics come from TrainingPeaks exports. Distances are stored in meters and durations in hours; summary tools return kilometers and hours.",
          "Narrative search covers titles, descriptions, athlete comments, and coach comments using SQLite FTS5.",
          "FIT files remain byte-for-byte in SQLite. Indexed session/lap metadata supports discovery, and get_activity_detail decodes samples on demand.",
          "For plans: obtain planning context, clarify the athlete's goal and constraints, create a load framework, draft sessions, seek approval, then save only if requested.",
        ].join("\n"),
      }],
    }),
  );

  server.registerPrompt(
    "build-training-plan",
    {
      title: "Build a personalized training plan",
      description: "Analyze TrainingPeaks history and create a safe, evidence-aware plan that can optionally be saved.",
      argsSchema: z.object({
        goal: z.string().min(1),
        eventDate: isoDate.optional(),
        primarySport: z.string().optional(),
        availability: z.string().optional(),
        constraints: z.string().optional(),
      }),
    },
    ({ goal, eventDate, primarySport, availability, constraints }) => ({
      messages: [{
        role: "user" as const,
        content: {
          type: "text" as const,
          text: [
            `Build me a personalized training plan for: ${goal}.`,
            eventDate ? `Target/event date: ${eventDate}.` : "Ask me for a target date if the goal is time-bound.",
            primarySport ? `Primary sport: ${primarySport}.` : "Infer likely sports from history, then confirm the primary focus.",
            availability ? `Availability: ${availability}.` : "Ask which days and weekly hours are available.",
            constraints ? `Constraints: ${constraints}.` : "Ask about injury/illness constraints, equipment, preferences, and a rest day.",
            "Call get_planning_context and draft_plan_framework before drafting sessions. Use exact history, explain assumptions, include recovery and adaptation rules, and do not save until I approve.",
          ].join("\n"),
        },
      }],
    }),
  );

  server.registerPrompt(
    "review-my-training",
    {
      title: "Review recent training",
      description: "Review a date range for volume, load, consistency, notable comments, and actionable patterns.",
      argsSchema: z.object({ startDate: isoDate, endDate: isoDate, focus: z.string().optional() }),
    },
    ({ startDate, endDate, focus }) => ({
      messages: [{
        role: "user" as const,
        content: {
          type: "text" as const,
          text: `Review my training from ${startDate} through ${endDate}.${focus ? ` Focus on ${focus}.` : ""} Use the MCP analytics and workout-search tools, cite specific workouts where useful, distinguish facts from interpretation, and suggest practical next steps.`,
        },
      }],
    }),
  );

  return server;
}

const databasePath = databasePathFromArgs(process.argv.slice(2));
serveStdio(() => createTrainingPeaksServer(databasePath), {
  onerror: (error) => process.stderr.write(`TrainingPeaks MCP error: ${error.message}\n`),
});
