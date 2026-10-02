# Legacy TypeScript tools

This TypeScript CLI downloads the last five years of TrainingPeaks `workouts` and `files` exports in three-month windows, safely unzips archives, and consolidates everything into one SQLite database. It also accepts the direct CSV format documented for workout-summary exports.

It is resumable. Completed windows are skipped on the next run, downloads are cached, HTTP 408/429/5xx responses are retried with backoff, and overlapping boundary dates are deduplicated during import.

TrainingPeaks permits export ranges of up to 12 months, but this CLI defaults to smaller three-month requests to reduce the likelihood of the files endpoint timing out. See [TrainingPeaks Data Export](https://help.trainingpeaks.com/hc/en-us/articles/204985370-Data-Export).

## Requirements

- Node.js 22.5 or newer
- A current TrainingPeaks bearer token

## Run

```bash
npm install
read -rsp "TrainingPeaks token: " TRAININGPEAKS_TOKEN && export TRAININGPEAKS_TOKEN
npm start -- \
  --athlete-id YOUR_ATHLETE_ID \
  --end 2026-09-17 \
  --years 5 \
  --months-per-request 3 \
  --database ./trainingpeaks.sqlite
unset TRAININGPEAKS_TOKEN
```

The token is deliberately not accepted as a command-line option, where it could be saved in shell history or exposed in process listings. Alternatively, put only the token in a permission-restricted file and use `--token-file /path/to/token-file`.

The default end date is today, so the shorter form is:

```bash
npm start -- --athlete-id YOUR_ATHLETE_ID
```

Useful options:

- `--force` downloads and imports every window again.
- `--keep-extracted` retains unzipped working directories. By default, downloads remain cached but extracted copies are removed after a successful import.
- `--work-dir <path>` changes the `.trainingpeaks` cache location.
- `--months-per-request <1-12>` changes the request size if three months is still too large.

If the process stops, run the same command again. Windows marked complete in `export_jobs` are skipped, and a cached valid ZIP can be imported without another API call.

## Database contents

- `workouts`: one row per exported workout, with common fields promoted to columns and the complete source record in `raw_json`.
- `workout_fields`: every original CSV/JSON field in a queryable name/value form.
- `files`: filenames, paths, extensions, MIME types, and references to their content.
- `file_blobs`: deduplicated binary contents of every unzipped export file, including FIT/TCX/GPX files and workout CSV/JSON files.
- `export_jobs`, `export_files`, and `workout_exports`: provenance and resume state for each three-month API request.

Example queries:

```sql
SELECT workout_date, workout_type, title
FROM workouts
ORDER BY workout_date DESC;

SELECT f.relative_path, b.byte_size, b.sha256
FROM files AS f
JOIN file_blobs AS b ON b.id = f.blob_id
ORDER BY f.relative_path;

-- Restore one stored file with the sqlite3 CLI:
SELECT writefile('restored.fit', b.content)
FROM files AS f
JOIN file_blobs AS b ON b.id = f.blob_id
WHERE f.file_name = 'example.fit'
LIMIT 1;
```

Run `npm test`, `npm run typecheck`, and `npm run build` to verify the project.

## Chat with the data through MCP

The project also includes a local TypeScript MCP server. It uses the existing SQLite file directly; no PostgreSQL, PGlite, external vector database, or cloud service is required.

The MCP combines:

- exact SQLite analytics for volume, distance, TSS, intensity, heart rate, power, and trends;
- SQLite FTS5 search across workout titles, descriptions, athlete comments, and coach comments;
- indexed FIT, TCX, and GPX session/lap metadata, with full activity records decoded only when requested;
- recent-load and planning context;
- locally persisted draft/active training plans.

Build the search and FIT indexes after importing or refreshing TrainingPeaks data:

```bash
npm run mcp:index -- \
  --database ./trainingpeaks.sqlite \
  --timezone UTC
npm run build
```

Start the server directly over stdio:

```bash
node ./dist/src/mcp/server.js --database ./trainingpeaks.sqlite
```

The server writes MCP protocol messages to stdout, so launch it through an MCP client rather than typing into it manually. Logs and Node warnings go to stderr.

### Codex registration

Register the compiled server globally with the Codex CLI:

```bash
codex mcp add trainingpeaks -- \
  node /absolute/path/to/tpgpt/dist/src/mcp/server.js \
  --database /absolute/path/to/tpgpt/trainingpeaks.sqlite
```

Confirm the registration:

```bash
codex mcp get trainingpeaks
```

Open a new Codex session after registration so the client discovers the new tool inventory.

### Available MCP capabilities

- `get_database_overview`
- `search_workouts`
- `get_workout`
- `summarize_training`
- `compare_training_periods`
- `get_training_load`
- `get_personal_records`
- `get_activity_detail` (FIT, TCX, and GPX)
- `get_planning_context`
- `draft_plan_framework`
- `save_training_plan`
- `list_training_plans`
- `get_training_plan`

The server also exposes `build-training-plan` and `review-my-training` prompts, plus overview and data-guide resources.

Example requests in an MCP-enabled chat:

- “Compare my running volume and TSS for the last eight weeks with the prior eight weeks.”
- “Find workouts where I mentioned unusual fatigue, heart symptoms, pain, or trouble recovering.”
- “Analyze the laps and heart-rate progression from my longest run in August.”
- “Build a 12-week plan around my current load for a fall half marathon. Ask me for missing constraints before writing it.”
- “Save that plan as a draft after I approve it.”

Planning tools provide general training guidance, not diagnosis or treatment. They instruct the model to ask about pain, injury, illness, medical restrictions, availability, and goals before prescribing a plan. Saved plans are separate from imported TrainingPeaks workouts and do not upload anything to TrainingPeaks.

Run the end-to-end MCP protocol check with:

```bash
npm run mcp:smoke
```
