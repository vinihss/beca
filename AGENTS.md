# AGENTS.md

## Project
- **beca**: shell UI para **Hyprland/Wayland** (Rust + [Slint](https://slint.rs/), scripts **Lua**). CLI único, multi-módulo, config/temas em runtime (XDG), sem recompilar para ajustar tema/params. Merge com `shell-scripting-starter` incorporado.
- Norte: `beca mod <modulo>:<acao>` (ex.: `beca mod voice:show`, `beca mod launcher:show`) · config `$XDG_CONFIG_HOME/beca/config.toml` · temas `themes/*.toml`. Ver `ARCHITECTURE.md` e `docs/schema/`.
- **Workspace:** `crates/beca` (CLI bin), `crates/beca-core` (config/tema XDG), `crates/beca-apps` (descoberta/launch `.desktop`), `crates/ui-lib` (kit: inclui `SearchWindow`, `VoiceCaptureWindow`), `crates/voice-capture` (engine voz, lib+bin), `crates/desktop-app`, `crates/examples-runner`. `mlua` = **lua54** (vendored).

## Commands
- `cargo run -p beca -- --help` / `cargo run -p beca -- mod --help`
- `cargo run -p beca -- mod launcher:show` — launcher de apps (`SearchWindow` + `beca-apps`, `beca.ui/apps`)
- `cargo run -p beca -- mod voice:show [--hold]` / `beca mod voice:transcribe -i <wav>` — módulo voice **nativo** (lib `voice-capture` engine + config `beca-core`)
- `cargo run -p beca -- run <script.lua> [args...]` — executa script Lua com runtime `beca.*` (path ou nome em `$XDG_CONFIG_HOME/beca/scripts/`); suporta UI via `beca.ui.window/run/quit` (ex.: `VoiceCaptureWindow`)
- `cargo run -p beca -- daemon` — daemon IPC (unix socket `$XDG_RUNTIME_DIR/beca/beca.sock`); `beca ping` verifica
- `cargo build --release -p beca` → `target/release/beca`
- `cargo check` / `cargo build` (workspace) · `cargo test -p beca-core`
- Baseline: `cargo run -p voice-capture -- ...` (bin compat), `cargo run -p desktop-app`, `cargo run -p desktop-app --bin sdk-run -- voice-sdk.luau [--test]` (legado, compat `beca.*`/`sdk.*`; prefer `beca run`), `cargo run -p examples-runner --bin wizard|dashboard`
- Lint/format: `cargo fmt` / `cargo clippy`. Sem CI/testes detectados.

## Architecture / wiring
- **Separação**: Slint/UI em `*/ui/`, Rust em `*/src/`, scripts em `*/scripts/` ou XDG.
- **`crates/beca`** (CLI): `clap` → `beca mod <modulo>:<acao>`, `beca run <script>`, `beca daemon`/`ping`; `runtime` unificado `beca.*` (`ui/apps/voice/events/system/hyprland` + `runtime/hooks`), scripts XDG com fallback embutido, hooks `on_load`/`on_unload`. APIs: `beca.system`, `beca.voice` (transcribe/config), `beca.events`, `beca.runtime`, `beca.ui.window/run/quit`, `beca.hyprland` (dispatch/exec `{ok,response}`, `read_events`). Daemon: IPC unix socket (`$XDG_RUNTIME_DIR/beca/beca.sock`, JSON lines; `ping/version/hyprland.dispatch`/`hyprland.socket_paths`). Dispatch `launcher` e `voice` nativo.
- **`crates/beca-core`**: XDG paths, load+merge config/tema (CLI>ENV>file>defaults), schemas em `docs/schema/`.
- **`crates/beca-apps`**: descoberta `.desktop` + `search`/`launch` (serviço, de `shell-scripting-starter`).
- **`crates/ui-lib`** (kit): `Theme`, `DesktopButton`, `InfoCard`, `MediaButton`, `WaveformView`, `VoiceCaptureWindow`, `SearchWindow`. Contratos estáveis (props/callbacks). Tokens `Theme` **`in-out`** (runtime-settable); `beca`/engine voice aplicam tema de `beca-core` na init.
- **`crates/voice-capture`**: engine voz (lib+bin, `cpal/hound/ureq`, `POST /transcribe` → `{text}`), usado nativamente pelo módulo `voice` e pelo bin compat.
- **`crates/desktop-app`**: host/guia (`MainWindow`+`/behavior.luau`) e `sdk-run` legado (compat `beca.*`/`sdk.*`, prefer `beca run`).

## Direção (alvo)
- Aprofundar runtime unificado: ampliar `beca.voice/events/system` (hooks/lifecycle, reload seguro em contexto vivo, integração UI), evoluir `beca.hyprland` (streaming contínuo `.socket2.sock` → `beca.events`, integração com daemon), e completar convergência/aposentadoria de `desktop-app`/`sdk-run`. Voice: UI config-driven parcial (`ui.status_recording/status_transcribing`; título/fluxo evoluindo).
- **Daemon de comunicação Hyprland/Wayland**: processo longo (`beca daemon`) com IPC local (unix socket) para `beca mod ...`; integra Hyprland (`$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket.sock` comandos, `.socket2.sock` eventos) e Wayland/layer-shell para superfícies do shell. Expor `beca.hyprland`/ações padrão.
- Tema/params runtime aplicados ao `Theme` (tokens setáveis). Consolidar `voice` como módulo totalmente config-driven (textos/tema/comportamento via `beca-core`, sem hardcode).

## Gotchas
- **`--hold` vs pipe**: pipe = stdout+exit (binds Hyprland), sem `--hold`. Daemon `POST /transcribe` → `{ "text" }` (default `127.0.0.1:4100`, env `VOICE_DAEMON_URL`).
- **Slint**: só `Window` gera struct Rust; exports não-`Window` → warnings esperados. Estrutura compilada; tema runtime exige tokens setáveis.
- **Scripts:** Lua 5.4 (`mlua` vendored). `beca` carrega `<mod>.lua` de `$XDG_CONFIG_HOME/beca/scripts/` com fallback embutido; namespace global `beca.*`. `voice` nativo no CLI (lib engine). Preferir código/config; `Analise-Bilbioteca.md`/`lib-examples.md` = referência de padrões.
