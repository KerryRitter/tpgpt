import assert from "node:assert/strict";
import { createServer } from "node:http";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { after, before, describe, it } from "node:test";
import type { AddressInfo } from "node:net";
import { downloadExport } from "../src/downloader.js";
import type { Config } from "../src/types.js";

describe("export downloader", () => {
  const csv = "WorkoutId,WorkoutDay,WorkoutTitle\n1001,2026-01-02,Easy Run\n";
  const server = createServer((request, response) => {
    assert.equal(request.headers.authorization, undefined, "tokens must not reach an unrelated download host");
    requestCount += 1;
    response.writeHead(200, { "Content-Type": "text/csv" });
    response.end(csv);
  });
  let root = "";
  let requestCount = 0;
  let apiBaseUrl = "";

  before(async () => {
    root = await mkdtemp(join(tmpdir(), "trainingpeaks-download-test-"));
    await new Promise<void>((resolvePromise) => server.listen(0, "127.0.0.1", resolvePromise));
    const address = server.address() as AddressInfo;
    apiBaseUrl = `http://127.0.0.1:${address.port}/fitness/v1/export`;
  });

  after(async () => {
    await new Promise<void>((resolvePromise, reject) => {
      server.close((error) => error ? reject(error) : resolvePromise());
    });
    await rm(root, { recursive: true, force: true });
  });

  it("accepts and caches a direct workout CSV response", async () => {
    const config: Config = {
      athleteId: "123456",
      token: "test-token",
      databasePath: join(root, "database.sqlite"),
      workDirectory: join(root, "work"),
      endDate: "2026-09-17",
      years: 5,
      monthsPerRequest: 3,
      forceDownload: false,
      keepExtracted: false,
      apiBaseUrl,
    };
    const window = { start: "2026-06-17", end: "2026-09-17" };
    const first = await downloadExport(config, "workouts", window);
    const second = await downloadExport(config, "workouts", window);

    assert.equal(first.format, "single");
    assert.equal(first.cached, false);
    assert.equal(second.cached, true);
    assert.equal(requestCount, 1);
    assert.equal(await readFile(first.downloadPath!, "utf8"), csv);
  });
});

describe("embedded export downloader", () => {
  const zip = Buffer.from("UEsFBgAAAAAAAAAAAAAAAAAAAAAAAA==", "base64");
  const server = createServer((_request, response) => {
    response.writeHead(200, { "Content-Type": "application/json" });
    response.end(JSON.stringify({
      fileName: "WorkoutExport.zip",
      contentType: "application/zip",
      data: zip.toString("base64"),
    }));
  });
  let root = "";
  let apiBaseUrl = "";

  before(async () => {
    root = await mkdtemp(join(tmpdir(), "trainingpeaks-embedded-test-"));
    await new Promise<void>((resolvePromise) => server.listen(0, "127.0.0.1", resolvePromise));
    const address = server.address() as AddressInfo;
    apiBaseUrl = `http://127.0.0.1:${address.port}/fitness/v1/export`;
  });

  after(async () => {
    await new Promise<void>((resolvePromise, reject) => {
      server.close((error) => error ? reject(error) : resolvePromise());
    });
    await rm(root, { recursive: true, force: true });
  });

  it("decodes a base64 ZIP returned inside JSON", async () => {
    const config: Config = {
      athleteId: "123456",
      token: "test-token",
      databasePath: join(root, "database.sqlite"),
      workDirectory: join(root, "work"),
      endDate: "2026-09-17",
      years: 5,
      monthsPerRequest: 3,
      forceDownload: false,
      keepExtracted: false,
      apiBaseUrl,
    };
    const result = await downloadExport(config, "workouts", { start: "2026-06-17", end: "2026-09-17" });
    assert.equal(result.format, "zip");
    assert.deepEqual(await readFile(result.downloadPath!), zip);
  });
});
