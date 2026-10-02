import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { opendir, readFile, rm } from "node:fs/promises";
import { extname, join, relative, sep } from "node:path";
import { parse } from "csv-parse";
import type { TrainingPeaksDatabase, ImportCounts } from "./database.js";
import type { ImportContext, WorkoutRow } from "./types.js";

async function listFiles(root: string, directory = root): Promise<string[]> {
  const output: string[] = [];
  const entries = await opendir(directory);
  for await (const entry of entries) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) output.push(...await listFiles(root, path));
    else if (entry.isFile()) output.push(path);
  }
  return output.sort((left, right) => left.localeCompare(right));
}

function portableRelative(root: string, filePath: string): string {
  return relative(root, filePath).split(sep).join("/");
}

function isObject(value: unknown): value is WorkoutRow {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

async function importCsv(
  database: TrainingPeaksDatabase,
  context: ImportContext,
  filePath: string,
  sourceFileId: number,
): Promise<number> {
  const parser = createReadStream(filePath).pipe(parse({
    bom: true,
    columns: true,
    skip_empty_lines: true,
    relax_column_count: true,
    relax_quotes: true,
    trim: false,
  }));

  let rowNumber = 1;
  let count = 0;
  for await (const value of parser) {
    rowNumber += 1;
    if (!isObject(value)) continue;
    if (Object.values(value).every((field) => String(field ?? "").trim() === "")) continue;
    database.storeWorkout(context.athleteId, context.exportId, sourceFileId, rowNumber, value);
    count += 1;
  }
  return count;
}

async function importJson(
  database: TrainingPeaksDatabase,
  context: ImportContext,
  filePath: string,
  sourceFileId: number,
): Promise<number> {
  const parsed: unknown = JSON.parse(await readFile(filePath, "utf8"));
  let candidates: unknown[] | null = Array.isArray(parsed) ? parsed : null;
  if (isObject(parsed)) {
    for (const key of ["workouts", "items", "data", "results"]) {
      if (Array.isArray(parsed[key])) {
        candidates = parsed[key];
        break;
      }
    }
  }
  if (!candidates || !candidates.every(isObject)) return 0;

  let count = 0;
  for (const [index, row] of candidates.entries()) {
    if (!isObject(row)) continue;
    database.storeWorkout(context.athleteId, context.exportId, sourceFileId, index + 1, row);
    count += 1;
  }
  return count;
}

export async function sha256File(filePath: string): Promise<string> {
  const hash = createHash("sha256");
  for await (const chunk of createReadStream(filePath)) hash.update(chunk as Buffer);
  return hash.digest("hex");
}

export async function importExtracted(
  database: TrainingPeaksDatabase,
  context: ImportContext,
): Promise<ImportCounts> {
  const paths = await listFiles(context.extractionRoot);
  const stored = new Map<string, number>();
  let workouts = 0;

  database.begin();
  try {
    database.resetExportLinks(context.exportId);
    for (const filePath of paths) {
      const relativePath = portableRelative(context.extractionRoot, filePath);
      const file = database.storeFile(context.athleteId, context.exportId, relativePath, filePath);
      stored.set(filePath, file.id);
    }

    if (context.exportKind === "workouts") {
      for (const filePath of paths) {
        const sourceFileId = stored.get(filePath);
        if (!sourceFileId) throw new Error(`Internal error: file was not stored: ${filePath}`);
        const extension = extname(filePath).toLowerCase();
        if (extension === ".csv") {
          workouts += await importCsv(database, context, filePath, sourceFileId);
        } else if (extension === ".json") {
          workouts += await importJson(database, context, filePath, sourceFileId);
        }
      }
    }
    database.commit();
  } catch (error) {
    database.rollback();
    throw error;
  }

  return { files: paths.length, workouts };
}

export async function removeExtraction(path: string): Promise<void> {
  await rm(path, { recursive: true, force: true });
}
