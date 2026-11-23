# Development and builds — local cycle optimization

> **Atualizado:** 23 Nov 2026. Substitui a análise de 11 Nov 2026, que usava nomes de crates e números de `target/` antigos.
> Diagnóstico completo e não aplicado: [`build-performance-audit.md`](build-performance-audit.md).

## 1. Estado atual

- Workspace único, `resolver = "3"`, 12 members.
- `default-members = ["crates/beca-cli"]`: `cargo check`/`build`/`test` na raiz miram só o shell. Catálogo e painel continuam no workspace e entram em `--workspace`.
- Linker: `mold` via `.cargo/config.toml` para `x86_64-unknown-linux-gnu`.
- `profile.release`: `lto = "thin"`, `codegen-units = 16`. Para packaging final, usar `--config profile.release.lto=full --config profile.release.codegen-units=1`.
- CPU: i7-7500U (2 núcleos / 4 threads); NVMe btrfs com ~19 GiB livres no momento das medições. **Não aumentar `jobs`** e **não instalar sccache** neste estado.

Configuração aplicada no `Cargo.toml` raiz:

```toml
[workspace.dependencies]
anyhow = "1"
slint = { version = "1.18.1", default-features = false, features = ["std", "backend-winit-wayland", "renderer-software", "compat-1-2"] }
slint-interpreter = { version = "1.18.1", default-features = false, features = ["std", "backend-winit-wayland", "renderer-software", "compat-1-2"] }

[profile.dev]
opt-level = 1
debug = "line-tables-only"
incremental = true

[profile.dev.package."*"]
opt-level = 0
debug = false

[profile.debugging]
inherits = "dev"
debug = true

[profile.debugging.package."*"]
debug = 2
```

`beca-core`, `beca-ui`, `beca-launcher`, `beca-voice`, `beca-bar`, `beca-daemon` e os bins de UI herdam as features do Slint. `beca-layer-shell` não adiciona features extras — `compat-1-2` e `renderer-software` já vêm do root.

Para depurar com símbolos completos: `cargo build --profile debugging -p beca-cli`.

## 2. Por que `slint` **e** `slint-interpreter`

Ambos os crates 1.18.1 declaram `backend-default` nos defaults, que vira `i-slint-backend-selector/default` = `backend-winit` = X11 **+** Wayland. Se só `slint` for alterado, o interpreter reativa o custo por unificação de features. O `slint-interpreter` também não inclui `std` no default, por isso `std` está explícito nas duas declarações.

Removidos do grafo com essa mudança: `renderer-femtovg`/`glutin`, `accessibility`/`accesskit`, `system-tray`. O `Cargo.lock` perdeu 427 linhas.

**Não removidos:** `x11rb` e `zbus` continuam presentes. Eles não vêm das features desligadas: `i-slint-backend-winit` declara `zbus` como dependência não opcional no Linux, e `arboard` traz `x11rb` incondicionalmente em Unix. Enquanto o backend winit for necessário (catálogo, painel, janelas fallback), esse custo permanece. Também permanecem as versões próprias 0.18/0.31 de `smithay-client-toolkit`/`wayland-client`, convivendo com as do Slint.

`slint-interpreter` **não** pode sair: `beca-ui/src/ui_loader.rs` e `beca-core/src/lua/bridge.rs` dependem dele para carregar `.slint` por XDG sem recompilar.

## 3. Medições

Comandos: `cargo build --offline --timings -p beca-cli`, com tempo de parede e max RSS capturados por wrapper Python. O perfil change invalida fingerprints, então cada linha abaixo (exceto a baseline) é uma recompilação real.

| Etapa | Configuração | Tempo | Unidades dirty/total | Max RSS | `target/` |
|---|---|---:|---:|---:|---:|
| Baseline | dev original, `opt-level=1`, debug completo | 12,7 s | 3 / 722 | 464 MB | 13 GiB |
| Passo 2a | `default-members`, `anyhow` centralizado, debug reduzido | 13m56s | 353 / 722 | 1.510 MB | 16 GiB |
| Passo 2b | + `opt-level=0` nas dependências | 3m59s | 274 / 591 | 1.628 MB | 17 GiB |
| Passo 3 | + features do Slint restritas | 2m15s | 70 / 537 | 1.550 MB | 18 GiB |

Ressalvas:

- A baseline estava **quase toda fresca** (719 fresh), então 12,7 s mede iteração morna, não custo de compilação. Não é comparável às linhas seguintes.
- 2a → 2b é a comparação limpa: mesmo escopo, única variável = `opt-level` das deps. Queda de ~14 min para ~4 min.
- 2b → 3 também reduziu o número total de unidades (591 → 537) porque features removem crates do grafo; o tempo menor reflete menos trabalho, não só cache.
- O relatório antigo em `target/cargo-timings/` (19:02 UTC) era de antes dessas mudanças e está preservado em `/tmp/opencode/cargo-timing-baseline.html` apenas como referência histórica.

`target/` cresceu porque versões antigas dos artefatos não foram apagadas; não houve `cargo clean`.

## 4. Comandos recomendados

```bash
# iteração (só o shell)
cargo check -p beca-cli
cargo check -p beca-core
cargo check -p beca-ui

# validação dos 12 crates
cargo check --workspace
cargo test --workspace --lib

# contrato do projeto
cargo test -p beca-core -p beca-daemon

# binário e runtime
cargo build --release -p beca-cli
cargo run -p beca-cli -- --help
cargo run -p beca-cli -- mod bar:status

# previews (fora do default-members)
cargo run -p beca-ui-catalog
cargo run -p beca-panel

# medição (somente quando autorizado a pagar o build)
cargo build --offline --timings -p beca-cli
```

## 5. Validação das mudanças aplicadas

- `cargo check --workspace` — OK.
- `cargo test -p beca-core -p beca-daemon` — 10 testes OK.
- `cargo test -p beca-ui --lib` — 18 testes OK.
- `cargo build -p beca-ui-catalog -p beca-panel` — OK com as features reduzidas.
- Smoke test: `beca --help`, `beca mod list`, `beca ping`, `beca mod bar:status` — OK.
- `cargo tree -e features` confirma a ausência de femtovg/accesskit/glutin/system-tray e a permanência de x11rb/zbus.

Não foi feita validação visual interativa das janelas (launcher/voz/catálogo/painel) nem build release. Esses passos continuam pendentes se a remoção de renderer/features afetar alguma superfície.

## 6. Decisões fechadas

- Não mexer na pilha Wayland duplicada nem separar crates de `beca-ui` sem novo diagnóstico.
- Não remover `slint-interpreter` nem o runtime dinâmico de `.slint`.
- Não aumentar `jobs`, não instalar sccache, não usar `cargo clean` como passo padrão.
- Não aplicar `lto=full` no perfil padrão; apenas via `--config` no packaging.
