import { createWriteStream } from "node:fs";
import { mkdir, open, rename, rm, stat, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { Readable } from "node:stream";
import { pipeline } from "node:stream/promises";
import { verifyZip } from "./archive.js";
import type { Config, DateWindow, ExportKind } from "./types.js";

export interface DownloadResult {
  downloadPath: string | null;
  format: "zip" | "single" | "empty";
  requestUrl: string;
  cached: boolean;
}

export class HttpError extends Error {
  constructor(
    readonly status: number,
    message: string,
  ) {
    super(message);
    this.name = "HttpError";
  }
}

class RetryableError extends Error {}

function delay(milliseconds: number): Promise<void> {
  return new Promise((resolvePromise) => setTimeout(resolvePromise, milliseconds));
}

function retryDelay(response: Response | undefined, attempt: number): number {
  const retryAfter = response?.headers.get("retry-after");
  if (retryAfter) {
    const seconds = Number(retryAfter);
    if (Number.isFinite(seconds)) return Math.min(60_000, Math.max(1_000, seconds * 1_000));
    const at = Date.parse(retryAfter);
    if (!Number.isNaN(at)) return Math.min(60_000, Math.max(1_000, at - Date.now()));
  }
  return Math.min(30_000, 1_000 * 2 ** (attempt - 1));
}

function isTrustedApiHost(url: URL): boolean {
  const hostname = url.hostname.toLowerCase();
  return url.protocol === "https:" && (!url.port || url.port === "443") &&
    (hostname === "trainingpeaks.com" || hostname.endsWith(".trainingpeaks.com"));
}

function downloadUrlFromJson(value: unknown): string | null {
  if (typeof value === "string") {
    return /^https?:\/\//i.test(value) ? value : null;
  }
  if (!value || typeof value !== "object") return null;

  const object = value as Record<string, unknown>;
  for (const key of ["downloadUrl", "downloadURL", "fileUrl", "fileURL", "signedUrl", "url", "href", "location"]) {
    if (typeof object[key] === "string") return object[key];
  }
  for (const key of ["data", "result", "export", "file"]) {
    const nested = downloadUrlFromJson(object[key]);
    if (nested) return nested;
  }
  return null;
}

function base64FileFromJson(value: unknown): Buffer | null {
  if (!value || typeof value !== "object") return null;
  const object = value as Record<string, unknown>;
  const encoded = object.data;
  const fileName = typeof object.fileName === "string" ? object.fileName.toLowerCase() : "";
  const contentType = typeof object.contentType === "string" ? object.contentType.toLowerCase() : "";
  if (
    typeof encoded !== "string" ||
    !(fileName.endsWith(".zip") || contentType.includes("zip"))
  ) return null;

  const decoded = Buffer.from(encoded, "base64");
  if (decoded.length === 0) throw new Error("Export API returned an empty base64 file");
  return decoded;
}

async function fetchToFile(
  url: URL,
  destination: string,
  token: string,
  depth = 0,
): Promise<"download" | "empty"> {
  if (depth > 4) throw new Error("Too many download indirections from the export endpoint");
  if (url.protocol !== "https:" && !(url.protocol === "http:" && ["127.0.0.1", "localhost", "[::1]"].includes(url.hostname))) {
    throw new Error("Export downloads require HTTPS");
  }

  let lastError: unknown;
  for (let attempt = 1; attempt <= 5; attempt += 1) {
    let response: Response | undefined;
    try {
      const headers = new Headers({
        Accept: "application/zip, application/octet-stream, application/json",
        "Content-Type": "application/json",
        Referer: "https://app.trainingpeaks.com/",
        "User-Agent": "trainingpeaks-export-importer/1.0",
      });
      if (isTrustedApiHost(url)) headers.set("Authorization", `Bearer ${token}`);

      response = await fetch(url, {
        method: "GET",
        headers,
        redirect: "manual",
        signal: AbortSignal.timeout(10 * 60_000),
      });

      if ([301, 302, 303, 307, 308].includes(response.status)) {
        const location = response.headers.get("location");
        if (!location) throw new HttpError(response.status, "Export redirect has no location");
        await response.body?.cancel();
        return fetchToFile(new URL(location, url), destination, token, depth + 1);
      }

      if (response.status === 204) return "empty";
      if (response.status === 408 || response.status === 429 || response.status >= 500) {
        await response.body?.cancel();
        throw new RetryableError(`HTTP ${response.status}`);
      }
      if (!response.ok) {
        await response.body?.cancel();
        throw new HttpError(response.status, `HTTP ${response.status}`);
      }

      const contentType = response.headers.get("content-type")?.toLowerCase() ?? "";
      if (contentType.includes("json")) {
        const payload: unknown = await response.json();
        const embeddedFile = base64FileFromJson(payload);
        if (embeddedFile) {
          await writeFile(destination, embeddedFile);
          return "download";
        }
        const nextUrl = downloadUrlFromJson(payload);
        if (!nextUrl) {
          throw new Error("Export API returned JSON without a download URL");
        }
        return fetchToFile(new URL(nextUrl, response.url), destination, token, depth + 1);
      }

      if (!response.body) throw new Error("Export response had no body");
      await pipeline(
        Readable.from(response.body as unknown as AsyncIterable<Uint8Array>),
        createWriteStream(destination, { flags: "w" }),
      );
      return "download";
    } catch (error) {
      if (error instanceof HttpError) throw error;
      lastError = error;
      if (attempt === 5) break;
      await delay(retryDelay(response, attempt));
    }
  }

  throw new Error(`Download failed after 5 attempts: ${lastError instanceof Error ? lastError.message : String(lastError)}`);
}

export function exportUrl(config: Config, kind: ExportKind, window: DateWindow): string {
  return `${config.apiBaseUrl}/${encodeURIComponent(config.athleteId)}/${kind}/${window.start}/${window.end}`;
}

async function isZip(filePath: string): Promise<boolean> {
  const handle = await open(filePath, "r");
  try {
    const signature = Buffer.alloc(4);
    const { bytesRead } = await handle.read(signature, 0, 4, 0);
    return bytesRead === 4 && signature[0] === 0x50 && signature[1] === 0x4b &&
      ((signature[2] === 0x03 && signature[3] === 0x04) ||
       (signature[2] === 0x05 && signature[3] === 0x06) ||
       (signature[2] === 0x07 && signature[3] === 0x08));
  } finally {
    await handle.close();
  }
}

export async function downloadExport(
  config: Config,
  kind: ExportKind,
  window: DateWindow,
): Promise<DownloadResult> {
  const requestUrl = exportUrl(config, kind, window);
  const downloadDirectory = join(config.workDirectory, "downloads");
  await mkdir(downloadDirectory, { recursive: true });
  const stem = join(downloadDirectory, `${kind}-${window.start}-${window.end}`);
  const archivePath = `${stem}.zip`;
  const singlePath = `${stem}.${kind === "workouts" ? "csv" : "bin"}`;

  if (!config.forceDownload) {
    for (const candidate of [archivePath, singlePath]) {
      try {
        const details = await stat(candidate);
        if (details.size > 0) {
          if (candidate === archivePath) await verifyZip(candidate);
          return {
            downloadPath: candidate,
            format: candidate === archivePath ? "zip" : "single",
            requestUrl,
            cached: true,
          };
        }
      } catch {
        // Missing or incomplete cache entries are downloaded again.
      }
    }
  }

  const temporaryPath = `${stem}.part`;
  await rm(temporaryPath, { force: true });
  const result = await fetchToFile(new URL(requestUrl), temporaryPath, config.token);
  if (result === "empty") {
    await rm(temporaryPath, { force: true });
    return { downloadPath: null, format: "empty", requestUrl, cached: false };
  }

  try {
    if (await isZip(temporaryPath)) {
      await verifyZip(temporaryPath);
      await rm(archivePath, { force: true });
      await rename(temporaryPath, archivePath);
      return { downloadPath: archivePath, format: "zip", requestUrl, cached: false };
    }

    // TrainingPeaks documents the workout-summary export as a direct CSV. The
    // files export is normally a ZIP, but preserving a non-ZIP response as a
    // binary source file is preferable to silently discarding it.
    await rm(singlePath, { force: true });
    await rename(temporaryPath, singlePath);
    return { downloadPath: singlePath, format: "single", requestUrl, cached: false };
  } catch (error) {
    await rm(temporaryPath, { force: true });
    throw error;
  }
}
