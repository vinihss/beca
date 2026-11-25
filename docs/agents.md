# Beca Developer Guide

> Entry point for agents: [`AGENTS.md`](../AGENTS.md) (short, always loaded). This file is loaded via `opencode.jsonc` `instructions`. Runtime rules live in [`runtime-rules/`](runtime-rules/README.md).

## Project
- **beca**: UI shell for **Hyprland/Wayland** (Rust + [Slint](https://slint.rs/), **Lua** scripts). Single CLI, multi-module, runtime config/themes (XDG), no recompile to tweak theme/params.
- North star: `beca mod <module>:<action>` · config `$XDG_CONFIG_HOME/beca/config.toml` · themes `themes/*.toml` · scripts `scripts/*.lua`. See `docs/architecture.md` and `docs/schema/`.
- **Workspace:** `crates/beca-cli` (bin `beca`), `crates/beca-daemon` (IPC + handlers), `crates/beca-process`, `crates/beca-bar`, `crates/beca-launcher`, `crates/beca-voice`, `crates/beca-core` (config/theme + Lua runtime), `crates/beca-apps` (.desktop), `crates/beca-ui` (Slint kit), `crates/beca-layer-shell` (Wayland surfaces), `crates/beca-widgets` (shared bar widget providers/actions). `mlua` = **lua54** (vendored).

## Commands
- `cargo run -p beca-cli -- --help` / `beca mod --help`
- `beca mod alert:show [message] [--title T --message M --timeout-ms N]` — one-shot `AlertWindow` on a click-through layer shell (always local; `BECA_ALERT_TOP_MARGIN`)
- `beca mod launcher:show` — app launcher (`beca.ui.search_window` + `beca.apps`)
- `beca mod voice:show [--hold]` / `beca mod voice:transcribe -i <wav>` — native voice (engine `beca-ui` + config `beca-core`)
- `beca mod ...` routes through the daemon when active (spawn/lifecycle; `--local` forces local). `voice:transcribe` (pipe/stdout) and `alert:*` (one-shot overlay) are always local.
- `beca run <script.lua> [args...]` — runs a script with the `beca.*` runtime (path or name in `$XDG_CONFIG_HOME/beca/scripts/`); supports UI via `beca.ui.window/run/quit`
- `beca daemon` — IPC daemon (`$XDG_RUNTIME_DIR/beca/beca.sock`); `beca ping` / `beca subscribe` (stream Hyprland events)
- `cargo build --release -p beca-cli` · `cargo check` · `cargo test -p beca-core`
- Lint/format: `cargo fmt` / `cargo clippy`. No CI detected; extensive tests absent.

## Architecture / wiring
- **Separation**: Slint/UI in `*/ui/`, Rust in `*/src/`, scripts in `scripts/` or XDG.
- **`crates/beca-cli`** (bin `beca`): `cli.rs` (clap), `commands/` (`mod.rs`, `module.rs`, `registry.rs`, thin action wrappers), `output.rs`. No UI or daemon code lives here.
- **`crates/beca-process`**: /proc scanning, PID liveness, detach/spawn and runtime-file cleanup helpers.
- **`crates/beca-daemon`**: typed JSON-line protocol (`protocol.rs` `Request`), `client.rs`, `control.rs` (resident launcher/voice + local spawns), `handlers/{bar,launcher,voice,system}.rs`, `hyprland.rs`, `state.rs`.
- **`crates/beca-bar`**: `show.rs` (window fallback), `hyprland.rs` (geometry). Widget readers/actions now live in `beca-widgets`.
- **`crates/beca-launcher`**: resident launcher server, Lua bridge and UI wiring.
- **`crates/beca-voice`**: voice sessions, quick/converse, transcribe and resident server.
- **`crates/beca-core`**: `config.rs` (XDG/paths/load/merge), `bar.rs` (`BarProperties`, shared by both bar renderers), `theme.rs`, `error.rs`, `lua.rs`+`lua/{runtime,bridge,runner}.rs` (`beca.*` API, XDG scripts + fallback, `on_load`/`on_unload` hooks).
- **`crates/beca-apps`**: `desktop_entry.rs` + `indexer.rs` (discovery/launch `.desktop`).
- **`crates/beca-ui`**: `ui/` (kit: `Theme` in-out, `SearchWindow`, `VoiceCaptureWindow`, `bar/`, `voice/`, `components/`), `src/{lib,ui_loader,theme}.rs` + `src/voice/` (capture/transcribe/quick/streaming/view).
- **`crates/beca-layer-shell`**: layer-shell surfaces (`bar.rs`, `launcher.rs`, `quick.rs`, `voice.rs`, `platform.rs`) — resident bar/standalone HUD windows.
- **`crates/beca-widgets`**: `WidgetId`, `WidgetController`/`WidgetSink`, system readers (clock/volume/network/stats/alerts) and actions, shared by both bar renderers.
- **Daemon**: unix socket JSON lines, typed `Request` enum with `#[serde(tag = "type")]` (wire format unchanged) — `ping/version/hyprland.dispatch`/`hyprland.socket_paths`, `mod` (routing/spawn), `bar.status`, `launcher.status`, `config.reload`, `hyprland.subscribe` (stream `.socket2.sock`).
- **`beca.*` runtime**: `system` (notify/clipboard/exec/env/paths/log), `voice` (transcribe/config), `events` (on/off/emit), `runtime` (config/theme/paths/reload), `ui` (window/run/quit), `hyprland` (dispatch `{ok,response}`/read_events).

## Direction (target)
- Expand lifecycle/reload in living context and UI integration; evolve `beca.hyprland` (streaming `.socket2.sock` → `beca.events` in-process, daemon bridge).
- Daemon: surface management/lifecycle, layer-shell/Wayland, rich routing.
- Fully config-driven voice (title/flow/behavior via `beca-core`).
- **UI design system:** tokens semânticos, primitivas normalizadas e catálogo/storybook nativo separado (`beca-ui-catalog`), com identidade visual a reavaliar — ver `docs/ui-design-system.md`.
- **Build/performance:** diagnóstico read-only e medições das mudanças aplicadas em `docs/build-performance-audit.md`; ciclo de desenvolvimento em `docs/build-dev.md`. `default-members` é apenas `beca-cli` — use `--workspace` para validar os 13 crates.

## Gotchas
- **`--hold` vs pipe**: pipe = stdout+exit (binds), no `--hold`. STT daemon `POST /transcribe` → `{ "text" }` (default `127.0.0.1:4100`, `VOICE_DAEMON_URL`).
- **Slint**: only `Window` generates a Rust struct; non-`Window` exports are pruned from `library.slint` to avoid noise — components are resolved by name at runtime from the embedded bundle.
- **Scripts:** Lua 5.4. Global `beca.*` namespace. `beca` loads `<mod>.lua` from `$XDG_CONFIG_HOME/beca/scripts/` with embedded fallback.
