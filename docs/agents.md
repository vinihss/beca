# AGENTS.md

## Project
- **beca**: UI shell for **Hyprland/Wayland** (Rust + [Slint](https://slint.rs/), **Lua** scripts). Single CLI, multi-module, runtime config/themes (XDG), no recompile to tweak theme/params.
- North star: `beca mod <module>:<action>` · config `$XDG_CONFIG_HOME/beca/config.toml` · themes `themes/*.toml` · scripts `scripts/*.lua`. See `docs/architecture.md` and `docs/schema/`.
- **Workspace:** `crates/beca` (CLI/daemon), `crates/beca-core` (config/theme + Lua runtime), `crates/beca-apps` (.desktop), `crates/beca-ui` (Slint kit), `crates/beca-layer-shell` (Wayland layer-shell surfaces). `mlua` = **lua54** (vendored).

## Commands
- `cargo run -p beca -- --help` / `beca mod --help`
- `beca mod launcher:show` — app launcher (`beca.ui.search_window` + `beca.apps`)
- `beca mod voice:show [--hold]` / `beca mod voice:transcribe -i <wav>` — native voice (engine `beca-ui` + config `beca-core`)
- `beca mod ...` routes through the daemon when active (spawn/lifecycle; `--local` forces local). `voice:transcribe` is always local (pipe/stdout).
- `beca run <script.lua> [args...]` — runs a script with the `beca.*` runtime (path or name in `$XDG_CONFIG_HOME/beca/scripts/`); supports UI via `beca.ui.window/run/quit`
- `beca daemon` — IPC daemon (`$XDG_RUNTIME_DIR/beca/beca.sock`); `beca ping` / `beca subscribe` (stream Hyprland events)
- `cargo build --release -p beca` · `cargo check` · `cargo test -p beca-core`
- Lint/format: `cargo fmt` / `cargo clippy`. No CI detected; extensive tests absent.

## Architecture / wiring
- **Separation**: Slint/UI in `*/ui/`, Rust in `*/src/`, scripts in `scripts/` or XDG.
- **`crates/beca`** (bin): `cli.rs`, `comandos.rs`+`comandos/{launcher,voice,bar,runner}.rs`, `daemon.rs`+`daemon/{socket,hyprland}.rs`. `beca mod` routed via daemon, `beca run`.
- **`crates/beca-core`**: `config.rs` (XDG/paths/load/merge), `theme.rs`, `error.rs`, `lua.rs`+`lua/{runtime,bridge}.rs` (`beca.*` API, XDG scripts + fallback, `on_load`/`on_unload` hooks).
- **`crates/beca-apps`**: `desktop_entry.rs` + `indexer.rs` (discovery/launch `.desktop`).
- **`crates/beca-ui`**: `ui/` (kit: `Theme` in-out, `SearchWindow`, `VoiceCaptureWindow`, `bar/`, `voice/`, `components/`), `src/{lib,ui_loader,theme}.rs` + `src/voice/` (capture/transcribe/quick/streaming/view).
- **`crates/beca-layer-shell`**: layer-shell surfaces (`bar.rs`, `launcher.rs`, `quick.rs`, `voice.rs`, `platform.rs`) — resident bar/standalone HUD windows.
- **Daemon**: unix socket JSON lines — `ping/version/hyprland.dispatch`/`hyprland.socket_paths`, `mod` (routing/spawn), `hyprland.subscribe` (stream `.socket2.sock`).
- **`beca.*` runtime**: `system` (notify/clipboard/exec/env/paths/log), `voice` (transcribe/config), `events` (on/off/emit), `runtime` (config/theme/paths/reload), `ui` (window/run/quit), `hyprland` (dispatch `{ok,response}`/read_events).

## Direction (target)
- Expand lifecycle/reload in living context and UI integration; evolve `beca.hyprland` (streaming `.socket2.sock` → `beca.events` in-process, daemon bridge).
- Daemon: surface management/lifecycle, layer-shell/Wayland, rich routing.
- Fully config-driven voice (title/flow/behavior via `beca-core`).

## Gotchas
- **`--hold` vs pipe**: pipe = stdout+exit (binds), no `--hold`. STT daemon `POST /transcribe` → `{ "text" }` (default `127.0.0.1:4100`, `VOICE_DAEMON_URL`).
- **Slint**: only `Window` generates a Rust struct; non-`Window` exports are pruned from `library.slint` to avoid noise — components are resolved by name at runtime from the embedded bundle.
- **Scripts:** Lua 5.4. Global `beca.*` namespace. `beca` loads `<mod>.lua` from `$XDG_CONFIG_HOME/beca/scripts/` with embedded fallback.
