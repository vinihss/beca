# Beca — Workspace and Build Performance Audit

> Diagnóstico **read-only** realizado em 2026-11-23. A auditoria em si não modificou arquivos; `git status --short` antes e depois foi idêntico. Números marcados como **[medido]** vêm de `target/cargo-timings/cargo-timing.html`; os demais são **[estimado]** ou deduzidos de análise estática. Mudanças aplicadas depois da auditoria e suas medições estão na [§13](#13-aplicação-e-medições-2026-11-23).

## 1. Escopo e método

- Repositório inteiro (`/home/vinicius/beca`); nenhum subdiretório foi informado como filtro.
- Comandos Cargo executados com `--locked --offline`.
- Fontes: `cargo metadata --format-version 1 --no-deps`, `cargo tree --workspace` / `-d` / `-e features`, todos os `Cargo.toml`, `build.rs`, `.cargo/config.toml`, relatório de timings, `git status`.

## 2. Arquitetura atual

### Workspace

- **Um único workspace**, `resolver = "3"`, `edition = 2024`, `version = 0.1.0`.
- **12 members** e **12 default members**: não existe `default-members`, então `cargo check`/`build` na raiz compila também catálogo e painel.
- Todos os diretórios com `Cargo.toml` em `crates/` estão declarados; nada órfão.

### Targets

| Tipo | Crate | Target | Papel |
|---|---|---|---|
| **Shell real** | `beca-cli` | `bin beca` | Único entry point: `src/main.rs` → `commands::execute(cli::Cli::parse())` |
| Ferramenta de dev | `beca-ui-catalog` | `bin beca-ui-catalog` | Storybook Slint |
| Experimento | `beca-panel` | `bin beca-panel` | Painel standalone |
| Lib | `beca-core` | `lib` | XDG config/theme, `BarProperties`, Lua runtime/bridge/runner |
| Lib | `beca-ui` | `lib` | Kit Slint + motor de voz (capture/transcribe/quick/streaming) |
| Lib | `beca-daemon` | `lib` | IPC JSON-lines tipado, cliente, lifecycle, handlers, Hyprland |
| Lib | `beca-process` | `lib` | `/proc`, liveness, spawn destacado, limpeza de runtime |
| Lib | `beca-bar` | `lib` | Barra, geometria Hyprland, widgets |
| Lib | `beca-launcher` | `lib` | Launcher residente (Lua + Slint + `.desktop`) |
| Lib | `beca-voice` | `lib` | Sessões de voz e servidor residente |
| Lib | `beca-apps` | `lib` | Descoberta/launch de `.desktop` |
| Lib | `beca-layer-shell` | `lib` | Superfícies layer-shell + platform Slint |

`examples/` contém scripts `.luau`/`.slint`, não crates. Existem **3 build scripts**, todos com `slint-build`: `beca-ui`, `beca-ui-catalog`, `beca-panel`.

### Direção das dependências

```text
beca-cli ──┬─> beca-core ──┬─> beca-apps
           │               ├─> beca-ui ───────┐
           │               └─> slint, slint-interpreter, mlua
           ├─> beca-process
           ├─> beca-daemon ─┬─> beca-core, beca-ui, beca-apps, beca-process
           │                └─> beca-voice ──> beca-core, beca-ui, beca-layer-shell
           ├─> beca-bar ──────> beca-core, beca-ui, beca-layer-shell, slint-interpreter
           └─> beca-launcher ─> beca-core, beca-ui, beca-apps, beca-layer-shell, mlua, slint

beca-layer-shell ─> beca-ui, beca-core, slint, smithay-client-toolkit, wayland-client, calloop
beca-panel ──> beca-ui, slint (+ slint-build)
beca-ui-catalog ─> beca-ui, slint (+ slint-build)
```

- **Não há ciclo Cargo** [verificado].
- **Fronteira de camadas frouxa** [verificado]: `beca-core` depende de `beca-ui` + `slint` + `beca-apps`, embora não haja uso de `slint::` nem `beca_apps::` em `crates/beca-core/src`. O `slint_interpreter` **é** usado (`lua/bridge.rs`) para janelas Lua, então esse não deve ser removido.
- Outros usos duvidosos por varredura de texto [estimado]: `clap`/`serde` em `beca-ui`; `calloop`/`serde_json` em `beca-layer-shell`; `beca-ui` em `beca-panel` (o bin usa o `include_modules!` do próprio `panel.slint`, não `beca_ui::AppWindow`).

## 3. Configuração Cargo

- `profile.dev` **no diagnóstico**: `opt-level = 1` (workspace **e** dependências), sem `debug` explícito → herda `debug = 2` (completo). Configuração alterada depois; ver §13.
- `profile.release`: `lto = "thin"`, `codegen-units = 16`. Comentário já orienta `lto=full, codegen-units=1` apenas para packaging final via `--config`.
- `test`/`bench`/`doc`: defaults. Não há `[profile.dev.package."*"]`.
- **Incremental ativo** por default [verificado]: `target/debug/incremental` ≈ 2,3 GiB.
- `.cargo/config.toml`: `-C link-arg=-fuse-ld=mold` para `x86_64-unknown-linux-gnu` (e blocos MSVC). A rustflag vale para **todos** os profiles, apesar do comentário mencionar release.
- `anyhow = "1"` estava literal em 8 crates em vez de centralizado em `[workspace.dependencies]` — centralizado em 23/11 (§13).
- `slint-build` está corretamente em `[build-dependencies]` nos 3 crates com `build.rs`.
- **Não existe `[dev-dependencies]`** no workspace. Nenhuma dependência opcional e nenhuma feature local declarada.

## 4. Grafo de dependências

### Quem puxa o quê

- → `beca-core`: bar, cli, daemon, launcher, layer-shell, voice (**6**).
- → `beca-ui`: bar, core, daemon, launcher, layer-shell, panel, catalog, voice (**8**).
- → `slint`: core, launcher, layer-shell, panel, ui, catalog, voice (**7**).
- → `slint-interpreter`: bar, core, ui, voice (**4**).
- → `slint-build`: ui, catalog, panel (**3**).

### Versões duplicadas [verificado via `cargo tree -d`]

- `smithay-client-toolkit` 0.18.1 (nosso) vs 0.19.2 (via Slint/winit).
- `wayland-protocols` 0.31.2 vs 0.32.13; `wayland-protocols-wlr` 0.2.0 vs 0.3.12.
- `calloop` 0.12.4 vs 0.13.0; `calloop-wayland-source` 0.2 vs 0.3.
- `syn` 2.0.119 vs 3.0.9; `thiserror` 1/2; `rustix` 0.38/1.1; `hashbrown` 0.14–0.17; `shlex` 1/2; `base64` 0.22/0.23; `tiny-skia` 0.11/0.12; `webpki-roots` 0.26/1.0.
- `i-slint-compiler` aparece 2× no grafo de features, mas na mesma versão (1.18.1).

Causa provável: pilha Wayland própria convivendo com a pilha do Slint 1.18, além de gerações diferentes de proc-macros.

### Árvores transitivas maiores [verificado]

```text
slint                 950 nós
slint-interpreter     913 nós
  i-slint-backend-selector 873
  i-slint-compiler         339
slint-build           336
ureq                  115   (rustls, webpki-roots, base64)
mlua                   32   (lua-src vendored via cc)
wayland-client         30
clap                   23
cpal                   10   (alsa-sys via pkg-config)
```

### Features Slint [verificado]

`slint`/`slint-interpreter` 1.18.1 usam todos os defaults:

```toml
default = ["std", "backend-default", "renderer-femtovg", "renderer-software",
           "accessibility", "compat-1-2", "system-tray"]
```

`backend-default` → `backend-winit` → **Wayland + X11**. Confirmado no grafo: `i-slint-backend-winit feature "x11"`, `feature "wayland"`, `renderer-femtovg`, `renderer-software`, `accesskit_unix`, `zbus`. O `features = ["compat-1-2", "renderer-software"]` em `beca-layer-shell` é redundante com os defaults.

**Correção após a aplicação (§13):** remover os defaults e manter só `backend-winit-wayland` elimina femtovg/`glutin`, accessibility/`accesskit` e system-tray, **mas não elimina `x11rb` nem `zbus`**. No Slint 1.18.1, `i-slint-backend-winit` declara `zbus` como dependência não opcional no Linux e `arboard` traz `x11rb` incondicionalmente em Unix. A estimativa inicial de economia com X11/accessibility estava otimista; a parte de femtovg/accesskit foi confirmada.

## 5. Slint e UI

- **Estático:** `slint::include_modules!()` em `beca-ui/src/lib.rs`, `beca-ui-catalog/src/main.rs` e `beca-panel/src/main.rs`, cada um com seu `slint-build`.
- **Dinâmico:** `beca-ui/src/ui_loader.rs` usa `slint_interpreter::{Compiler, ComponentDefinition, ComponentInstance}` com cache, assinatura de arquivos e fallback XDG/embedado (`include!(concat!(env!("OUT_DIR"), "/ui_bundle.rs"))`). `beca-core/src/lua/bridge.rs` expõe isso como `beca.ui.window/run/quit`. **Remover o interpreter quebraria o contrato de UI trocável sem rebuild.**
- `catalog.slint` e `panel.slint` importam de `../../beca-ui/ui/library.slint`: o mesmo componente é compilado **3×** em builds estáticos, além da recompilação em runtime pelo interpreter.
- `beca-ui/build.rs` emite `rerun-if-changed=ui` e um por arquivo; catálogo/painel emitem `rerun-if-changed=../beca-ui/ui`. Portanto uma edição em `crates/beca-ui/ui/components/*.slint` invalida `beca-ui` e os dois bins de dev [verificado].
- Catálogo/painel são bins separados, mas **default members** e herdam interpreter, áudio, mlua e ureq mesmo sem usar voz.

## 6. Ambiente de build [verificado]

| Item | Valor |
|---|---|
| rustc / cargo | 1.98.1 (stable), host/target `x86_64-unknown-linux-gnu` |
| CPU | Intel i7-7500U @ 2,70 GHz — 2 núcleos / 4 threads (`nproc = 4`) |
| RAM | 19 GiB total, 7,9 GiB disponíveis, 7,2 GiB em buff/cache |
| Swap | `/dev/zram0` 4 GiB, 2,7 GiB usados |
| FS | `/dev/nvme0n1p2` btrfs em `/` e `/home`, NVMe (`ROTA=0`) |
| Disco | 238 GiB, **24 GiB livres (91% usado)** |
| `target/` | 12 GiB (debug 9,3 GiB; release 2,2 GiB; incremental 2,3 GiB) |
| Jobs | não configurado → default = 4 |
| Cache | `RUSTC_WRAPPER` unset; **sccache não instalado** |
| Linker | mold via rustflags; `ld.lld`, `ld.gold` e GNU ld 2.47 disponíveis |
| CPU freq | governor `powersave`, 2,70 GHz |
| Thermal | `x86_pkg_temp` 55 °C — sem sinal claro de throttling |

## 7. Gargalos de compilação

### Medidos — `target/cargo-timings/cargo-timing.html`

**Ressalvas importantes:** relatório de 2026-11-23 19:02 UTC, **anterior** aos manifests finais de `beca-bar`/`beca-launcher`; esses crates não aparecem; há “1 job failed” e `beca-cli` com 0,0 s. Perfil dev, 726 units (373 fresh / 353 dirty), total **1068,5 s**, jobs=4. É ordem de grandeza, não baseline do HEAD atual.

| Tempo | Unidade | Frontend / Codegen |
|---:|---|---|
| 261,4 s | `i-slint-compiler 1.18.1` | 24,6 / 236,7 |
| 207,5 s | **`beca-ui`** | 45,8 / 161,8 |
| 181,7 s | `syn 3.0.9` | 16,4 / 165,3 |
| 181,6 s | `zbus 5.19.0` (accessibility) | 31,7 / 149,9 |
| 179,6 s | `i-slint-core` | 17,0 / 162,6 |
| 169,8 s | `read-fonts` | 35,1 / 134,7 |
| 129,0 s | **`x11rb-protocol`** | 28,4 / 100,6 |
| 125,0 s | `slint-interpreter` | 8,6 / 116,4 |
| 120,5 s | `skrifa` | 9,4 / 62,2 |
| 106,4 s | `i-slint-backend-winit` | 4,7 / 101,7 |
| 88,1 s | `wayland-protocols 0.32.13` | 24,1 / 64,0 |
| 72,8 s | `image 0.25.10` | 6,2 / 66,6 |
| 71,6 s | `rustls` (via `ureq`) | 9,4 / 62,2 |
| 71,4 s | `accesskit_unix` | 8,5 / 62,9 |

Workspace: `beca-ui` 207,5 s · `beca-ui-catalog` 45,0 s · `beca-layer-shell` 43,7 s · `beca-panel` 38,7 s · `beca-core` 29,5 s · `beca-daemon` 9,5 s · `beca-apps` 7,1 s · `beca-process` 3,1 s · `beca-voice` 3,0 s.

### Estimados

- Debug info completo em dev (`debug = 2` para tudo).
- Duplicação de pilhas Wayland (0.18/0.19, 0.31/0.32) e proc-macro (`syn` 2/3).
- Cascata de recompilação por `rerun-if-changed=../beca-ui/ui`.
- Custo de build scripts não foi capturado (units apareceram com 0,0 s por estarem fresh).

Para coletar baseline atual: `cargo build --timings -p beca-cli` ou `cargo check --timings --workspace`.

## 8. Testes

- Unitários: `beca-core` (6), `beca-ui` (21: 12 streaming + 5 ui_loader + 4 theme), `beca-daemon` (3 em `protocol.rs`), `beca-cli` (3 em `cli.rs`), `beca-layer-shell` (2 em `platform.rs`).
- Sem testes: `beca-bar`, `beca-launcher`, `beca-voice`, `beca-apps`, `beca-process`.
- Integração: apenas `crates/beca-ui/tests/streaming_live.rs`, com `#[ignore = "requires live q-rs server and voice daemon"]` (HTTP em `:8787`/`:4100`). Não exige Wayland/Hyprland nem áudio.
- Os 5 testes de `ui_loader` recompilam componentes Slint via interpreter — custo de CPU considerável por teste.
- Testes de config usam `temp_dir()` + PID e `remove_dir_all` no início: razoável, mas estado global e PIDs podem colidir em execuções concorrentes.
- Não há `[dev-dependencies]`, logo nada duplicado nesse eixo.

## 9. Riscos arquitetonais

1. UI vazando para camadas base: `beca-core → beca-ui/slint`, `beca-layer-shell/daemon/launcher → beca-ui`.
2. `beca-ui` monolítico (kit + voz): qualquer mudança invalida 8 dependentes e 2 bins de dev.
3. Defaults do Slint ligam X11 + femtovg + accessibility + system-tray desnecessariamente para um shell Wayland.
4. Mesmo `.slint` compilado 3× + runtime.
5. Versões duplicadas de Wayland/smithay/calloop e syn/thiserror/rustix.
6. Pressão de recursos: 4 threads, zram em uso, 91% de disco, 2,3 GiB de incremental.
7. Deps provavelmente mortas (confirmar com `cargo check` antes de remover): `beca-core → beca-apps`/`slint`, `beca-ui → clap`/`serde`, `beca-layer-shell → calloop`/`serde_json`, `beca-panel → beca-ui`.

## 10. Recomendações (registradas; aplicação parcial em 23/11 — ver §13)

### Imediatas, baixo risco

- Usar `-p beca-cli` / `-p <lib>` no dia a dia; `cargo check --workspace --exclude beca-ui-catalog --exclude beca-panel` para validação ampla.
- Gerar timing novo antes de qualquer decisão de feature.
- Remover uma dep suspeita por vez e validar com `cargo check -p <crate>`.

### Configuração Cargo

- `profile.dev`: `debug = "line-tables-only"` e `[profile.dev.package."*"] debug = false`; manter `opt-level = 1` primeiro. Opcional: `[profile.debugging] inherits = "dev"` com debug completo.
- Centralizar `anyhow` em `[workspace.dependencies]`.
- Avaliar `default-members = ["crates/beca-cli"]` **ou** manter tudo e documentar `-p`. drawbacks: `cargo check` deixa de validar catálogo/painel por padrão.
- **Não** aumentar `jobs`; considerar `jobs = 3` apenas se houver OOM.

### Dependências e features

- Testar, em todas as declarações de uma vez (feature unification): `default-features = false`, `features = ["std", "renderer-software", "backend-winit-wayland", "compat-1-2"]`. Remove X11, femtovg, accessibility e system-tray. **Decisão de produto**: validar catálogo/painel e fallback de janela em runtime.
- Remover features redundantes em `beca-layer-shell` se cobertas pelo novo conjunto.
- Alinhar pilha Wayland própria com a do Slint (quebra de API; exige refactor).
- Isolar `ureq` em feature `voice-network` para catálogo/painel e core não pagarem rustls.

### Arquitetura

- Separar de `beca-ui` o kit estático/loader do motor de voz, ou extrair `beca-ui-kit` para que `beca-core` não puxe Slint.
- Fazer `beca-panel` reutilizar `beca_ui::AppWindow` e remover seu `build.rs`/`slint-build`.
- Retirar catálogo/painel dos default members (workspace separado ou `exclude`).

### Longo prazo

- Usar `cargo build --timings` como gate de regressão de build.
- sccache só com CI/múltiplos worktrees; disco a 91% e cache local têm pouco valor agora.
- Reavaliar o runtime dinâmico de UI apenas se o time aceitar abrir mão de UI trocável sem rebuild.
- Monitorar temperatura/swap durante builds: 4 threads em 2 núcleos podem dominar o tempo mais que qualquer flag.

## 11. Comandos recomendados

```bash
# checagem rápida
cargo check -p beca-cli
cargo check -p beca-core
cargo check -p beca-ui
cargo check --workspace --exclude beca-ui-catalog --exclude beca-panel

# build / run do shell
cargo build --release -p beca-cli
cargo run -p beca-cli -- --help
cargo run -p beca-cli -- mod bar:status

# previews
cargo run -p beca-ui-catalog
cargo run -p beca-panel

# testes
cargo test -p beca-core -p beca-daemon
cargo test -p beca-ui --lib
cargo test -p beca-ui --test streaming_live -- --ignored

# validação completa / medição
cargo check --workspace
cargo test --workspace --lib
cargo build --timings -p beca-cli
```

## 12. Pendências e incertezas

1. Filtro de escopo não foi informado; o repo inteiro foi auditado.
2. Deps “não usadas” são suspeitas por varredura de texto; removê-las exige `cargo check`.
3. Custo real dos build scripts não foi medido nesta corrida.
4. Sem dois timing reports consecutivos, não dá para atribuir recompilações a mudanças específicas.
5. O timing atual está desatualizado e registra 1 job falho; não há log da causa.
6. Remover X11/femtovg depende de decisão sobre catálogo/painel e fallback de janela.
7. Separar `beca-ui` muda APIs públicas (`AppWindow`, `SearchWindow`, `voice::*`).
8. `target/flycheck0` sugere target dir externo usado por editor; não há config persistida no repo que o explique.

---

## 13. Aplicação e medições (2026-11-23)

Foram aplicados os ciclos 1 e 2 do plano, sem reestruturação de crates:

1. **Cargo:** `default-members = ["crates/beca-cli"]`; `anyhow` centralizado em `[workspace.dependencies]` e herdado por 8 crates.
2. **Perfil dev:** `debug = "line-tables-only"`, `incremental = true`, `[profile.dev.package."*"] opt-level = 0, debug = false`, além do perfil opcional `debugging` com símbolos completos.
3. **Slint:** `default-features = false` em `slint` **e** `slint-interpreter`, com `["std", "backend-winit-wayland", "renderer-software", "compat-1-2"]`; features redundantes removidas de `beca-layer-shell`. `Cargo.lock` perdeu 427 linhas.

### Medições

`cargo build --offline --timings -p beca-cli`, tempo de parede e max RSS via wrapper Python:

| Etapa | Tempo | Dirty/total | Max RSS | `target/` |
|---|---:|---:|---:|---:|
| Baseline (quase toda fresca) | 12,7 s | 3 / 722 | 464 MB | 13 GiB |
| 2a — default-members + anyhow + debug reduzido | 13m56s | 353 / 722 | 1.510 MB | 16 GiB |
| 2b — + `opt-level=0` nas deps | 3m59s | 274 / 591 | 1.628 MB | 17 GiB |
| 3 — + features do Slint | 2m15s | 70 / 537 | 1.550 MB | 18 GiB |

- Baseline não é comparável: 719 de 722 units estavam frescas.
- 2a → 2b é a comparação controlada (só `opt-level` das deps mudou): ~14 min → ~4 min.
- 2b → 3 reduziu também o total de unidades (591 → 537), por features removidas do grafo.
- Nenhum `cargo clean`; `target/` cresceu por artefatos das versões anteriores dos perfis.

### Verificação do grafo

- Ausentes após a mudança: `renderer-femtovg`, `glutin`, `accessibility`, `accesskit`, `system-tray`.
- Ainda presentes: `x11rb`, `zbus`, `wayland-protocols 0.31`, `smithay-client-toolkit 0.18` — ver correção na §4.
- `slint-interpreter` mantido: é usado por `beca-ui/src/ui_loader.rs` e `beca-core/src/lua/bridge.rs`.

### Validação

- `cargo check --workspace` — OK.
- `cargo test -p beca-core -p beca-daemon` — 10 testes OK.
- `cargo test -p beca-ui --lib` — 18 testes OK.
- `cargo build -p beca-ui-catalog -p beca-panel` — OK.
- Smoke test com `target/debug/beca`: `--help`, `mod list`, `ping`, `mod bar:status` — OK.

### Pendências

- Não houve validação visual interativa das janelas (launcher, voz, catálogo, painel) nem build release com a nova configuração.
- A pilha Wayland duplicada, a separação de `beca-ui` e a compilação tripla de `.slint` continuam sem alteração (itens 6, 2 e 5 da priorização original).
- Para uma baseline fria de verdade seria necessário `cargo clean` completo ou target dir separado; não feito por custo e espaço.
