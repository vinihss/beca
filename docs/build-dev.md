# Development and builds — local cycle optimization

> **Date:** 11 Nov 2026
> **Scope:** `cargo build` / `cargo test` / `cargo check` during `beca` development, without changing the release profile or the functional logic.

## 1. Current diagnosis

The project is already organized as a Rust workspace:

```text
crates/
  beca/                 CLI/daemon and main commands
  beca-apps/            .desktop entry integration
  beca-core/            configuration, themes, Lua/runtime bridge
  beca-ui/              Slint components/windows and the voice/UI engine
  beca-layer-shell/     wlr-layer-shell surfaces (bar, launcher, quick HUD)
```

The linker is already configured in `.cargo/config.toml` to use `mold`, and the release profile already uses `lto = "thin"` with `codegen-units = 16`.

The dev profile, however, is still the default for debug info:

```toml
[profile.dev]
opt-level = 1
```

That tends to generate a lot of debug info for the workspace and its dependencies, increasing codegen, link times and disk usage.

Local measurements observed:

- `target/debug`: ~29 GB;
- `target/release`: ~6.3 GB;
- project disk: ~96% used;
- a full dev build after a profile change can recompile the affected artifacts and cost several minutes;
- `cargo check` should be significantly faster because it does not produce the final binary nor run full codegen/link.

## 2. Strategies by priority

### 2.1 Reduce debug info in the dev profile

This is the biggest low-risk improvement for the local loop. The recommended configuration would be:

```toml
[profile.dev]
opt-level = 0
debug = "line-tables-only"

[profile.dev.package."*"]
opt-level = 1
debug = false

# Optional: lets you enable full debug info when actually debugging.
[profile.debugging]
inherits = "dev"
debug = true
```

Expected effects:

- `beca`, `beca-ui` and the other workspace members compile without optimization;
- dependencies keep `opt-level = 1`;
- workspace uses line tables only, keeping file/line info in backtraces;
- dependencies do not emit useful debug info for the user binary;
- builds and links get smaller and faster;
- `target/debug` should grow less in disk usage.

Trade-offs:

- full backtraces and variable inspection require `--profile debugging`;
- changing the profile invalidates dependent artifacts and may force a full rebuild;
- since `target/debug` already holds ~29 GB, free space should be assessed before the first rebuild with the new configuration.

If the concern is changing only one thing at a time, an intermediate step would be:

```toml
[profile.dev]
opt-level = 1
debug = "line-tables-only"

[profile.dev.package."*"]
debug = false
```

That reduces debug info without losing `opt-level = 1`, but tends to yield a smaller gain.

### 2.2 Use `cargo check` as the development command

During code writing, the default path should be:

```bash
cargo check -p beca-cli
cargo check --workspace
```

`check` keeps:

- parsing;
- type analysis;
- the borrow checker;
- name resolution;
- macro and trait verification;

but avoids:

- final machine-code emission for all crates;
- binary linking;
- the most expensive part of the build cycle.

`cargo test --no-run` is not equivalent in cost to `cargo check`: it compiles the test binary and runs codegen, so it is useful when the goal is to prepare or inspect the test executable, not for every edit iteration.

Recommended cycle:

1. `cargo check -p <crate>` while editing;
2. `cargo test -p <crate> --lib` when running tests;
3. `cargo build -p beca-cli` only when you need to open the binary or validate the final link.

### 2.3 Split `beca-ui` only where there is measured cost

The workspace is already split, so "put the project in a workspace" is done. A further split could help, but only if it reduces the units recompiled during normal iterations.

Current pain points:

- `beca-ui/src/voice/capture.rs` is large;
- `beca-ui/src/voice/quick.rs` is also considerable;
- `beca-ui` loads many Slint components and uses `slint-interpreter`;
- changes to `ui/**/*.slint` can invalidate the `beca-ui` crate, depending on the build script and the bundle embedding strategy.

Possible future boundaries, only after measuring with timings:

```text
beca-ui-core    # ui_loader, theme, common view-model/properties
beca-voice      # voice engine (capture, STT/LLM/TTS, quick)
beca-ui-shell   # shared Slint components/windows
```

Splitting the `.slint` packaging process from the runtime Rust would also be justified, reducing cross-invalidation. However, it is not a mechanical change: it touches public APIs between crates, dependencies and runtime contracts.

Rule for this work:

1. run a build report to know where time is actually spent;
2. extract only the most expensive units;
3. keep the number of crates close to what incremental compilation needs;
4. do not create trivial layers that only increase cross-crate dependency cost.

Example measurement when authorized:

```bash
cargo build --timings -p beca-cli
cargo check --timings --workspace
```

### 2.4 `sccache`

`sccache` is not a priority in this repository.

It would tend to help if there were:

- CI recompiling identical states;
- frequent switching between worktrees/branches restoring identical files;
- repeated dependency builds across different environments;
- shared remote cache.

In the local editing flow, the changed crate rarely matches the ideal cache key, so the main cost is still recompiling the changed unit. Also, the external cache would duplicate part of the compilation artifacts, which is undesirable with disk already at ~96%.

**Recommended decision:** do not install/configure `sccache` now. Reassess only if there is CI, multiple worktrees or repeated rebuilds of identical states.

## 3. Recommended evaluation order

1. Assess free space and the impact of `target/debug`.
2. Change the `dev` profile to reduce debug info.
3. Measure a full dev rebuild.
4. Measure incremental iterations of typical edits with `cargo check`.
5. Use `cargo build --timings` to identify the dominant crates before any refactor.
6. Only then consider splitting `beca-ui` or moving the `.slint` bundle.

## 4. Closed decisions

- Do not switch release to optimize the editing flow.
- Do not add `sccache` in the current state.
- Do not split crates without prior profiling.
- Use `cargo check` as the main command of the write cycle.
- Prioritize debug info reduction and `opt-level` in the dev profile.

## 5. Validation command after a profile change

When the change is implemented, validate with:

```bash
cargo check --workspace
cargo test -p beca-ui --lib
```

To compare the new profile against real numbers, use Cargo's timings report during an authorized rebuild:

```bash
cargo build --timings -p beca-cli
```

Do not use `cargo clean` as a standard step in this validation. If dev artifacts are skewing the comparison, selective cleanup must be decided explicitly by the author of the change.
