# 11 — Hardening, Packaging & Optional Web Target

## Goal

The "release-quality" pass over everything built in 01–10: predictable
failure behavior, sane performance, secrets out of plain files, debuggable
logs, an installable desktop artifact, and a decision (build or defer) on the
web target.

## Deliverable / how to test

1. **Soak test**: leave Vesper running 24h on a busy account behind sleep/
   wake cycles and network flapping → memory stable (< ~1.5× baseline),
   room list/timelines still live, no ghost duplicates, no crash logs.
2. Airplane-mode test: app stays navigable on cache; sends queue and flush on
   reconnect; no unhandled-rejection panics.
3. Session tokens/crypto passphrases no longer readable in plain files
   (OS keyring).
4. `dx bundle --platform desktop --release` produces a signed-or-ad-hoc
   macOS app bundle that launches standalone.
5. **Web decision**: either `dx serve --platform web` builds and logs in
   (IndexedDB store, with a tracked issue list for what degrades: media file
   paths → blob URLs, notifications → web notifications, rfd → web file
   picker), or this doc records a clear decision to defer with rationale.
6. Every panic hook logs to a file the user can attach to an issue.

## Workstreams (parallelizable)

### A. Error taxonomy & surfaces
- Map `matrix_sdk::Error`/HTTP/UTD/rate-limit cases into a `ClientErrorKind`
  enum consumed by a single toast/snackbar component; current stringly errors
  become structured. Global "sync disconnected / reconnecting" pill fed by the
  03 status signal. Retry policy: SDK handles sync retries; app-level retries
  only for sends (queue) and joins (manual retry).

### B. Secrets in the keyring
- `keyring` crate: session `AuthSession` blob + crypto-store passphrase move
  from `data_dir` files to OS keychain (fallback to file with a loud warning
  on headless/Linux-without-keyring). One-time migration reads old
  session.json, stores, deletes. Re-run 02's session-restore tests.

### C. Performance
- Timeline virtualization sanity check (measure with a 10k-event room; if jank,
  cap rendered rows around the viewport).
- Room-list diff application: ensure O(page) remaps, not full-list rebuilds,
  on high-churn diffs.
- Cold start budget: <3s to interactive on warm store; trace startup spans and
  fix regressions. Media/avatar cache size cap (e.g. 500MB LRU) with a
  settings row from checkpoint 10.

### D. Diagnostics
- `tracing-appender` rolling log file in data dir (`logs/`), `RUST_LOG`
  override respected, panic hook writing to log + last-screen state dump
  (never message content). "Copy diagnostics" button in settings (redacts
  MXIDs? no — keep MXIDs, redact bodies/tokens).

### E. Packaging
- `Dioxus.toml` bundle config: app icon, bundle id `dev.vesper.app`, category.
- macOS ad-hoc signing + `.app` + `.dmg` via `dx bundle`; document Windows/
  Linux follow-ups. Auto-update: explicitly out of scope (note future
  tauri-updater-style or manual-releases path).

### F. Web target (build-or-defer decision gate)
- Attempt: enable `indexeddb` + `js` matrix-sdk features under a
  `#[cfg(target_arch = "wasm32")]` client-crate path; adapt `media_uri` (07)
  to blob URLs, swap `rfd` dialogs for web file APIs, swap notify-rust for the
  Notifications API, runtime bridge to the wasm executor (the SDK abstracts
  spawning; our tokio thread becomes wasm-bindgen-futures spawns).
- Ship only if login + room list + timeline work end-to-end in the spike
  window (one focused day); otherwise record the deferral + blockers here and
  keep web out of the release.

## Acceptance criteria

- [ ] 24h soak: no crash, bounded memory, live updates intact.
- [ ] Offline browse + queued sends verified.
- [ ] Keyring storage with migration; `grep -r "access_token" data_dir` clean.
- [ ] Log file + panic capture + diagnostics export button.
- [ ] Desktop bundle builds and runs standalone.
- [ ] Web: shipped-with-caveats OR documented deferral.
- [ ] Zero unimplemented trait stubs left in `MatrixClient` (`grep todo!/unimplemented! packages/client`).
- [ ] `docs/` updated where later decisions diverged from these plans; each
      checkpoint doc gains a short "Implemented / Deviations" footer.

## AI implementation prompt

> Execute Vesper's hardening pass per docs/00 and docs/11, working through
> workstreams A–F: structured ClientErrorKind with a single toast system and
> reconnect pill; migrate session + crypto passphrase to the keyring crate
> with file-fallback warning and one-time migration; profile a 10k-event room
> and cold start against the <3s budget, cap media cache LRU; rolling log
> files + panic hook + diagnostics export (redact bodies/tokens, keep MXIDs);
> Dioxus.toml bundle config and a standalone macOS build via dx bundle; then
> time-box the web-target spike (indexeddb/js sdk features, blob media URIs,
> web file + notification APIs) and either land it or record the deferral in
> this doc. Finish by sweeping packages/client for unimplemented!/todo!()
> stubs and adding Implemented/Deviations footers to every checkpoint doc.
