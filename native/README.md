# TPGPT — native Rust app

The desktop app uses egui for its native chat UI and Wry for a separate TrainingPeaks login webview. SQLite, downloads, archive import, activity decoding, MCP, and CLI session handling are Rust. The application has no Vite frontend, Tauri dependency, web-based chat UI, or bundled Node runtime.

`src/auth-capture.js` is a document-start browser hook, embedded in the executable. It observes TrainingPeaks fetch/XHR authorization headers in the main document and child frames, and sends the bearer token through Wry's narrow IPC callback. It does not render the app UI. The login browser runs as a private helper instance of the same executable; captures travel over an anonymous stdout pipe to the native app. Tokens are kept in memory and never written to settings, SQLite, logs, CLI arguments, or the CLI environment.

## Build and run

Install Rust 1.88+ and one of the authenticated CLIs: `codex` or `claude`. The app uses the CLI's existing credentials and provider defaults. It does not require an OpenAI or Anthropic API key of its own.

The sidebar checks both CLIs in the background at startup and after changing executable paths. A green **Installed** check means the CLI answered `--version`; hover to see its version and path. **Not installed** links directly to the provider's installation docs. A found CLI that cannot run shows **Needs attention** with its error. **Setup & sign-in** explains how to choose, install, and sign in, and includes **Recheck installations** after installing or repairing a CLI. Chat is unavailable until the selected CLI can run. Sign-in is separate from the installation check.

