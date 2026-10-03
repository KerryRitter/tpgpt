# Publication and release checks

The repository contains source and public dependency license notices. Local medical records, training databases, export caches, settings, credentials, and screenshots are excluded. `scripts/audit-publication.py --staged` checks the exact Git index before publishing; CI checks tracked source again. GitHub secret scanning and push protection are enabled.

Bundles are assembled from explicit files in fresh staging directories. The release job accepts only the expected seven platform bundles and publishes SHA-256 checksums. No account login or personal training database is used by the release builds. Workflow actions are pinned to commit hashes and the downloaded audit tool is checked against a pinned SHA-256 digest.

Release CI runs `cargo audit` and `npm audit`; reported Rust vulnerabilities or high/critical npm findings block publication. The initial XML dependency was upgraded to quick-xml 0.41.0 to address RUSTSEC-2026-0194 and RUSTSEC-2026-0195.

The audit also reports informational upstream warnings: GTK's glib 0.18 `VariantStrIter` API has a soundness advisory (RUSTSEC-2024-0429), and proc-macro-error and ttf-parser are unmaintained. TPGPT does not call `VariantStrIter` and uses bundled fonts. These warnings remain in the toolkit dependency tree and are not claimed to be fixed; future toolkit updates need to be reviewed. A passing dependency audit is not a guarantee that software has no defects.

TrainingPeaks passwords are entered in its login page; the native app does not collect or store them. Captured tokens remain in memory, travel between local processes over an anonymous pipe, and are attached only to requests to trusted TrainingPeaks HTTPS hosts. Export redirects to other HTTPS hosts do not receive the token. Tokens are excluded from assistant process arguments and environment, and are not persisted in settings, SQLite, or app logs. Login and exports necessarily communicate with TrainingPeaks; credentials and tokens do not stay on-device during those authenticated requests.

The imported database, activity files, saved plans, settings, and app chat history are stored locally. There is no hosted TPGPT backend or automatic database upload. Chat sends prompts and the training context retrieved by the assistant to the chosen model provider. Assistant credentials and its session storage are managed by its CLI. This is local storage with cloud AI, not a promise that training data shared in chat never leaves the device. See the README for the data flow and platform signing limitations.
