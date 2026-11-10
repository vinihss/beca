# Arquitetura e definições — **beca** (shell para Hyprland/Wayland)

> **Estado:** merge `beca` + `shell-scripting-starter` realizado (CLI `beca`, módulo `launcher`, serviço `beca-apps`, `SearchWindow` em `ui-lib`, `beca-core`). Nome: **`beca`** · CLI único · multi-módulo · XDG · temas/params sem recompilar · scripts **Lua 5.4**.

## 1. Norte (conceito)
**beca** é um **shell UI para Hyprland/Wayland**: binário/CLI único que expõe **módulos** (voice, launcher, futuros: bar, notifications…), invocado por binds do Hyprland, com **configuração e temas em runtime** (sem recompilar). Modelo "rofi+sh" elevado a shell modular.

| Peça | Papel |
|---|---|
| **CLI `beca`** | Entrypoint único; `beca mod <modulo>:<acao>` |
| **`ui-lib`** | Kit de componentes/janelas Slint (contratos estáveis) |
| **Módulos** | Features (voice, launcher, …) = UI da `ui-lib` + ações padrão Rust |
| **Scripts Lua** | Lógica/handlers do usuário (disco/XDG, fallback embutido) |
| **Ações padrão (Rust)** | Capacidades: record/transcribe, clipboard, app.close, notify, apps, compositor |
| **Config/tema (XDG)** | `config.toml` + `themes/*.toml` em runtime (`beca-core`) |
| **Daemon compositor** | (alvo) IPC Hyprland/Wayland + lifecycle (ver §5) |

**Princípio (do starter):** scripts descrevem comportamento; Rust detém operações privilegiadas, serviços, ciclo de vida e integração Linux; Slint detém renderização/interação. API de serviços **language-neutral** (reutilizável por runtimes).

**Limite Slint (`slint-build`):** estrutura compilada. Sem recompilar: cores/fontes/raios/espaçamentos, textos, endpoints/params, comportamento Lua/config.

## 2. CLI (contrato)
```bash
beca mod <modulo>:<acao> [args...] [--config <path>] [--theme <nome>]
```
Exemplos:
- `beca mod launcher:show` — launcher de aplicações (`SearchWindow`, busca/launch `.desktop`)
- `beca mod voice:show [--hold]` — captura/transcrição (UI)
- `beca mod voice:transcribe -i <wav>` — transcrição de arquivo (pipe/stdout)

Precedência: **CLI > ENV > `$XDG_CONFIG_HOME/beca/config.toml` > defaults**.

## 3. Configuração e temas (XDG, runtime)
- Base: `$XDG_CONFIG_HOME/beca/` (default `~/.config/beca/`) — `config.toml`, `themes/<nome>.toml`, `scripts/`.
- Tema sem recompilar: tokens injetados no `Theme` global na init (`Theme` `in-out`, aplicado por `beca`/engine voice via `beca-core`).
- Scripts Lua em disco com prioridade XDG, fallback embutido. Schemas: `docs/schema/config.example.toml`, `docs/schema/theme.default.example.toml`.

## 4. Estado atual (pós-merge)
```text
crates/
  beca/                   CLI `beca` (clap): mod launcher:show, mod voice:*
    src/runtime           base `beca.*` por domínio, load script XDG/fallback
    src/modules/launcher  beca.ui.search_window + beca.apps (Lua 5.4 embutido)
    src/modules/voice     nativo (lib engine voice-capture + beca-core config/tema)
  beca-core/              config/tema XDG, load+merge, structs (testado vs schemas)
  beca-apps/              serviço: descoberta/launch .desktop (de shell-scripting-starter)
  ui-lib/                 kit Slint: Theme, DesktopButton, InfoCard, MediaButton,
                          WaveformView, VoiceCaptureWindow, SearchWindow
  voice-capture/          engine voz (lib+bin): cpal/WAV/ureq, POST /transcribe→{text}, aplica tema
  desktop-app/            host/guia (MainWindow+/behavior.luau) + sdk-run (MVP sdk.*)
  examples-runner/        wizard/dashboard
docs/schema/              config/theme exemplos · scripts/launcher.lua
```
- Runtime `beca` (unificado, base): namespace global `beca.*` por domínio (`ui/apps/voice/events/system/hyprland`); scripts de módulo carregados de `$XDG_CONFIG_HOME/beca/scripts/<mod>.lua` com fallback embutido (`launcher.lua`). `desktop-app`/`sdk-run` mantém API `sdk.window/set/on/call/run` (MVP) → convergir para `beca.*`.
- `mlua` lua54 (vendored). Launcher funcional via `beca mod launcher:show`.

## 5. Daemon de comunicação Hyprland/Wayland (componente central, alvo)
Necessário para o shell ser integrado ao compositor (não apenas janelas soltas):

```text
Hyprland  <──IPC sockets──>  [beca daemon / compositor bridge]  <──unix socket──>  beca mod ... (cliente)
Wayland   <──protocolos──>        │  eventos + dispatch + layer-shell
                                  ▼
                         runtime/módulos + ui-lib (superfícies shell)
```
- **Hyprland IPC:** `$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket.sock` (comandos/dispatch/`hyprctl`) e `.socket2.sock` (eventos: workspace, janela ativa, monitor…).
- **Wayland/layer-shell:** superfícies de shell (bar/overlay/launcher) via `wlr-layer-shell` e protocolos relevantes (ancora/exclusividade) integradas às janelas Slint.
- **Modo daemon:** `beca daemon` (processo longo) mantém conexões/estado, expõe IPC local para o CLI, gerencia lifecycle/reload de módulos/scripts e multiplexa eventos. API de script: `beca.hyprland` / ações padrão de compositor.
- Crate alvo: `beca-compositor`/`beca-daemon` (serviço Rust privilegiado).

## 6. Migração (fases)
1. **Docs + schema + merge** — `beca` CLI, `beca-core`, `beca-apps`, `SearchWindow`, launcher/voice integrados. **(feito)**
2. **Consolidar voice** — módulo nativo (retirar delegação), capacidades `record/transcribe` no runtime.
3. **Tema runtime** — tokens `Theme` setáveis + aplicar tema carregado.
4. **Runtime unificado** — `beca.*` por domínio, scripts em disco (XDG), lifecycle/reload. **(base feita no `beca`: domínios + load XDG/fallback p/ launcher; falta expandir capacidades/reload e convergir `sdk-run`)**
5. **Daemon Hyprland/Wayland** — IPC compositor, layer-shell, `beca.hyprland`, binds/eventos.
6. **Módulos adicionais** — bar, notifications, etc.

## 7. Invariantes
- Pipe/binds: stdout = transcrição, sem `--hold`, exit estável. `POST /transcribe` → `{ "text" }`.
- UI `ui/`, Rust `src/`, scripts `scripts/` ou XDG. Serviços privilegiados em Rust, script orquestra.
- `Analise-Bilbioteca.md`/`lib-examples.md` = referência de padrões; código/config prevalecem.
