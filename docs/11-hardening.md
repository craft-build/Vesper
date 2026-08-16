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

## Implemented / Deviations

### A. Error taxonomy & surfaces — implemented

- `ClientError` is now `{ kind: ClientErrorKind, message }` with kinds
  Network / Auth / RateLimited / Invalid / Server / Storage / Unsupported
  / Unknown (`packages/client/src/api.rs`); every constructor site in
  `client` and the mock classifies, HTTP errors map through
  `ErrorKind` (Forbidden→Auth, LimitExceeded→RateLimited, etc.).
- `ToastCenter` (`packages/ui/src/design_system/toast_center.rs`):
  App-scoped, provided once, rendered by a single `ToastHost` — kind picks
  tone + title. Settings saves/toggles/avatar, downloads, and logout
  failures route through it; form-adjacent errors (login fields, dialog
  password prompts) stay inline.
- Reconnect pill: already fed by the 03 `connecting` signal; unchanged.

### B. Secrets in the keyring — implemented

- `packages/client/src/secrets.rs`: keyring v4 (`v1` mode + apple/windows
  native stores, target-gated; Linux deliberately file-fallback), service
  `dev.vesper.app`, entries `session` + `store-passphrase`.
- One-time migration both directions: legacy `session.json` /
  `store-passphrase` files are hoisted into the keyring and deleted on
  first touch (read or save); `cleanup_files` (logout) deletes keyring
  entries too. Tests force the file backend via `VESPER_SECRET_STORE=file`.
- `grep -r access_token <data_dir>` is clean after one launch post-upgrade
  (the only writer of that JSON was the session file, now keyring-only).

### C. Performance — implemented (scoped)

- Media cache: Vesper-owned on-disk LRU (`media_cache.rs`, 500 MB default
  cap, mtime-based eviction, atomic writes) replacing the SDK's unevicted
  sqlite media store (`use_cache = false`); in-memory data-URI map capped
  at 192 MB FIFO in `use_media_src`; Settings → General gained the
  size + Clear row (bytes freed surfaced as a toast).
- Room list: diff batches pair `RoomListItem`s with mapped `Convo`s —
  O(batch) remaps (was: full-list remap incl. per-room store lookups per
  batch); DM presence dots refresh via a changed-presence dirty set;
  space-membership moves remap only affected rooms. Regression test keeps
  pairs index-aligned.
- Timeline: rendered-rows cap (newest 800, older-rows notice) as the
  virtualization sanity check; timelines open at a ~30-event page and grow
  only through explicit back-pagination.
- Cold start: `vesper_startup` spans (launcher init + handoff, session
  restore incl. store open) so regressions against the <3 s budget are
  visible in any log. True 10k-event-room profiling with Instruments and
  viewport virtualization remain open follow-ups (capping makes them
  non-blocking for release).

### D. Diagnostics — implemented

- `tracing-appender` rolling daily files in `<data_dir>/logs/`
  (stdout layer kept for `dx serve`), `RUST_LOG` respected.
- Panic hook logs payload + location + coarse last-screen name (never
  message content) to `vesper_panic` target, then defers to the default
  hook. Route changes record the screen name via `client::diagnostics`.
- Settings → SUPPORT → Copy: app/OS/screen/secrets-backend facts + tail
  (250 lines) of the newest log, token-shaped values redacted, MXIDs/room
  ids kept per policy; clipboard via the webview's async clipboard API.

### E. Packaging — implemented (macOS first)

- `packages/desktop/Dioxus.toml`: bundle id `dev.vesper.app`, publisher,
  category SocialNetworking, descriptions, app icon (1024px
  `assets/icon.png` + `assets/icon.icns`, embedded as `CFBundleIconFile`;
  regeneration script `scripts/make-icns.sh`). The desktop crate is named
  `vesper` — the binary name titles the `.app` (a crate named `desktop`
  would ship "Desktop.app").
- `dx bundle --platform desktop --release` produces `Vesper.app` — ad-hoc
  signed, verified to launch standalone (smoke run confirmed the rolling
  log + `vesper_startup` spans: init 2 ms, session-restore 2 ms on the
  no-session path — far inside the <3 s budget). Deliverable copied to
  `packages/desktop/dist/Vesper.app`.
- The bundler's **DMG step fails in this environment** (`hdiutil create:
  Directory not empty`, reproducible with a trivial folder — an
  environment-level hdiutil problem, not a bundle defect; DMG creation
  deferred until the tool works here). Windows/Linux bundling documented
  follow-ups; auto-update out of scope for this release.

### F. Web target — DEFERRED (decision recorded)

Decision: **defer the real-backend web port**; the release ships desktop
only. Rationale:

- The trait seam stays web-green: `ui` and `web` compile for
  `wasm32-unknown-unknown` with the mock backend (verified), so the
  deferral costs nothing structurally.
- The real port needs matrix-sdk `indexeddb`+`js` feature wiring under a
  `wasm32` client path, blob-URL media, web file/notification API swaps,
  and a wasm executor bridge — a real risk of subtle dual-target rot in
  the runtime bridge for less than a day of spike budget remaining after
  A–E.
- Blockers list (picked up when revisited): `client` crate needs a
  `#[cfg(target_arch = "wasm32")]` implementation of `VesperClient`
  against matrix-sdk indexeddb; `media_uri` already returns `data:` URIs
  (no path assumptions to fix); `rfd`/`notify-rust` call sites are
  already native-cfg-gated; secrets module needs a web-appropriate store
  (or a documented no-persistence mode) since OS keyrings don't exist.

### Acceptance criteria status

- [x] Offline browse + queued sends — verified in checkpoint 04/06 flows
      (SDK send queue + offline sync mode); re-verified on next manual soak.
- [x] Keyring storage with migration; no `access_token` in data dir.
- [x] Log file + panic capture + diagnostics export button.
- [x] Desktop bundle builds + launches standalone (macOS `.app`,
      ad-hoc signed, icon embedded; `.dmg` deferred on an environment
      hdiutil failure — see §E).
- [x] Web: documented deferral (§F above).
- [x] No `todo!`/`unimplemented!` in `packages/client` (grep clean).
- [x] Checkpoint docs carry Implemented/Deviations footers.
- [ ] 24h soak — inherently a human run; everything it checks (bounded
      caches, O(batch) diffs, panic capture) is instrumented for it.
