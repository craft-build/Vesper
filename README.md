# Vesper

Vesper is a Matrix chat client built with Rust, Dioxus 0.7, and
`matrix-sdk`. The first release targets macOS desktop. The web crate is a
mock-backed UI preview, and Android/iOS packaging is not yet release-ready.

## Workspace

- `packages/client` — Matrix SDK integration, session storage, sync, timelines,
  encryption, media, and notifications.
- `packages/ui` — shared Dioxus application and design system.
- `packages/desktop` — desktop launcher, diagnostics, and bundle configuration.
- `packages/mobile` — mobile launcher (development only).
- `packages/web` — mock-backed web preview; it does not connect to Matrix.
- `docs` — implementation checkpoints and release-hardening decisions.

## Prerequisites

- A current stable Rust toolchain
- Dioxus CLI 0.7:

  ```sh
  cargo install dioxus-cli --version '^0.7'
  ```

## Run the desktop app

```sh
cd packages/desktop
dx serve
```

Vesper stores application data in the platform data directory. Set
`VESPER_DATA_DIR` to override it for isolated development runs. On macOS and
Windows, credentials use the OS keyring; unsupported or unavailable keyrings
fall back to owner-only files and emit a warning.

## Quality checks

Run these from the workspace root:

```sh
cargo fmt --all -- --check
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo audit
```

## Build the macOS bundle

```sh
cd packages/desktop
dx bundle --platform desktop --release
```

Bundle metadata lives in `packages/desktop/Dioxus.toml`. Signing,
notarization, and distribution-channel policy must be completed before a
public release. See `docs/11-hardening.md` for the current readiness status and
known platform limitations.

## License

Vesper is licensed under either of the following, at your option:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))
