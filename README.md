# beca — shell UI para Hyprland/Wayland (Rust + Slint + Lua)

**beca** é um shell UI modular para Hyprland/Wayland: CLI único, componentes Slint reutilizáveis (`beca-ui`), ações padrão em Rust e scripts **Lua** configuráveis. Temas e parâmetros ajustáveis **sem recompilar** (XDG).

## CLI
```bash
beca mod launcher:show                  # launcher de apps (SearchWindow + .desktop)
beca mod voice:show [--hold]            # captura/transcrição (UI)
beca mod voice:transcribe -i audio.wav  # transcreve arquivo (pipe/stdout, local)
beca mod ... --local                    # força execução local (sem daemon)
beca run script.lua [args...]           # executa script com runtime beca.*
beca daemon                             # daemon IPC (unix socket) + bridge Hyprland
beca ping                               # verifica daemon
beca subscribe                          # stream de eventos Hyprland via daemon
```
Config: `$XDG_CONFIG_HOME/beca/config.toml` · Temas: `$XDG_CONFIG_HOME/beca/themes/<nome>.toml` · Scripts: `$XDG_CONFIG_HOME/beca/scripts/` (precedência CLI > ENV > config > defaults).

## Rodar (workspace)
```bash
cargo run -p beca -- mod launcher:show
cargo run -p beca -- mod voice:show --hold
cargo run -p beca -- mod voice:transcribe -i /tmp/arquivo.wav
cargo run -p beca -- run examples/voice_sdk.luau
cargo build --release -p beca                     # → target/release/beca
cargo test -p beca-core
```

## Crates
- `beca` — CLI/daemon (`cli`, `comandos/`, `daemon/`)
- `beca-core` — config/tema XDG, runtime Lua (`lua/runtime`, `lua/bridge`, API `beca.*`)
- `beca-apps` — descoberta/launch `.desktop`
- `beca-ui` — kit Slint (`ui/*.slint`, `SearchWindow`, `VoiceCaptureWindow`)
- `beca-voice` — engine voz (captura/transcrição, lib+bin)

## Docs
- `docs/architecture.md` — conceito, estrutura, daemon Hyprland/Wayland, fases
- `docs/agents.md` — comandos/gotchas
- `docs/hyprland_ipc.md` — notas IPC Hyprland
- `docs/schema/` — `config.example.toml`, `theme.default.example.toml`
- `scripts/launcher.lua` — launcher de referência
- `examples/` — scripts Lua de exemplo
