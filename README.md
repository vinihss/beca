# beca — shell UI para Hyprland/Wayland (Rust + Slint + Lua)

**beca** é um shell UI modular para Hyprland/Wayland: CLI único, componentes Slint reutilizáveis (`ui-lib`), ações padrão em Rust e scripts **Lua** configuráveis. Temas e parâmetros ajustáveis **sem recompilar** (XDG). Merge de `shell-scripting-starter` incorporado.

## CLI
```bash
beca mod launcher:show                  # launcher de apps (SearchWindow + .desktop)
beca mod voice:show [--hold]            # captura/transcrição (UI)
beca mod voice:transcribe -i audio.wav  # transcreve arquivo (pipe/stdout)
beca run script.lua [args...]           # executa script com runtime beca.*
beca daemon                             # daemon IPC (unix socket) + bridge base
beca ping                               # verifica daemon
```
Config: `$XDG_CONFIG_HOME/beca/config.toml` · Temas: `$XDG_CONFIG_HOME/beca/themes/<nome>.toml` (precedência CLI > ENV > config > defaults).

## Rodar (workspace)
```bash
cargo run -p beca -- mod launcher:show
cargo run -p beca -- mod voice:show --hold
cargo run -p beca -- mod voice:transcribe -i /tmp/arquivo.wav
cargo build --release -p beca                     # → target/release/beca
# baseline/engines
cargo run -p voice-capture -- --help
cargo run -p desktop-app --bin sdk-run -- voice-sdk.luau [--test]
cargo test -p beca-core
```

## Crates
- `beca` (CLI), `beca-core` (config/tema XDG), `beca-apps` (apps/.desktop), `ui-lib` (kit: SearchWindow, VoiceCaptureWindow…), `voice-capture` (engine voz), `desktop-app`, `examples-runner`.

## Docs
- `ARCHITECTURE.md` — conceito, CLI, daemon Hyprland/Wayland (alvo), fases
- `AGENTS.md` — comandos/gotchas
- `docs/schema/` — `config.example.toml`, `theme.default.example.toml`
- `scripts/launcher.lua` — launcher de referência
