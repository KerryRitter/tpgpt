import assert from "node:assert/strict";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, it } from "node:test";
import { TrainingPeaksDatabase } from "../src/database.js";
import { TrainingDataStore } from "../src/mcp/store.js";

describe("MCP training data store", () => {
  it("searches, summarizes, scaffolds, and saves plans", async () => {
    const root = await mkdtemp(join(tmpdir(), "trainingpeaks-mcp-test-"));
    const databasePath = join(root, "training.sqlite");
    const sourcePath = join(root, "workouts.csv");
    await writeFile(sourcePath, "fixture");

    const imported = new TrainingPeaksDatabase(databasePath);
    const exportId = imported.startExport({
      athleteId: "1",
      kind: "workouts",
      window: { start: "2026-01-01", end: "2026-04-01" },
    }, "https://example.test/export");
    imported.begin();
    const source = imported.storeFile("1", exportId, "workouts.csv", sourcePath);
    imported.storeWorkout("1", exportId, source.id, 2, {
      WorkoutDay: "2026-01-02",
      WorkoutTitle: "Strong tempo run",
      WorkoutType: "Run",
      WorkoutDescription: "Controlled threshold intervals",
      AthleteComments: "Felt strong and smooth throughout",
      TimeTotalInHours: "1.0",
      DistanceInMeters: "10000",
      TSS: "75",
      IF: "0.85",
    });
    imported.storeWorkout("1", exportId, source.id, 3, {
      WorkoutDay: "2026-01-04",
      WorkoutTitle: "Easy spin",
      WorkoutType: "Bike",
      TimeTotalInHours: "2.0",
      DistanceInMeters: "50000",
      TSS: "80",
      IF: "0.6",
    });
    imported.commit();
    imported.close();

    const store = new TrainingDataStore(databasePath);
    try {
      assert.equal(store.overview().workouts, 2);
      const search = store.searchWorkouts({ query: "felt strong" });
      assert.equal(search.count, 1);

      const summary = store.summarize({
        startDate: "2026-01-01",
        endDate: "2026-01-31",
        groupBy: "type",
      });
      const groups = summary.groups as Array<Record<string, unknown>>;
      assert.equal(groups.length, 2);
      assert.equal(groups.find((group) => group.period === "Run")?.distance_km, 10);

      const framework = store.planFramework({
        goal: "Spring 10K",
        primarySport: "Run",
        startDate: "2026-02-01",
        endDate: "2026-03-31",
        daysPerWeek: 4,
      });
      assert.ok((framework.weeks as unknown[]).length >= 8);

      const saved = store.savePlan({
        name: "Spring 10K draft",
        goal: "Run a strong 10K",
        startDate: "2026-02-01",
        endDate: "2026-03-31",
        sessions: [{
          date: "2026-02-02",
          sport: "Run",
          title: "Easy run",
          durationMinutes: 40,
          intensity: "easy",
          description: "Conversational aerobic running.",
        }],
      });
      assert.equal((saved.sessions as unknown[]).length, 1);
      assert.equal((store.listPlans().plans as unknown[]).length, 1);
    } finally {
      store.close();
      await rm(root, { recursive: true, force: true });
    }
  });
});
