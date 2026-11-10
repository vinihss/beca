# AGENTS.md

## Project
- **beca**: shell UI para **Hyprland/Wayland** (Rust + [Slint](https://slint.rs/), scripts **Lua**). CLI único, multi-módulo, config/temas em runtime (XDG), sem recompilar para ajustar tema/params.
- Norte: `beca mod <modulo>:<acao>` · config `$XDG_CONFIG_HOME/beca/config.toml` · temas `themes/*.toml` · scripts `scripts/*.lua`. Ver `docs/architecture.md` e `docs/schema/`.
- **Workspace:** `crates/beca` (CLI/daemon), `crates/beca-core` (config/tema + runtime Lua), `crates/beca-apps` (.desktop), `crates/beca-ui` (kit Slint), `crates/beca-voice` (engine voz lib+bin). `mlua` = **lua54** (vendored).

## Commands
- `cargo run -p beca -- --help` / `beca mod --help`
- `beca mod launcher:show` — launcher de apps (`beca.ui.search_window` + `beca.apps`)
- `beca mod voice:show [--hold]` / `beca mod voice:transcribe -i <wav>` — voice nativo (engine `beca-voice` + config `beca-core`)
- `beca mod ...` roteia via daemon quando ativo (spawn/lifecycle; `--local` força local). `voice:transcribe` sempre local (pipe/stdout).
- `beca run <script.lua> [args...]` — executa script com runtime `beca.*` (path ou nome em `$XDG_CONFIG_HOME/beca/scripts/`); suporta UI via `beca.ui.window/run/quit`
- `beca daemon` — daemon IPC (`$XDG_RUNTIME_DIR/beca/beca.sock`); `beca ping` / `beca subscribe` (stream eventos Hyprland)
- `cargo build --release -p beca` · `cargo check` · `cargo test -p beca-core`
- Lint/format: `cargo fmt` / `cargo clippy`. Sem CI/testes extensos detectados.

## Architecture / wiring
- **Separação**: Slint/UI em `*/ui/`, Rust em `*/src/`, scripts em `scripts/` ou XDG.
- **`crates/beca`** (bin): `cli.rs`, `comandos.rs`+`comandos/{launcher,voice,runner}.rs`, `daemon.rs`+`daemon/{socket,hyprland}.rs`. Roteamento `beca mod` via daemon, `beca run`.
- **`crates/beca-core`**: `config.rs` (XDG/paths/load/merge), `tema.rs`, `erro.rs`, `lua.rs`+`lua/{runtime,bridge}.rs` (API `beca.*`, scripts XDG+fallback, hooks `on_load`/`on_unload`).
- **`crates/beca-apps`**: `desktop_entry.rs` + `indexador.rs` (descoberta/launch `.desktop`).
- **`crates/beca-ui`**: `ui/*.slint` (kit: `Theme` in-out, `SearchWindow`, `VoiceCaptureWindow` etc.), `src/{lib,search_window,voice_window}.rs`.
- **`crates/beca-voice`**: `captura.rs` (mic/UI) + `transcricao.rs` (STT `POST /transcribe`→`{text}`, clipboard/notify), lib+bin.
- **Daemon**: unix socket JSON lines — `ping/version/hyprland.dispatch`/`hyprland.socket_paths`, `mod` (routing/spawn), `hyprland.subscribe` (stream `.socket2.sock`).
- **Runtime `beca.*`**: `system` (notify/clipboard/exec/env/paths/log), `voice` (transcribe/config), `events` (on/off/emit), `runtime` (config/theme/paths/reload), `ui` (window/run/quit), `hyprland` (dispatch `{ok,response}`/read_events).

## Direção (alvo)
- Ampliar lifecycle/reload em contexto vivo e integração UI; evoluir `beca.hyprland` (streaming `.socket2.sock` → `beca.events` in-process, daemon bridge).
- Daemon: gerência de superfícies/lifecycle, layer-shell/Wayland, roteamento rico.
- Voice config-driven completo (título/fluxo/comportamento via `beca-core`).

## Gotchas
- **`--hold` vs pipe**: pipe = stdout+exit (binds), sem `--hold`. Daemon STT `POST /transcribe` → `{ "text" }` (default `127.0.0.1:4100`, `VOICE_DAEMON_URL`).
- **Slint**: só `Window` gera struct Rust; exports não-`Window` → warnings esperados. Tema runtime via tokens `in-out`.
- **Scripts:** Lua 5.4. Namespace global `beca.*`. `beca` carrega `<mod>.lua` de `$XDG_CONFIG_HOME/beca/scripts/` com fallback embutido.
