import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { formatIsoDate, assertIsoDate } from "./dates.js";
import type { Config } from "./types.js";

interface ParsedArguments {
  athleteId?: string;
  tokenFile?: string;
  databasePath?: string;
  workDirectory?: string;
  endDate?: string;
  years?: number;
  monthsPerRequest?: number;
  forceDownload: boolean;
  keepExtracted: boolean;
  apiBaseUrl?: string;
  help: boolean;
}

export const HELP = `TrainingPeaks five-year export importer

Usage:
  npm start -- --athlete-id <id> [options]

Authentication:
  Set TRAININGPEAKS_TOKEN, or pass --token-file <path>.
  The token is intentionally not accepted on the command line.

Options:
  --athlete-id <id>       TrainingPeaks athlete ID (required)
  --database <path>       SQLite output (default: ./trainingpeaks.sqlite)
  --work-dir <path>       Download/extraction cache (default: ./.trainingpeaks)
  --end <YYYY-MM-DD>      Last export date (default: today in local time)
  --years <n>             History length in years (default: 5)
  --months-per-request <n>  Export window size (default: 3, max: 12)
  --token-file <path>     Read bearer token from a file
  --force                 Download completed windows again
  --keep-extracted        Keep unzipped working files after import
  --api-base-url <url>    Override API root (primarily for testing)
  -h, --help              Show this help
`;

function takeValue(argv: string[], index: number, option: string): string {
  const value = argv[index + 1];
  if (!value || value.startsWith("--")) {
    throw new Error(`${option} requires a value`);
  }
  return value;
}

export function parseArguments(argv: string[]): ParsedArguments {
  const result: ParsedArguments = {
    forceDownload: false,
    keepExtracted: false,
    help: false,
  };

  for (let i = 0; i < argv.length; i += 1) {
    const option = argv[i];
    switch (option) {
      case "--athlete-id":
        result.athleteId = takeValue(argv, i, option);
        i += 1;
        break;
      case "--token-file":
        result.tokenFile = takeValue(argv, i, option);
        i += 1;
        break;
      case "--database":
        result.databasePath = takeValue(argv, i, option);
        i += 1;
        break;
      case "--work-dir":
        result.workDirectory = takeValue(argv, i, option);
        i += 1;
        break;
      case "--end":
        result.endDate = takeValue(argv, i, option);
        i += 1;
        break;
      case "--years": {
        const value = takeValue(argv, i, option);
        result.years = Number(value);
        i += 1;
        break;
      }
      case "--months-per-request": {
        const value = takeValue(argv, i, option);
        result.monthsPerRequest = Number(value);
        i += 1;
        break;
      }
      case "--api-base-url":
        result.apiBaseUrl = takeValue(argv, i, option);
        i += 1;
        break;
      case "--force":
        result.forceDownload = true;
        break;
      case "--keep-extracted":
        result.keepExtracted = true;
        break;
      case "-h":
      case "--help":
        result.help = true;
        break;
      default:
        throw new Error(`Unknown option: ${option}`);
    }
  }

  return result;
}

function localToday(): string {
  const now = new Date();
  const localMidnightUtc = new Date(Date.UTC(now.getFullYear(), now.getMonth(), now.getDate()));
  return formatIsoDate(localMidnightUtc);
}

export function loadConfig(argv: string[], environment = process.env): Config | { help: true } {
  const args = parseArguments(argv);
  if (args.help) return { help: true };

  const athleteId = args.athleteId ?? environment.TRAININGPEAKS_ATHLETE_ID;
  if (!athleteId || !/^\d+$/.test(athleteId)) {
    throw new Error("A numeric --athlete-id (or TRAININGPEAKS_ATHLETE_ID) is required");
  }

  const token = args.tokenFile
    ? readFileSync(resolve(args.tokenFile), "utf8").trim()
    : environment.TRAININGPEAKS_TOKEN?.trim();
  if (!token) {
    throw new Error("Set TRAININGPEAKS_TOKEN or provide --token-file");
  }

  const endDate = args.endDate ?? localToday();
  assertIsoDate(endDate, "--end");

  const years = args.years ?? 5;
  if (!Number.isInteger(years) || years < 1 || years > 50) {
    throw new Error("--years must be an integer between 1 and 50");
  }

  const monthsPerRequest = args.monthsPerRequest ?? 3;
  if (!Number.isInteger(monthsPerRequest) || monthsPerRequest < 1 || monthsPerRequest > 12) {
    throw new Error("--months-per-request must be an integer between 1 and 12");
  }

  const apiBaseUrl = args.apiBaseUrl ?? "https://tpapi.trainingpeaks.com/fitness/v1/export";
  const parsedApiUrl = new URL(apiBaseUrl);
  if (!['http:', 'https:'].includes(parsedApiUrl.protocol)) {
    throw new Error("--api-base-url must be an HTTP(S) URL");
  }

  return {
    athleteId,
    token,
    databasePath: resolve(args.databasePath ?? "trainingpeaks.sqlite"),
    workDirectory: resolve(args.workDirectory ?? ".trainingpeaks"),
    endDate,
    years,
    monthsPerRequest,
    forceDownload: args.forceDownload,
    keepExtracted: args.keepExtracted,
    apiBaseUrl: apiBaseUrl.replace(/\/$/, ""),
  };
}
