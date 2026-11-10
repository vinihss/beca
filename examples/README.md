# Exemplos (baseados em `lib-examples.md`)

Exemplos adaptados para este workspace (`crates/ui-lib/ui/library.slint`, `Theme`/`DesktopButton`/`InfoCard`, Slint 1.18, `mlua` luau).

## Conteúdo
- `wizard/` — assistente multi-etapas (Slint + Luau + Rust de referência, fonte).
- `dashboard/` — dashboard de métricas (Slint + Luau + Rust de referência, fonte).

## Executar (crate `examples-runner`)
```bash
cargo run -p examples-runner --bin wizard
cargo run -p examples-runner --bin dashboard
```
- UI compilada: `crates/examples-runner/ui/wizard.slint`, `crates/examples-runner/ui/dashboard.slint` (via `crates/examples-runner/ui/app.slint` / `build.rs`)
- Lógica: `crates/examples-runner/src/bin/{wizard,dashboard}.rs` + `crates/examples-runner/scripts/{wizard,dashboard}_behavior.luau`

## Pré-visualizar (UI, sem lógica)
```bash
slint-viewer crates/examples-runner/ui/wizard.slint
slint-viewer crates/examples-runner/ui/dashboard.slint
```

> Os `.slint` importam `../../ui-lib/ui/library.slint`. `/behavior.luau` na raiz é usado pelo host/guia `cargo run -p desktop-app`.
