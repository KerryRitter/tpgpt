#!/usr/bin/env node
import { indexActivities, parseIndexOptions } from "./indexer.js";

indexActivities(parseIndexOptions(process.argv.slice(2))).catch((error: unknown) => {
  process.stderr.write(`${error instanceof Error ? error.stack ?? error.message : String(error)}\n`);
  process.exitCode = 1;
});
