#!/usr/bin/env node
import { existsSync } from "node:fs";
import { resolve } from "node:path";
import { Client } from "@modelcontextprotocol/client";
import { StdioClientTransport } from "@modelcontextprotocol/client/stdio";

const databasePath = resolve(process.env.TRAININGPEAKS_DATABASE ?? "trainingpeaks.sqlite");
const builtServer = resolve("dist/src/mcp/server.js");
const transport = existsSync(builtServer)
  ? new StdioClientTransport({
      command: process.execPath,
      args: [builtServer, "--database", databasePath],
      cwd: process.cwd(),
      stderr: "pipe",
    })
  : new StdioClientTransport({
      command: resolve("node_modules/.bin/tsx"),
      args: [resolve("src/mcp/server.ts"), "--database", databasePath],
      cwd: process.cwd(),
      stderr: "pipe",
    });

const client = new Client({ name: "trainingpeaks-smoke-test", version: "1.0.0" });

try {
  await client.connect(transport);
  const tools = await client.listTools();
  const prompts = await client.listPrompts();
  const resources = await client.listResources();
  const overview = await client.callTool({ name: "get_database_overview", arguments: {} });
  const summary = await client.callTool({
    name: "summarize_training",
    arguments: { startDate: "2026-06-17", endDate: "2026-09-17", groupBy: "type" },
  });
  const planning = await client.callTool({
    name: "get_planning_context",
    arguments: { asOfDate: "2026-09-17", lookbackWeeks: 8 },
  });
  const search = await client.callTool({
    name: "search_workouts",
    arguments: { query: "felt strong", limit: 5 },
  });
  const searchData = search.structuredContent as { workouts?: Array<{ id: number }> } | undefined;
  const firstWorkoutId = searchData?.workouts?.[0]?.id;
  const workout = firstWorkoutId
    ? await client.callTool({ name: "get_workout", arguments: { workoutId: firstWorkoutId } })
    : undefined;
  const workoutData = workout?.structuredContent as { activityFiles?: Array<{ file_id: number }> } | undefined;
  const firstFileId = workoutData?.activityFiles?.[0]?.file_id;
  const activity = firstFileId
    ? await client.callTool({ name: "get_activity_detail", arguments: { fileId: firstFileId, maximumSamples: 3 } })
    : undefined;
  const planningData = planning.structuredContent as { weekly?: unknown[]; sportBalance?: unknown[] } | undefined;
  const summaryData = summary.structuredContent as { groups?: unknown[] } | undefined;
  const activityData = activity?.structuredContent as { totalRecordCount?: number; totalLapCount?: number } | undefined;
  process.stdout.write(`${JSON.stringify({
    toolNames: tools.tools.map((tool) => tool.name),
    promptNames: prompts.prompts.map((prompt) => prompt.name),
    resourceUris: resources.resources.map((resource) => resource.uri),
    overview: overview.structuredContent,
    summaryGroupCount: summaryData?.groups?.length ?? 0,
    planningWeekCount: planningData?.weekly?.length ?? 0,
    planningSportCount: planningData?.sportBalance?.length ?? 0,
    narrativeSearchResults: searchData?.workouts?.length ?? 0,
    fetchedWorkoutId: firstWorkoutId ?? null,
    decodedActivity: activityData ? {
      totalRecordCount: activityData.totalRecordCount ?? 0,
      totalLapCount: activityData.totalLapCount ?? 0,
    } : null,
  }, null, 2)}\n`);
} finally {
  await client.close();
}
