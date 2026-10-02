#!/usr/bin/env node
import { HELP, loadConfig } from "./config.js";
import { runExports } from "./export-runner.js";

async function main(): Promise<void> {
  const config = loadConfig(process.argv.slice(2));
  if ("help" in config) {
    process.stdout.write(HELP);
    return;
  }
  const result = await runExports(config, (event) => {
    const line = `${event.phase.toUpperCase()} ${event.label}: ${event.message}\n`;
    (event.phase === "error" ? process.stderr : process.stdout).write(line);
  });
  process.stdout.write(`Finished: ${result.completed} completed, ${result.skipped} skipped, ${result.failed} failed.\n`);
  if (result.failed) process.exitCode = 1;
}

main().catch((error: unknown) => {
  process.stderr.write(`Fatal: ${error instanceof Error ? error.message : String(error)}\n`);
  process.exitCode = 1;
});
