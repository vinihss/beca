# Arquitetura e definições — **beca** (shell para Hyprland/Wayland)

> **Estado:** merge `beca` + `shell-scripting-starter` realizado (CLI `beca`, módulo `launcher`, serviço `beca-apps`, `SearchWindow` em `beca-ui`, `beca-core`). Nome: **`beca`** · CLI único · multi-módulo · XDG · temas/params sem recompilar · scripts **Lua 5.4**.

## 1. Norte (conceito)
**beca** é um **shell UI para Hyprland/Wayland**: binário/CLI único que expõe **módulos** (voice, launcher, futuros: bar, notifications…), invocado por binds do Hyprland, com **configuração e temas em runtime** (sem recompilar). Modelo "rofi+sh" elevado a shell modular.

| Peça | Papel |
|---|---|
| **CLI `beca`** | Entrypoint único; `beca mod <modulo>:<acao>` |
| **`beca-ui`** | Kit de componentes/janelas Slint (contratos estáveis) |
| **Módulos** | Features (voice, launcher, …) = UI da `beca-ui` + ações padrão Rust |
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
  beca/                 CLI/daemon: cli, comandos/{launcher,voice,runner}, daemon/{socket,hyprland}
  beca-core/            config/tema XDG, erro, lua/{runtime,bridge} (API beca.*)
  beca-apps/            .desktop: desktop_entry, indexador (search/launch)
  beca-ui/              kit Slint (ui/*.slint): Theme, SearchWindow, VoiceCaptureWindow...
  beca-voice/           engine voz lib+bin: captura, transcricao
docs/                   architecture.md, agents.md, hyprland_ipc.md, schema/
examples/               scripts Lua de exemplo
scripts/                scripts de referência (launcher.lua)
docs/schema/              config/theme exemplos · scripts/launcher.lua
```
- Runtime `beca` (unificado): namespace global `beca.*` por domínio (`ui/apps/voice/events/system/hyprland` + `runtime/hooks`); scripts de módulo de `$XDG_CONFIG_HOME/beca/scripts/<mod>.lua` com fallback embutido. Capacidades: `beca.system`, `beca.voice` (transcribe/config), `beca.events` (on/off/emit), `beca.runtime` (config/theme/paths/reload), `beca.ui.window/run/quit`, `beca.hyprland` (dispatch/exec `{ok,response}` via `.socket.sock`, `read_events` via `.socket2.sock`), hooks `on_load`/`on_unload`. `beca run` legado → convergir/aposentar.
- `mlua` lua54 (vendored). Launcher funcional via `beca mod launcher:show`.

## 5. Daemon de comunicação Hyprland/Wayland (componente central, em andamento)
- **Base implementada (`beca daemon`)**: processo longo com IPC local unix socket (`$XDG_RUNTIME_DIR/beca/beca.sock`, JSON lines), multi-cliente (thread por conexão).
  - Requests: `ping`, `version`, `hyprland.dispatch`, `hyprland.socket_paths`, `mod` (routing: `voice:transcribe` local/preservado; outros spawn via daemon, `--local`/`BECA_NO_DAEMON` bypass), `hyprland.subscribe` (stream eventos de `.socket2.sock` como JSON lines).
  - Clientes: `beca ping`, `beca subscribe`; `beca mod ...` roteia via daemon quando ativo. Runtime `beca.hyprland` (`dispatch`/`read_events`).
- **Alvo (completar):** bridge streaming → `beca.events` in-process, lifecycle/reload/gerência de superfícies no daemon, layer-shell/Wayland, API `beca.hyprland` orientada a eventos.

```text
Hyprland  <──IPC sockets──>  [beca daemon / compositor bridge]  <──unix socket──>  beca mod ... (cliente)
Wayland   <──protocolos──>        │  eventos + dispatch + layer-shell
                                  ▼
                         runtime/módulos + beca-ui (superfícies shell)
```
- **Hyprland IPC:** `$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket.sock` (comandos/dispatch/`hyprctl`) e `.socket2.sock` (eventos: workspace, janela ativa, monitor…).
- **Wayland/layer-shell:** superfícies de shell (bar/overlay/launcher) via `wlr-layer-shell` e protocolos relevantes integrados às janelas Slint.
- **Modo daemon:** `beca daemon` mantém conexões/estado, expõe IPC local, gerencia lifecycle/reload e multiplexa eventos. API de script: `beca.hyprland` / ações padrão de compositor.

## 6. Migração (fases)
1. **Docs + schema + merge/organização** — CLI, crates `beca-*`, docs/schema, XDG, launcher/voice integrados. **(feito)**
2. **Consolidar voice** — módulo nativo (retirar delegação), capacidades `record/transcribe` no runtime. **(feito, `beca-voice`)**
3. **Tema runtime** — tokens `Theme` setáveis + aplicar tema carregado. **(feito)**
4. **Runtime unificado** — `beca.*` por domínio, scripts em disco (XDG), lifecycle/reload. **(expandido: `system/voice/events/runtime/hooks`, `beca.ui.window/run/quit`, `beca run`, reload/hooks básicos, bridge em `beca-core`; falta lifecycle em contexto vivo e streaming hyprland→eventos in-process)**
5. **Daemon Hyprland/Wayland** — IPC compositor, layer-shell, `beca.hyprland`, binds/eventos.
6. **Módulos adicionais** — bar, notifications, etc.

## 7. Invariantes
- Pipe/binds: stdout = transcrição, sem `--hold`, exit estável. `POST /transcribe` → `{ "text" }`.
- UI `ui/`, Rust `src/`, scripts `scripts/` ou XDG. Serviços privilegiados em Rust, script orquestra.
- `Analise-Bilbioteca.md`/`lib-examples.md` = referência de padrões; código/config prevalecem.
