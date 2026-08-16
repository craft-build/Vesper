# 02 — Authentication & Session Persistence

## Goal

First real homeserver contact: password login (with homeserver
auto-discovery), session tokens persisted to disk and restored across
restarts, logout, and a mock↔real switch so the mock remains usable for UI
work.

## Deliverable / how to test

1. `dx serve --platform desktop` → LoginScreen now accepts
   homeserver/user/password; log in with a real test account → app shows your
   real display name in the shell (room list may still be mock/empty — that's
   checkpoint 03).
2. Quit and relaunch: you're still logged in, no password prompt.
3. Settings → Log out (temporary button in settings is fine) → back to
   LoginScreen, session files removed.
4. `VESPER_BACKEND=mock dx serve --platform desktop` (or a `--mock` flag)
   returns the fully mocked experience.

## Context

- Trait methods to implement first: `login`, `me` (and scaffolding for the
  rest — `unimplemented!()`-returning stubs wired but not yet called by UI).
- `App` (`packages/ui/src/app.rs:29`) currently provides
  `Rc::new(MockClient::default())`; this is the ONE place the backend is
  chosen.
- LoginScreen (`packages/ui/src/screens/login_screen.rs`) needs a homeserver
  field and needs to surface `ClientError` instead of succeeding instantly.

## Design decisions

- **`client::session` module**: builds `matrix_sdk::Client` via
  `Client::builder().server_name(server_name).handle_refresh_tokens()` →
  `.sqlite_store(data_dir/<device_id-or-new>)` → `.build()`. On first
  login, `matrix_auth().login_username(u, p).initial_device_display_name("Vesper (macOS)").send()`.
- **Session file**: after login serialize the user's session
  (`AuthSession::Matrix(...)` via the SDK's session type) to
  `data_dir/session.json` (0600 perms; note in code that a keyring upgrade is
  a later hardening item). On launch, if the file exists: deserialize,
  `matrix_auth().restore_session(session)` → validate with `whoami()`; on
  failure delete the file and show login.
- **Command bridge grows**: `Command::Login { homeserver, user, pass, respond }`,
  `Command::Restore { respond }`, `Command::Logout { respond }`. The
  `matrix_sdk::Client` constructed by these commands stays owned by the
  runtime task forever — `MatrixClient` (which sits behind
  `Rc<dyn VesperClient>` on the Dioxus side) holds **only** the
  `UnboundedSender<Command>` plus Send-safe snapshots/signal handles, and
  never an SDK handle. Per the thread-ownership rule in `00-roadmap.md` §3,
  nothing non-`Send` crosses a thread boundary; the channel payloads are
  enforced `Send` by construction.
- **Backend switch**: `fn backend() -> Rc<dyn VesperClient>` in
  `ui::data` reading env var `VESPER_BACKEND` (default `matrix`, `mock` =
  MockClient). `App` calls this; keeps components untouched.
- **Errors**: map SDK errors to friendly strings in `ClientError` (unknown
  homeserver, bad credentials, network down). Never include tokens.
- `me()` after login: use account profile (`client.account().get_profile()`),
  map to `model::Me`. Avatar URL stays `None`/placeholder until checkpoint 07.

## Implementation steps

1. `client/src/session.rs`: `async fn connect_login(...)`, `async fn connect_restore(...)`, `async fn logout()` + serialization helpers.
2. `ClientState` runtime wiring: implement the three commands in the bridge;
   expose a handle type used by the `MatrixClient` impl.
3. `client/src/lib.rs`: `pub struct MatrixClient { /* cmd sender, cached Me signal */ }`
   with `MatrixClient::new() -> (Self, JoinHandle)`; implement first two
   `VesperClient` methods; others return `Err`/empty stubs with tracing warns.
   The trait is `?Send`/async — the impl just forwards commands over the
   channel and awaits oneshot responses, so no tokio context is needed on the
   Dioxus side.
4. `ui::data::backend()` switch + move provider in `app.rs` to use it.
5. LoginScreen: add homeserver input (default `matrix.org`, accept full MXIDs
   with implied homeserver), wire `on_login` to `client.login(...)`, show
   `ClientError` inline, disable inputs while pending.
6. `App`: on mount, attempt session restore (a `use_resource` that calls a new
   `MatrixClient::restore()` before first paint of login screen; while pending
   show the existing LOGO splash state).
7. Settings: minimal "Log out" row now (full settings come in checkpoint 10);
   on success, clear `me` signal → router falls back to LoginScreen.

## Acceptance criteria

- [ ] Fresh login against matrix.org works, wrong password shows an inline error.
- [ ] Relaunch restores session without a network-visible password prompt; killing network during restore falls back to login screen with a clear error, NOT a panic.
- [ ] Logout clears `session.json` and the SDK store dir.
- [ ] Mock mode still works end-to-end.
- [ ] No `matrix_sdk` imports anywhere under `packages/ui/src` except `ui::data` re-exports (enforce with a grep in CI/manual check).

## AI implementation prompt

> Implement real Matrix authentication for Vesper per docs/00 and docs/02.
> The repo has a `VesperClient` trait seam (packages/ui/src/data/client.rs)
> and a runtime bridge skeleton (packages/client/src/runtime.rs). Add session
> connect/restore/logout to packages/client using matrix-sdk 0.18 (sqlite
> store under dirs::data_dir()/vesper, session serialized to session.json),
> add Login/Restore/Logout commands to the bridge, implement
> `VesperClient for MatrixClient` for `login`/`me` with the rest as stubs, add
> a VESPER_BACKEND env switch selecting mock vs matrix in ui::data, and update
> LoginScreen (homeserver field + inline errors) and Settings (logout row).
> App must restore the session on relaunch. Verify with a matrix.org test
> account. Never log credentials.

## Implemented / Deviations (retrospective footer)

**Implemented** as designed: password login with well-known discovery,
`MatrixSession` persistence + restore-across-restart, logout (remote +
local wipe), `VESPER_BACKEND=mock` switch, friendly fixed-sentence error
mapping (never raw server text).

**Deviations**:
- **Tokens moved off disk (checkpoint 11).** `session.json` is gone; the
  session blob lives in the OS keyring (service `dev.vesper.app`), with a
  `0600` file fallback + loud warning where no keyring exists. First
  launch after upgrade migrates the old file into the keyring and deletes
  it. See `packages/client/src/secrets.rs`.
- Errors are no longer a bare string: `ClientError` carries a
  `ClientErrorKind` (auth/network/rate-limited/…) consumed by the toast
  center (checkpoint 11 workstream A). Fixed sentences unchanged.
- Restore failures fall through to the login screen with a warning log
  instead of surfacing an error dialog — a stale session is not a
  recoverable error state for the user.
