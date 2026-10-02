# TPGPT

A native Rust desktop app for chatting with your TrainingPeaks history through Codex or Claude Code.

Log in to TrainingPeaks, sync workouts and activity files into local SQLite, and ask about training volume, load, trends, or plans. Conversations are saved locally and resume the assistant's original CLI session.

## Download and install

Download a bundle from [GitHub Releases](https://github.com/KerryRitter/tpgpt/releases/latest).

| Platform | Bundle | Installation |
| --- | --- | --- |
| Ubuntu / Debian, x86_64 | `tpgpt_VERSION_amd64.deb` | `sudo apt install ./tpgpt_VERSION_amd64.deb` |
| Linux, x86_64 | `tpgpt-linux-x86_64.tar.gz` | Extract, install the runtime dependencies below, then run `./tpgpt` |
| Windows, x86_64 | `tpgpt-windows-x86_64.zip` | Extract and open `TPGPT/tpgpt.exe` |
| macOS, Apple Silicon | `tpgpt-macos-arm64.dmg` or `.zip` | Copy `TPGPT.app` into Applications |
| macOS, Intel | `tpgpt-macos-x86_64.dmg` or `.zip` | Copy `TPGPT.app` into Applications |

Linux releases are built on Ubuntu 22.04. The archive requires GTK 3, WebKitGTK 4.1, EGL, OpenGL, and xkbcommon; on Ubuntu/Debian:

```bash
sudo apt install libgtk-3-0 libwebkit2gtk-4.1-0 libegl1 libgl1 libxkbcommon0
```

Windows needs the [Microsoft WebView2 Evergreen Runtime](https://developer.microsoft.com/en-us/microsoft-edge/webview2/#download-section), normally already present on Windows 10/11. macOS uses system WebKit. These releases have no developer code-signing certificate or Apple notarization; macOS offers an **Open Anyway** option in System Settings → Privacy & Security after an initial launch attempt. SHA-256 checksums are included with each release.

## Set up your assistant

Install and sign in to one of these external CLIs:

| Assistant | Install | Sign in |
| --- | --- | --- |
| Codex | [Official CLI setup](https://learn.chatgpt.com/docs/codex/cli) | `codex login` |
| Claude Code | [Official quickstart](https://code.claude.com/docs/en/quickstart) | Run `claude`; use `/login` to sign in again |

TPGPT checks both CLIs at startup. A green check means the executable runs; **Not installed** opens the installation docs. **Setup & sign-in** explains the remaining steps and lets you recheck. Select your assistant before starting a new conversation. Existing conversations keep their original assistant.

Open TPGPT, log in to TrainingPeaks in its login window, then select **Sync history**. The initial import covers five years in three-month windows and can resume after interruption. If athlete detection fails, enter your numeric athlete ID in **Settings**.

The desktop app is Rust with an egui chat interface and a Wry login webview. It has no JavaScript frontend or bundled Node runtime. A small embedded browser hook captures TrainingPeaks authorization headers. npm-installed assistant CLIs may require their own Node runtime.

## Your data

- Workouts, activity files, settings, and chat history stay in your user-data directory. An existing training database can be selected in **Settings**.
- TrainingPeaks bearer tokens stay in memory and are not stored in the database, settings, logs, or assistant process environment.
- The chosen assistant CLI sends prompts and requested training context to its model provider using your account. Its own session storage follows the CLI's configuration.
- Saved plans remain local; TPGPT does not upload workouts or plans to TrainingPeaks.
- No credentials, medical records, training databases, cached exports, or personal screenshots are included in this repository or its bundles.

TPGPT is an independent project and is not affiliated with TrainingPeaks, OpenAI, or Anthropic. TrainingPeaks login and export use its private API, which may change.

## Build from source

Install Rust 1.88+ and the platform prerequisites in [the native app guide](native/README.md).

```bash
cargo run --release --locked -p tpgpt
cargo test --release --locked
```

Build bundles with `./scripts/bundle-native.sh` on Linux/macOS or `./scripts/bundle-windows.ps1` on Windows. GitHub Actions builds on each platform, checks the source for private files and common credential formats, runs unit tests and an MCP smoke check, and assembles releases from explicitly selected artifacts. Dependency license notices are included in every bundle.

The original [TypeScript export and MCP tools](docs/legacy-tools.md) are retained for development and command-line use. They are not bundled with the desktop app.
