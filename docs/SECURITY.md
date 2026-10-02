# Publication and release checks

The repository contains source and public dependency license notices. Local medical records, training databases, export caches, settings, credentials, and screenshots are excluded. `scripts/audit-publication.py --staged` checks the exact Git index before publishing; CI checks tracked source again. GitHub secret scanning and push protection are enabled.

Bundles are assembled from explicit files in fresh staging directories. The release job accepts only the expected seven platform bundles and publishes SHA-256 checksums. No account login or personal training database is used by the release builds. Workflow actions are pinned to commit hashes and the downloaded audit tool is checked against a pinned SHA-256 digest.

Release CI runs `cargo audit` and `npm audit`; reported Rust vulnerabilities or high/critical npm findings block publication. The initial XML dependency was upgraded to quick-xml 0.41.0 to address RUSTSEC-2026-0194 and RUSTSEC-2026-0195.

The audit also reports informational upstream warnings: GTK's glib 0.18 `VariantStrIter` API has a soundness advisory (RUSTSEC-2024-0429), and proc-macro-error and ttf-parser are unmaintained. TPGPT does not call `VariantStrIter` and uses bundled fonts. These warnings remain in the toolkit dependency tree and are not claimed to be fixed; future toolkit updates need to be reviewed. A passing dependency audit is not a guarantee that software has no defects.

TrainingPeaks tokens remain in memory and are restricted to its trusted HTTPS hosts. Chat sends requested context to the chosen assistant provider. See the README for the data flow and platform signing limitations.
