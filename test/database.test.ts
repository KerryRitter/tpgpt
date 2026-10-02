import assert from "node:assert/strict";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, it } from "node:test";
import { TrainingPeaksDatabase } from "../src/database.js";
import { importExtracted } from "../src/importer.js";

describe("SQLite import", () => {
  it("stores source bytes and upserts overlapping workout rows", async () => {
    const root = await mkdtemp(join(tmpdir(), "trainingpeaks-test-"));
    const extractionRoot = join(root, "extracted");
    await mkdir(extractionRoot);
    await writeFile(
      join(extractionRoot, "workouts.csv"),
      [
        "WorkoutId,WorkoutDay,WorkoutTitle,WorkoutType,Custom Field",
        "1001,2026-01-02,Easy Run,Run,alpha",
        "1002,2026-01-03,Endurance Ride,Bike,beta",
        "",
      ].join("\n"),
    );

    const database = new TrainingPeaksDatabase(join(root, "trainingpeaks.sqlite"));
    try {
      const identity = {
      athleteId: "123456",
        kind: "workouts" as const,
        window: { start: "2026-01-01", end: "2026-04-01" },
      };
      const exportId = database.startExport(identity, "https://example.test/export");
      const context = { ...identity, exportId, exportKind: identity.kind, extractionRoot };

      const first = await importExtracted(database, context);
      const second = await importExtracted(database, context);
      assert.deepEqual(first, { files: 1, workouts: 2 });
      assert.deepEqual(second, { files: 1, workouts: 2 });

      const workoutCount = database.database.prepare("SELECT count(*) AS count FROM workouts").get() as { count: number };
      const blobCount = database.database.prepare("SELECT count(*) AS count FROM file_blobs").get() as { count: number };
      const custom = database.database.prepare(`
        SELECT value_text FROM workout_fields WHERE field_name = 'Custom Field' ORDER BY value_text LIMIT 1
      `).get() as { value_text: string };
      assert.equal(workoutCount.count, 2);
      assert.equal(blobCount.count, 1);
      assert.equal(custom.value_text, "alpha");
    } finally {
      database.close();
      await rm(root, { recursive: true, force: true });
    }
  });
});
