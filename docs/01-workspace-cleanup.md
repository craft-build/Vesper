# 01 — Workspace Cleanup & Dependency Foundation

## Goal

Reshape the workspace from "Dioxus fullstack scaffold" into "native Matrix
client" and land every dependency later checkpoints need — without changing any
user-visible behavior. The app must still run end-to-end on `MockClient` after
this checkpoint.

## Deliverable / how to test

1. `cargo check --workspace` passes.
2. `dx serve --platform desktop` launches, mock login works, all screens behave
   exactly as before.
3. `RUST_LOG=info,ui=debug dx serve --platform desktop` shows tracing logs.
4. `cargo build -p client` compiles a new (still unused) crate linking
   `matrix-sdk`.

## Context

- Relevant files: root `Cargo.toml`, `packages/*/Cargo.toml`,
  `packages/ui/src/data/client.rs` (trait), `packages/ui/src/app.rs`
  (providers).
- matrix-sdk current stable: **0.18** (features: `e2e-encryption` and `sqlite`
  are default; we add `bundled-sqlite`, `markdown`). `matrix-sdk-ui` 0.18 gives
  `Timeline` and `RoomListService`.

## Design decisions

- **Drop the `api` package entirely** and remove it from workspace members —
  there is no server side; Matrix homeservers are the backend.
- **Web/mobile descoped**: remove the `fullstack`/`server` features from
  `packages/web/Cargo.toml` and `packages/mobile/Cargo.toml` but keep the
  crates compiling (web stays a thin `dioxus::launch(ui::App)` shell with only
  the `web` renderer feature; mobile likewise with `mobile`). Do not delete
  them — checkpoint 11 may revive web.
- **New crate `packages/client`**: `matrix-sdk` + bridge code. `ui` does NOT
  depend on it yet (MockClient still wired in `app.rs`); the dependency is
  added in checkpoint 02 so this diff stays behavior-neutral.
- **Runtime bridge skeleton** (`packages/client/src/runtime.rs`): a
  `ClientRuntime` that owns a `tokio::runtime::Runtime` on a dedicated thread
  plus an `UnboundedSender<Command>`; commands are an enum that checkpoint 02
  starts filling in (`Login { ... }`, later `SendMessage`, ...). Results return
  via per-command oneshot/callback. This file must compile now even if it's
  only exercised by a unit test.

## Implementation steps

1. Delete `packages/api`; remove `"packages/api"` and `api = ...` from the root
   `Cargo.toml`.
2. In `packages/web/Cargo.toml` and `packages/mobile/Cargo.toml`: drop
   `features = ["fullstack"]` from the dioxus dependency, delete the `server`
   feature entries, and audit `packages/web/src` / `packages/mobile/src` for
   fullstack/`server_fn` references (remove). Web keeps the `web` feature
   defaulting on; mobile keeps the `mobile` feature.
3. Create `packages/client`:
   ```toml
   [package]
   name = "client"
   version = "0.1.0"
   edition = "2021"

   [dependencies]
   matrix-sdk = { version = "0.18", features = ["bundled-sqlite", "markdown"] }
   matrix-sdk-ui = "0.18"
   tokio = { version = "1", features = ["rt-multi-thread", "macros", "sync", "fs"] }
   async-trait = "0.1"
   anyhow = "1"
   tracing = "0.1"
   futures = "0.3"
   directories = "6"
   serde = { version = "1", features = ["derive"] }
   serde_json = "1"
   ```
   (Pin the exact 0.18.x available; note in Cargo.toml to re-check latest.)
4. Implement `client::runtime::ClientRuntime` per the design above, with a
   `spawn()` constructor returning `(ClientRuntime, UnboundedSender<Command>)`
   and a dummy `Command::Ping` handled by a test.
5. Add `tracing-subscriber` (with `fmt` + `env-filter`) to
   `packages/desktop/Cargo.toml`; init in `desktop`’s `main()` with
   `EnvFilter::try_from_default_env().unwrap_or("info,matrix_sdk=warn,ui=debug")`.
6. Wire `client` into the workspace members and as an (unused-for-now)
   dependency of nothing — verify it builds standalone.
7. Update `AGENTS.md`/`README` only if they mention the removed fullstack
   pieces.

## Acceptance criteria

- [ ] No crate in the workspace depends on `dioxus/fullstack`.
- [ ] `packages/api` gone; build green.
- [ ] `client` crate compiles with matrix-sdk 0.18 linked; `cargo test -p client` passes the Ping round-trip test through the runtime bridge.
- [ ] Desktop app visually identical to before (mock data).
- [ ] `tracing` output visible on launch.

## AI implementation prompt

> Restructure the Vesper workspace for native Matrix-client development per
> docs/00-roadmap.md and this file. Remove `packages/api`, strip fullstack
> features from web/mobile, add a new `packages/client` crate depending on
> matrix-sdk 0.18 (+ bundled-sqlite, markdown) and matrix-sdk-ui 0.18, with a
> tokio-runtime bridge (`ClientRuntime`, `UnboundedSender<Command>`) and a Ping
> test. Add tracing-subscriber to the desktop entrypoint. Behavior of the
> running app must be unchanged (still MockClient). Show the final root
> Cargo.toml and new crate layout, and prove `cargo check --workspace` and
> `dx serve --platform desktop` succeed.

## Implemented / Deviations (retrospective footer)

**Implemented**: five-crate workspace (`ui` / `client` / `desktop` /
`web` / `mobile`), the `VesperClient` trait seam keeping matrix-sdk out
of the UI crate, mock backend co-existing with the real client, shared
workspace deps.

**Deviations**: none structural. The seam proved durable through
checkpoints 02–11; the only addition is `ClientErrorKind` inside the
existing `ClientError` (checkpoint 11 §A) and wasm-conditional client
features (`matrix` off under `wasm32`), both anticipated by the
"feature-free seam" rule.
