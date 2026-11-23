# Beca — project instructions

Beca is a Wayland/Hyprland shell written in Rust + Slint with Lua scripts. Runtime config and themes live under XDG; changing them must not require a rebuild.

## Workspace layout

- `crates/beca-cli` — bin `beca`: clap CLI + thin command dispatch. No UI or IPC implementation here.
- `crates/beca-daemon` — typed JSON-line IPC, client, lifecycle control, handlers, Hyprland bridge.
- `crates/beca-process` — `/proc` scanning, PID liveness, detached spawns, runtime-file cleanup.
- `crates/beca-bar`, `crates/beca-launcher`, `crates/beca-voice` — module implementations.
- `crates/beca-core` — XDG config/theme, `BarProperties`, Lua runtime/bridge/runner.
- `crates/beca-apps` — `.desktop` discovery and launch.
- `crates/beca-ui` — Slint kit + voice engine.
- `crates/beca-layer-shell` — Wayland layer-shell surfaces.
- `crates/beca-ui-catalog`, `crates/beca-panel` — catalog/panel experiments; not the shell entry point.

## Hard contracts

- Public commands stay: `beca mod`, `beca daemon`, `beca ping`, `beca subscribe`, `beca config`, `beca run`, `beca restart`.
- Process detection depends on the exact cmdlines `beca mod --local bar:show-top` and `beca mod --local bar:show-bottom`. Do not change them.
- Daemon wire format is JSON lines with `{"type": ...}`. Keep it stable for autostart and external scripts.
- Autostart: `~/.config/hypr/scripts/beca-autostart.sh` calls `$HOME/.local/bin/beca`.
- Code, modules and types in English; notes and docs may be in Portuguese.

## Verify

- `cargo check` (root) covers only `beca-cli` — `default-members = ["crates/beca-cli"]`. Use `cargo check --workspace` for all 12 crates.
- `cargo test -p beca-core -p beca-daemon`
- `cargo test --workspace --lib`
- `cargo build --release -p beca-cli` → install to `~/.local/bin/beca`
- Runtime: `beca --help`, `beca mod list`, `beca ping`, `beca mod bar:status`

## References

- Detailed developer guide: `docs/agents.md` (loaded by `opencode.jsonc` `instructions`).
- Architecture: `docs/architecture.md`; build tips: `docs/build-dev.md`; build diagnosis: `docs/build-performance-audit.md`.
- Runtime rules: `docs/runtime-rules/README.md` — read only the file relevant to the change.
