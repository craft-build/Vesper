# 00 — Vesper Roadmap & Architecture

This is the index and architectural contract for turning the Vesper GUI skeleton
into a working Matrix client. Every other `NN-*.md` in this folder is a
self-contained implementation plan (usable directly as an AI prompt) for one
checkpoint. Each checkpoint ends in a **compilable, manually testable
deliverable**.

## Current state

- Dioxus 0.7 workspace: `packages/ui` (all components + design system),
  `packages/desktop`, `packages/web`, `packages/mobile`, `packages/api`.
- All UI talks to the backend exclusively through the trait
  `ui::data::client::VesperClient` (`packages/ui/src/data/client.rs`), pulled
  from Dioxus context as `Rc<dyn VesperClient>`. The only implementation today
  is `MockClient` (in-memory seed data).
- The workspace was scaffolded as Dioxus fullstack (`web`/`mobile`/`api` have
  `server` features). **We do not need fullstack**: Matrix IS the server. All
  state and I/O happens through `matrix-sdk` inside the client process.

## Target platforms

**Desktop first** (`packages/desktop` via `dx serve --platform desktop`).
The web target is descoped until checkpoint 11 (matrix-sdk supports wasm with
the IndexedDB store, but it doubles testing surface for zero user benefit right
now). Mobile remains scaffold-only.

## Architecture contract (applies to every checkpoint)

1. **The `VesperClient` trait stays the seam.** UI components never import
   `matrix-sdk` types. The trait may gain/evolve methods (e.g. async
   pagination, observers), but `ui::chat`/`ui::screens` keep consuming
   `Rc<dyn VesperClient>` + Dioxus signals only.
2. **New crate: `packages/client`** — houses the `MatrixClient`
   implementation of `VesperClient` and everything SDK-flavored. `ui` depends
   on it (or on a re-export), `matrix-sdk` never leaks into components.
3. **SDK runs on its own tokio runtime on a dedicated thread.** Dioxus
   desktop's executor is not a full tokio runtime. Pattern:
   `std::thread::spawn(|| tokio::runtime::Runtime::new().block_on(...))`.
   Commands go UI→runtime via `tokio::sync::mpsc::UnboundedSender`; results and
   live updates come back via oneshot channels that update signals.
   This bridge is built once in checkpoint 02 and reused everywhere.
   **Thread-ownership rule:** `Rc<dyn VesperClient>` and the `MatrixClient`
   struct live ONLY on the Dioxus side and are never moved onto (or shared
   with) the runtime thread. Conversely, the `matrix_sdk::Client` and its
   sync tasks live ONLY on the runtime thread. The only things crossing the
   boundary are channel messages, so every `Command` variant and every
   response payload must be owned, `Send` plain data (enforced by the channel
   types). `MatrixClient`'s fields are likewise restricted to Send-safe
   types: the command sender, `Arc`-shared snapshots, and signal handles
   (per rule 4). This is why `Rc` is sufficient for the Dioxus context
   despite the app being multithreaded underneath.
4. **Signals are the live-update mechanism.** `MatrixClient` keeps snapshots
   (room list, timelines) in memory; `VesperClient` methods read snapshots
   synchronously where the trait allows, while background sync tasks `.set()`
   `Signal`s exposed through a `ClientState` context so views re-render
   automatically.
5. **`matrix-sdk` 0.18** (verify latest on crates.io before each checkpoint)
   with features: default (`e2e-encryption`, `sqlite`) **plus**
   `bundled-sqlite` (avoid system SQLite dependency) and `markdown`. Enable
   `sso-login` later if wanted. Use `matrix-sdk-ui` 0.18 for
   `Timeline`/`RoomListService` — do NOT hand-roll sync.
6. **The Event Cache is on.** `client.event_cache().subscribe()` with the
   SQLite event-cache store; as of SDK 0.18 unread counts depend on it and it
   gives offline + fast back-pagination.

## Testing strategy (per checkpoint)

- Primary: run `dx serve --platform desktop`, log into a **throwaway account
  on matrix.org** (or a local Synapse/Conduit via docker compose if preferred).
- Each checkpoint doc lists a manual acceptance checklist.
- Where cheap: rust unit tests on mapping functions (Matrix types →
  `ui::data::model` types) using `matrix-sdk-test` fixtures.

## Checkpoints

| Doc | Checkpoint | Deliverable |
|-----|-----------|-------------|
| `01-workspace-cleanup.md` | Workspace & dependency foundation | App builds and runs unchanged on MockClient with new crate layout, tokio bridge skeleton, logging |
| `02-auth-session.md` | Login + session persistence | Real login to a homeserver, session restored across restarts, logout |
| `03-sync-roomlist.md` | Sync + room list | Live room/DM list with unread badges, updates as messages arrive |
| `04-timeline.md` | Room timeline (read) | Open a room, see real history, back-paginate |
| `05-composer-actions.md` | Send/reply/thread/react | Full conversation round-trip incl. reactions and threads |
| `06-live-state.md` | Typing, receipts, presence, notifications | Live typing indicators, read receipts, presence, desktop notifications |
| `07-media.md` | Media & attachments | Real avatars everywhere, image rendering, file/image send |
| `08-e2ee.md` | End-to-end encryption | Encrypted rooms decrypt, device list + SAS emoji verification wired to existing dialogs |
| `09-discovery-spaces.md` | Discovery & spaces | Join public rooms from DiscoveryModal; spaces appear in nav |
| `10-account-settings.md` | Account & settings | SettingsScreen fully functional: profile, devices, logout, preferences |
| `11-hardening.md` | Hardening & optional web | Offline resilience, error surfaces, packaging, web spike, release checklist |

Checkpoints 04–07 could in principle be reordered, but the listed order keeps
every deliverable demo-able on a real account from checkpoint 02 onward.

## Global conventions

- Keep Dioxus 0.7 idioms already used here (`Signal`, `use_context_provider`,
  `use_resource`, no `cx`/`Scope`).
- No server functions, no `#[post]`/`#[get]` — delete the fullstack remnants in
  checkpoint 01.
- Never log credentials or message content; `RUST_LOG` default `info` with
  `matrix_sdk=warn` unless debugging.
- Data directory: `dirs::data_dir()/vesper` (per-platform), one subdirectory
  per session/device ID.

## Implemented / Deviations (retrospective footer)

All eleven checkpoints shipped (01–11). The architecture held: the
`VesperClient` seam, runtime-thread SDK ownership, sync-storage signals
from the App scope, mock-first UI development. Notable global
deviations, each detailed in the per-checkpoint footers:

- **Secrets in the OS keyring** instead of mode-restricted files
  (checkpoint 11 §B; affects 02's session persistence and 08's store
  passphrase).
- **Vesper-owned media cache** with a 500 MB LRU cap instead of the SDK's
  unevicted sqlite media store (11 §C; affects 07).
- **O(batch) room-list diff application** pairing items with mapped rows
  (11 §C; affects 03) and a rendered-rows cap in timelines (affects 04).
- **Web target deferred** with rationale (11 §F): the trait seam and wasm
  build stay green, but shipping a real-backend web port was out of the
  spike window.