Use the assistant account you already have. Follow the official [Codex installation docs](https://learn.chatgpt.com/docs/codex/cli) and run `codex login`, or follow the [Claude Code quickstart](https://code.claude.com/docs/en/quickstart) and run `claude` to sign in. Use `/login` inside Claude Code to sign in again. Choose that assistant in the sidebar, then start a new conversation; existing conversations keep their original assistant.

Linux build prerequisites on Ubuntu/Debian:

```bash
sudo apt install build-essential pkg-config libgtk-3-dev libwebkit2gtk-4.1-dev libxkbcommon-dev libegl1-mesa-dev
cargo run --release -p tpgpt
```

Windows requires the MSVC Rust toolchain and WebView2 runtime. macOS uses the system WebKit. Release builds run on Ubuntu, Windows, and macOS hosts. Automated checks cover compilation, unit tests, and the MCP protocol; interactive login and chat still need validation on each platform.

The app opens TrainingPeaks on launch. Log in normally; when a TrainingPeaks request carries a bearer token, the login window hides and chat receives the connection. Select **Sync history** to download workouts and activity files. The default is five years in three-month windows. Imported windows, source bytes, and resume status use the existing database schema. Workouts are deduplicated, activity files are decoded and indexed, and narrative search uses SQLite FTS5. FIT, TCX, GPX, and gzip activity files are supported; full records are sampled on demand through MCP.

If automatic athlete detection does not find your ID, enter the numeric athlete ID in **Settings**. The hook checks athlete/export request paths and account/profile responses. Reauthentication is needed after a rejected or expired token. The hook observes page fetch/XHR; requests made only inside a service worker are outside that hook. A live authenticated TrainingPeaks import must be checked with your account because its private API and login flow can change.

To reuse an existing local database, put its absolute path in **Settings**, or set this on first launch:

```bash
TRAININGPEAKS_DATABASE="$PWD/trainingpeaks.sqlite" cargo run --release -p tpgpt
```

An existing database can be queried without logging in:

```bash
TRAININGPEAKS_DATABASE="$PWD/trainingpeaks.sqlite" cargo run --release -p tpgpt -- --no-login
```

Settings apply to new conversations. Existing conversations retain their database and provider. Select Codex or Claude, start a conversation, and send a question. The app saves its own conversation ID, the provider's native session ID, and message history in a separate `chat.sqlite`. It resumes Codex with `codex exec resume <id>` and Claude with `claude --resume <id>`. Prompts go over stdin and JSON events feed the native interface. **Stop** cancels a running import or CLI call; completed import windows are retained. Interrupted chats are recoverable after restart.

The native chat UI renders Markdown, including headings, lists, tables, and code blocks. It restores your latest conversation when reopened, supports searching conversations and copying replies, and offers **Retry message** after a failed turn. Prompt cards help start a conversation. Press **Enter** (or **Ctrl/Cmd+Enter**) to send; use **Shift+Enter** for a new line. You can draft the next question while a reply is running.

CLI paths can be names on `PATH` or absolute paths. The launcher also searches common user installation locations, including `.local/bin`, `.npm-global/bin`, and NVM installations. CLIs are external prerequisites and are not bundled. Training history stays in local SQLite; the chosen CLI sends requested context to its model provider under that provider's account and terms.

Desktop launchers often omit shell-specific paths. Each CLI process receives the CLI's own directory first on `PATH`, followed by inherited paths and common runtime locations. This lets npm-installed CLIs find their matching Node runtime when launched from the application menu, including NVM installations; shell startup files are not executed.

App settings, chat history, imports, and per-conversation work directories live under the platform's user-data directory. The original `TrainingPeaks Chat` storage identifier is retained so existing data stays available after the rename to TPGPT. `TRAININGPEAKS_APP_DATA=/absolute/path` overrides that location for isolated development or testing. Databases containing other athlete IDs are rejected during import to avoid silently mixing accounts.

## Bundle

```bash
./scripts/bundle-native.sh
```

The script builds the native executable and creates a Linux `.deb` and `.tar.gz`, or a macOS `.app` zip and `.dmg` for the host architecture. On Windows, run `./scripts/bundle-windows.ps1` in PowerShell to create the portable `.zip`. Outputs are in `native/bundle/` and contain only the executable, installation instructions, and dependency license notices. macOS bundles are ad-hoc signed; developer signing and notarization are not configured. Windows bundles are unsigned and require the WebView2 runtime.

## Native command-line and MCP modes

The same executable provides the data layer to both providers. No global MCP registration is changed by the app: each CLI call receives a configuration pointing at the executable and the conversation's SQLite file.

```bash
tpgpt --overview --database /absolute/path/trainingpeaks.sqlite
tpgpt --mcp --database /absolute/path/trainingpeaks.sqlite
```

The MCP inventory includes coverage, workout search and detail, exact summaries, comparisons, training load, records, activity detail, planning context, plan scaffolding, and locally saved plans. Plan writes are separate from imported workouts and never upload to TrainingPeaks. The assistant is instructed to save a plan only after an explicit request or approval.

The importer is also available without the UI:

```bash
read -rsp "TrainingPeaks token: " TRAININGPEAKS_TOKEN
export TRAININGPEAKS_TOKEN
tpgpt --import --database /absolute/path/trainingpeaks.sqlite --athlete-id YOUR_ID --years 5 --months-per-request 3
unset TRAININGPEAKS_TOKEN
```

## Validate

```bash
cargo test --release --locked
cargo fmt --all -- --check
npm test
npm run typecheck
```

The npm checks cover the retained TypeScript tools and the small browser hook. They are development checks, not runtime dependencies of the native app.

Validation on this host includes the native GUI, the MCP stdio protocol, importer and analytics fixtures, browser fetch/XHR capture, process cancellation, and a real Codex MCP query followed by a resumed turn. Claude's adapter is implemented and its stream parser is tested; its live test reported an expired OAuth session, so Claude must be signed in again before that integration can be verified end to end. TrainingPeaks credential capture and export have been verified with a live login on Linux; private account data is excluded from the repository and release bundles.

CLI behavior follows the official [Codex non-interactive documentation](https://learn.chatgpt.com/docs/non-interactive-mode) and [Claude CLI reference](https://code.claude.com/docs/en/cli-reference). Export ranges follow [TrainingPeaks Data Export](https://help.trainingpeaks.com/hc/en-us/articles/204985370-Data-Export).
