# Architecture and definitions — **beca** (Hyprland/Wayland shell)

> **State:** `beca` + `shell-scripting-starter` merged (CLI `beca`, `launcher` module, `beca-apps` service, `SearchWindow` in `beca-ui`, `beca-core`). Name: **`beca`** · single CLI · multi-module · XDG · themes/params without recompiling · **Lua 5.4** scripts.

## 1. North star (concept)
**beca** is a **Hyprland/Wayland UI shell**: a single binary/CLI exposing **modules** (voice, launcher, future: bar, notifications…), invoked by Hyprland binds, with **runtime configuration and themes** (no recompile). "rofi+sh" lifted into a modular shell.

| Piece | Role |
|---|---|
| **`beca` CLI** | Single entrypoint; `beca mod <module>:<action>` |
| **`beca-ui`** | Slint component/window kit (stable contracts) |
| **Modules** | Features (voice, launcher, …) = `beca-ui` UI + Rust built-in actions |
| **Lua scripts** | User logic/handlers (disk/XDG, embedded fallback) |
| **Built-in actions (Rust)** | Capabilities: record/transcribe, clipboard, app.close, notify, apps, compositor |
| **Config/theme (XDG)** | `config.toml` + `themes/*.toml` at runtime (`beca-core`) |
| **Compositor daemon** | (target) Hyprland/Wayland IPC + lifecycle (see §5) |

**Principle (from the starter):** scripts describe behavior; Rust holds privileged operations, services, lifecycle and Linux integration; Slint holds rendering/interaction. Service API **language-neutral** (reusable by runtimes).

**Slint limit (`slint-build`):** compiled structure. No recompile: colors/fonts/radius/spacing, texts, endpoints/params, Lua behavior/config.

## 2. CLI (contract)
```bash
beca mod <module>:<action> [args...] [--config <path>] [--theme <name>]
```
Examples:
- `beca mod launcher:show` — application launcher (`SearchWindow`, `.desktop` search/launch)
- `beca mod voice:show [--hold]` — capture/transcription (UI)
- `beca mod voice:transcribe -i <wav>` — file transcription (pipe/stdout)

Precedence: **CLI > ENV > `$XDG_CONFIG_HOME/beca/config.toml` > defaults**.

## 3. Configuration and themes (XDG, runtime)
- Base: `$XDG_CONFIG_HOME/beca/` (default `~/.config/beca/`) — `config.toml`, `themes/<name>.toml`, `scripts/`.
- Theme without recompiling: tokens injected into the global `Theme` at init (`Theme` `in-out`, applied by `beca`/voice engine via `beca-core`).
- Lua scripts on disk with XDG priority, embedded fallback. Schemas: `docs/schema/config.example.toml`, `docs/schema/theme.default.example.toml`.

## 4. Current state (post-merge)
```text
crates/
  beca/                 CLI/daemon: cli, comandos/{launcher,voice,bar,runner}, daemon/{socket,hyprland}
  beca-core/            XDG config/theme, error, lua/{runtime,bridge} (beca.* API)
  beca-apps/            .desktop: desktop_entry, indexer (search/launch)
  beca-ui/              Slint kit (ui/): Theme, SearchWindow, VoiceCaptureWindow + voice engine
  beca-layer-shell/     wlr-layer-shell surfaces (bar, launcher, quick HUD, voice window)
docs/                   architecture.md, agents.md, hyprland_ipc.md, ui-design-system.md, schema/
examples/               example Lua scripts
scripts/                reference scripts (launcher.lua)
docs/schema/            config/theme examples · scripts/launcher.lua
```
- `beca` runtime (unified): global `beca.*` namespace by domain (`ui/apps/voice/events/system/hyprland` + `runtime/hooks`); module scripts from `$XDG_CONFIG_HOME/beca/scripts/<mod>.lua` with embedded fallback. Capabilities: `beca.system`, `beca.voice` (transcribe/config), `beca.events` (on/off/emit), `beca.runtime` (config/theme/paths/reload), `beca.ui.window/run/quit`, `beca.hyprland` (dispatch/exec `{ok,response}` via `.socket.sock`, `read_events` via `.socket2.sock`), `on_load`/`on_unload` hooks. Legacy `beca run` → converge/retire.
- `mlua` lua54 (vendored). Working launcher via `beca mod launcher:show`.

## 5. Hyprland/Wayland communication daemon (central component, in progress)
- **Implemented base (`beca daemon`)**: long-lived process with local Unix socket IPC (`$XDG_RUNTIME_DIR/beca/beca.sock`, JSON lines), multi-client (thread per connection).
  - Requests: `ping`, `version`, `hyprland.dispatch`, `hyprland.socket_paths`, `mod` (routing: `voice:transcribe` in-process preserved; others spawned via daemon, `--local`/`BECA_NO_DAEMON` bypass), `hyprland.subscribe` (stream `.socket2.sock` events as JSON lines).
  - Clients: `beca ping`, `beca subscribe`; `beca mod ...` routes through the daemon when active. `beca.hyprland` runtime (`dispatch`/`read_events`).
- **Target (to complete):** streaming bridge → `beca.events` in-process, lifecycle/reload/surface management in the daemon, layer-shell/Wayland, event-oriented `beca.hyprland` API.

```text
Hyprland  <──IPC sockets──>  [beca daemon / compositor bridge]  <──unix socket──>  beca mod ... (client)
Wayland   <──protocols──>        │  events + dispatch + layer-shell
                                 ▼
                        runtime/modules + beca-ui (shell surfaces)
```
- **Hyprland IPC:** `$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/.socket.sock` (commands/dispatch/`hyprctl`) and `.socket2.sock` (events: workspace, active window, monitor…).
- **Wayland/layer-shell:** shell surfaces (bar/overlay/launcher) via `wlr-layer-shell` and relevant protocols integrated into the Slint windows.
- **Daemon mode:** `beca daemon` keeps connections/state, exposes local IPC, manages lifecycle/reload and multiplexes events. Script API: `beca.hyprland` / built-in compositor actions.

## 6. Migration (phases)
1. **Docs + schema + merge/organization** — CLI, `beca-*` crates, docs/schema, XDG, launcher/voice integrated. **(done)**
2. **Consolidate voice** — native module (remove delegation), `record/transcribe` capabilities in the runtime. **(done)**
3. **Runtime theme** — settable `Theme` tokens + apply loaded theme. **(done)**
4. **Unified runtime** — `beca.*` by domain, scripts on disk (XDG), lifecycle/reload. **(expanded: `system/voice/events/runtime/hooks`, `beca.ui.window/run/quit`, `beca run`, reload/basic hooks, bridge in `beca-core`; missing in-context living lifecycle and hyprland→events streaming in-process)**
5. **Hyprland/Wayland daemon** — compositor IPC, layer-shell, `beca.hyprland`, binds/events.
6. **UI design system** — maturação da UI (tokens semânticos, primitivas normalizadas, catálogo/storybook nativo separado, identidade reavaliada). Plano e decisões em `docs/ui-design-system.md` (**Fase 0 em preparação**).
7. **Additional modules** — bar, notifications, etc.

## 7. Invariants
- Pipe/binds: stdout = transcription, no `--hold`, stable exit. `POST /transcribe` → `{ "text" }`.
- UI `ui/`, Rust `src/`, scripts `scripts/` or XDG. Privileged services in Rust, script orchestrates.
- Code and config prevail over this document.
