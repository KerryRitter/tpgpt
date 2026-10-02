import { mkdir } from "node:fs/promises";
import { join } from "node:path";
import { createHash } from "node:crypto";
import { dateWindows } from "./dates.js";
import { extractZip, stageSingleFile } from "./archive.js";
import { TrainingPeaksDatabase } from "./database.js";
import { downloadExport, exportUrl, HttpError } from "./downloader.js";
import { importExtracted, removeExtraction, sha256File } from "./importer.js";
import { EXPORT_KINDS, type Config } from "./types.js";

export interface ExportProgress {
  phase: "download" | "complete" | "skip" | "error";
  label: string;
  done: number;
  total: number;
  message: string;
}

export async function runExports(config: Config, report: (event: ExportProgress) => void) {
  const windows = dateWindows(config.endDate, config.years, config.monthsPerRequest);
  await mkdir(config.workDirectory, { recursive: true });
  const database = new TrainingPeaksDatabase(config.databasePath);
  let completed = 0;
  let skipped = 0;
  let failed = 0;
  const total = windows.length * EXPORT_KINDS.length;
  const progress = (phase: ExportProgress["phase"], label: string, message: string) =>
    report({ phase, label, message, done: completed + skipped + failed, total });
  try {
    for (const window of windows) {
      for (const kind of EXPORT_KINDS) {
        const identity = { athleteId: config.athleteId, kind, window };
        const label = `${kind} ${window.start}..${window.end}`;
        if (!config.forceDownload && database.isComplete(identity)) {
          skipped += 1;
          progress("skip", label, "Already imported");
          continue;
        }
        const exportId = database.startExport(identity, exportUrl(config, kind, window));
        const extractionRoot = join(config.workDirectory, "extracted", `${kind}-${window.start}-${window.end}`);
        progress("download", label, "Downloading export");
        try {
          const download = await downloadExport(config, kind, window);
          if (!download.downloadPath) {
            database.begin();
            database.resetExportLinks(exportId);
            database.completeExport(exportId, createHash("sha256").update("").digest("hex"), { files: 0, workouts: 0 });
            database.commit();
          } else {
            const archiveHash = await sha256File(download.downloadPath);
            if (download.format === "zip") await extractZip(download.downloadPath, extractionRoot);
            else await stageSingleFile(download.downloadPath, extractionRoot);
            const counts = await importExtracted(database, {
              athleteId: config.athleteId, exportId, exportKind: kind, window, extractionRoot,
            });
            database.begin();
            database.completeExport(exportId, archiveHash, counts);
            database.commit();
            if (!config.keepExtracted) await removeExtraction(extractionRoot);
          }
          completed += 1;
          progress("complete", label, "Imported");
        } catch (error) {
          database.rollback();
          const message = (error instanceof Error ? error.message : String(error)).replaceAll(config.token, "[redacted]");
          database.failExport(exportId, new Error(message));
          failed += 1;
          progress("error", label, message);
          if (error instanceof HttpError && (error.status === 401 || error.status === 403)) {
            throw new Error(`AUTH_EXPIRED: TrainingPeaks returned HTTP ${error.status}. Sign in again.`);
          }
        }
      }
    }
  } finally {
    database.close();
  }
  return { completed, skipped, failed, total };
}
