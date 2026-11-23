# beca — UI shell for Hyprland/Wayland (Rust + Slint + Lua)

**beca** is a modular UI shell for Hyprland/Wayland: a single CLI, reusable Slint components (`beca-ui`), built-in Rust actions and configurable **Lua** scripts. Themes and parameters are adjustable **without recompiling** (XDG).

## CLI
```bash
beca mod launcher:show                  # app launcher (SearchWindow + .desktop)
beca mod voice:show [--hold]            # capture/transcription (UI)
beca mod voice:quick                    # quick overlay (oscilloscope) for the bar mic button
beca mod voice:transcribe -i audio.wav  # transcribe a file (pipe/stdout, local)
beca mod ... --local                    # force local execution (no daemon)
beca run script.lua [args...]           # run a script with the beca.* runtime
beca daemon                             # IPC daemon (unix socket) + Hyprland bridge
beca ping                               # check the daemon
beca subscribe                          # stream Hyprland events via the daemon
```
Config: `$XDG_CONFIG_HOME/beca/config.toml` · Themes: `$XDG_CONFIG_HOME/beca/themes/<name>.toml` · Scripts: `$XDG_CONFIG_HOME/beca/scripts/` (precedence CLI > ENV > config > defaults).

## Run (workspace)
```bash
cargo run -p beca-cli -- mod launcher:show
cargo run -p beca-cli -- mod voice:show --hold
cargo run -p beca-cli -- mod voice:transcribe -i /tmp/file.wav
cargo run -p beca-cli -- run examples/voice_sdk.luau
cargo build --release -p beca-cli                     # → target/release/beca
cargo test -p beca-core
```

## Crates
- `beca-cli` — binário `beca`: CLI (clap) e dispatch fino de comandos
- `beca-daemon` — protocolo IPC tipado, cliente, controladores e handlers
- `beca-process` — descoberta de processos, liveness e spawn destacado
- `beca-bar` — barra (fallback de janela), geometria Hyprland e widgets
- `beca-launcher` — launcher residente com Lua e UI
- `beca-voice` — sessões de voz, transcrição e servidor residente
- `beca-core` — XDG config/theme, `BarProperties`, Lua runtime (`lua/runtime`, `lua/bridge`, `lua/runner`, `beca.*` API)
- `beca-apps` — `.desktop` discovery/launch
- `beca-ui` — Slint kit (`ui/`: `components/`, `bar/`, `voice/`, `search/`) + voice engine (capture/transcribe/quick)
- `beca-layer-shell` — wlr-layer-shell surfaces (resident bar, launcher and voice/quick HUDs)

## Docs
- `docs/architecture.md` — concept, structure, Hyprland/Wayland daemon, phases
- `docs/agents.md` — commands/gotchas
- `docs/hyprland_ipc.md` — Hyprland IPC notes
- `docs/build-dev.md` — local dev/build optimization analysis
- `docs/build-performance-audit.md` — read-only diagnosis of workspace, Cargo, Slint, build times and hardware
- `docs/schema/` — `config.example.toml`, `theme.default.example.toml`
- `scripts/launcher.lua` — reference launcher
- `examples/` — example Lua scripts
