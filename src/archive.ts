import { createWriteStream } from "node:fs";
import { copyFile, mkdir, open, rm } from "node:fs/promises";
import { basename, dirname, extname, isAbsolute, resolve, sep } from "node:path";
import { pipeline } from "node:stream/promises";
import yauzl, { type Entry, type ZipFile } from "yauzl";

function openZip(archivePath: string): Promise<ZipFile> {
  return new Promise((resolvePromise, reject) => {
    yauzl.open(archivePath, { lazyEntries: true, autoClose: true }, (error, zipFile) => {
      if (error) reject(error);
      else if (!zipFile) reject(new Error(`Could not open ZIP archive: ${archivePath}`));
      else resolvePromise(zipFile);
    });
  });
}

function safeRelativeName(entry: Entry): string {
  const name = entry.fileName.replace(/\\/g, "/");
  if (
    !name ||
    name.includes("\0") ||
    isAbsolute(name) ||
    /^[A-Za-z]:/.test(name) ||
    name.split("/").some((part) => part === "..")
  ) {
    throw new Error(`Unsafe path in ZIP archive: ${JSON.stringify(entry.fileName)}`);
  }

  const unixMode = (entry.externalFileAttributes >>> 16) & 0xffff;
  if ((unixMode & 0xf000) === 0xa000) {
    throw new Error(`Symbolic links are not allowed in ZIP archives: ${JSON.stringify(entry.fileName)}`);
  }
  return name;
}

function entryStream(zipFile: ZipFile, entry: Entry): Promise<NodeJS.ReadableStream> {
  return new Promise((resolvePromise, reject) => {
    zipFile.openReadStream(entry, (error, stream) => {
      if (error) reject(error);
      else if (!stream) reject(new Error(`Could not read ZIP entry: ${entry.fileName}`));
      else resolvePromise(stream);
    });
  });
}

export async function verifyZip(archivePath: string): Promise<void> {
  const handle = await open(archivePath, "r");
  try {
    const signature = Buffer.alloc(4);
    const { bytesRead } = await handle.read(signature, 0, 4, 0);
    const valid = bytesRead === 4 && signature[0] === 0x50 && signature[1] === 0x4b &&
      ((signature[2] === 0x03 && signature[3] === 0x04) ||
       (signature[2] === 0x05 && signature[3] === 0x06) ||
       (signature[2] === 0x07 && signature[3] === 0x08));
    if (!valid) throw new Error(`Response is not a ZIP archive: ${archivePath}`);
  } finally {
    await handle.close();
  }
}

export async function extractZip(archivePath: string, destination: string): Promise<number> {
  await rm(destination, { recursive: true, force: true });
  await mkdir(destination, { recursive: true });
  const root = resolve(destination);
  const zipFile = await openZip(archivePath);

  return new Promise<number>((resolvePromise, reject) => {
    let extracted = 0;
    let settled = false;
    const allocatedFiles = new Set<string>();

    const uniqueFileName = (relativeName: string): string => {
      if (!allocatedFiles.has(relativeName)) {
        allocatedFiles.add(relativeName);
        return relativeName;
      }

      const extension = extname(relativeName);
      const stem = extension ? relativeName.slice(0, -extension.length) : relativeName;
      for (let duplicate = 2; ; duplicate += 1) {
        const candidate = `${stem}.duplicate-${duplicate}${extension}`;
        if (!allocatedFiles.has(candidate)) {
          allocatedFiles.add(candidate);
          return candidate;
        }
      }
    };

    const fail = (error: unknown): void => {
      if (settled) return;
      settled = true;
      zipFile.close();
      reject(error);
    };

    zipFile.on("error", fail);
    zipFile.on("end", () => {
      if (!settled) {
        settled = true;
        resolvePromise(extracted);
      }
    });
    zipFile.on("entry", (entry: Entry) => {
      void (async () => {
        const relativeName = safeRelativeName(entry);
        const outputName = relativeName.endsWith("/") ? relativeName : uniqueFileName(relativeName);
        const target = resolve(root, outputName);
        if (target !== root && !target.startsWith(`${root}${sep}`)) {
          throw new Error(`ZIP entry escapes extraction directory: ${JSON.stringify(entry.fileName)}`);
        }

        if (relativeName.endsWith("/")) {
          await mkdir(target, { recursive: true });
        } else {
          await mkdir(dirname(target), { recursive: true });
          const input = await entryStream(zipFile, entry);
          await pipeline(input, createWriteStream(target, { flags: "wx" }));
          extracted += 1;
        }
        zipFile.readEntry();
      })().catch(fail);
    });

    zipFile.readEntry();
  });
}

export async function stageSingleFile(filePath: string, destination: string): Promise<number> {
  await rm(destination, { recursive: true, force: true });
  await mkdir(destination, { recursive: true });
  await copyFile(filePath, resolve(destination, basename(filePath)));
  return 1;
}
