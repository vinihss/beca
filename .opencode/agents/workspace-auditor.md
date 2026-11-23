---
description: Auditoria somente-leitura do workspace Beca — estrutura, Cargo, dependências, Slint, build, hardware, testes e workflow
mode: subagent
permissions:
  - action: edit
    resource: "*"
    effect: deny
  - action: shell
    resource: "cargo build *"
    effect: deny
  - action: shell
    resource: "cargo check *"
    effect: deny
  - action: shell
    resource: "cargo test *"
    effect: deny
  - action: shell
    resource: "cargo clean"
    effect: deny
  - action: shell
    resource: "cargo add *"
    effect: deny
  - action: shell
    resource: "cargo remove *"
    effect: deny
  - action: shell
    resource: "cargo install *"
    effect: deny
  - action: shell
    resource: "rustup *"
    effect: deny
---

# Beca — Workspace and Build Performance Audit

You are conducting a technical investigation of the current Beca repository.

The project structure and architecture have changed. Do not assume previous crate names, directory structures, dependencies, or architectural decisions are still valid.

Your task is to inspect the actual repository and answer the questions below.

## IMPORTANT RULES

- Do not modify any files.
- Do not install or remove dependencies.
- Do not change Cargo configuration.
- Do not run destructive commands.
- Do not make architectural changes.
- Base every answer on the current repository.
- Include file paths and relevant code snippets when useful.
- Distinguish verified facts from assumptions.
- If information cannot be determined, explicitly say so.
- Do not execute expensive full builds unless explicitly requested.
- Prefer read-only Cargo inspection commands.

---

# 1. Project Structure

1. What is the current repository directory structure?
2. Which Cargo workspaces and crates currently exist?
3. Which crates are executable applications?
4. Which crates are libraries?
5. Which crates are development tools, examples, or catalogs?
6. What is the responsibility of each crate?
7. Are there crates that appear redundant or unnecessarily coupled?
8. Are all crates correctly declared as workspace members?
9. What are the current default workspace members?
10. Which executable is the actual Beca shell entry point?

Provide a tree of the current workspace, including relevant Cargo.toml and build.rs files.

# 2. Cargo Configuration

1. What is the complete root Cargo.toml configuration?
2. Which profiles are configured?
3. Are dev, test and release profiles customized?
4. Is incremental compilation enabled?
5. Are optimization levels appropriate for development?
6. Are debug symbols unnecessarily expensive?
7. Are there custom build scripts?
8. Are there workspace-wide dependencies that are not actually needed by every crate?
9. Are dependency versions centralized correctly?
10. Are there unused dependencies or unnecessary feature flags?

Inspect all relevant Cargo.toml files.

# 3. Dependency Graph

1. What are the direct dependencies of each crate?
2. Which crates depend on beca-core?
3. Which crates depend on the UI layer?
4. Are there circular dependencies or problematic dependency directions?
5. Which dependencies are optional?
6. Which dependencies enable default features unnecessarily?
7. Are there duplicated dependencies with different versions?
8. Which dependencies introduce the largest transitive dependency trees?
9. Are development-only dependencies correctly placed under dev-dependencies?
10. Are build-only dependencies correctly placed under build-dependencies?

Run and inspect:

```bash
cargo tree --workspace
cargo tree --workspace -d
cargo tree --workspace -e features
cargo metadata --format-version 1 --no-deps
```

Identify particularly expensive dependencies.

# 4. Slint and UI Architecture

1. Which crates directly depend on Slint?
2. Which crates depend on slint-build?
3. Which crates depend on slint-interpreter?
4. Is Slint used exclusively for static UI or also for dynamic runtime component loading?
5. Which Slint backends are enabled?
6. Which renderers are enabled?
7. Is X11 support being compiled?
8. Is Wayland support enabled?
9. Are multiple renderers being compiled unnecessarily?
10. Are .slint files compiled in more than one crate?
11. Are build scripts generating unnecessary code?
12. Can UI catalog or preview functionality be isolated from the main shell executable?
13. Are there unnecessary dependencies between UI components and system integration crates?

Identify opportunities to reduce Slint compilation overhead without sacrificing required runtime functionality.

# 5. Build Performance

1. What is the current Rust toolchain version?
2. What is the target architecture?
3. Which build profiles are actually used?
4. How many compilation jobs are configured?
5. Is the project using incremental compilation?
6. Is sccache or another compiler cache configured?
7. Is the linker configured explicitly?
8. Which crates have the highest compilation cost according to the available timing reports?
9. Which dependencies are repeatedly recompiled?
10. Are build scripts or procedural macros contributing significantly?
11. Does modifying a single UI component trigger unnecessary recompilation across the workspace?
12. Can package-specific build commands reduce development iteration time?

Do not run a full build just to answer these questions. Use existing timing reports and available build metadata.

If timing data is unavailable, explain how to collect it.

# 6. Hardware and Build Environment

Inspect the available environment and answer:

1. CPU model and available logical processors.
2. Available RAM.
3. Swap configuration.
4. Filesystem used by the project.
5. Whether the project and Cargo target directory reside on SSD/NVMe or another storage device.
6. Available disk space.
7. Rust compiler version.
8. Cargo version.
9. Configured Cargo build jobs.
10. Configured environment variables affecting compilation.
11. Whether the system uses mold, lld, GNU ld or another linker.
12. Whether CPU frequency scaling or thermal throttling might affect compilation.

Use read-only commands such as:

```bash
rustc --version
cargo --version
nproc
free -h
swapon --show
df -h .
lsblk -o NAME,TYPE,SIZE,FSTYPE,MOUNTPOINTS
env | grep -E '^(CARGO|RUST|CC|CXX|LD)='
```

Do not expose secrets, tokens, credentials or private environment variables.

# 7. Testing Architecture

1. Which crates have unit tests?
2. Which crates have integration tests?
3. Are there tests requiring Wayland or Hyprland?
4. Are there tests requiring audio hardware?
5. Are tests properly isolated?
6. Can tests run independently per crate?
7. Are there expensive fixtures or build-time operations?
8. Are test dependencies unnecessarily duplicated?
9. Can fast unit tests be separated from environment-dependent integration tests?
10. What is the recommended command for testing only the currently modified crate?

# 8. Development Workflow

1. What is the recommended command to compile only the main shell?
2. What is the recommended command to check a specific library crate?
3. What is the recommended command to run the UI catalog?
4. What is the recommended command to run unit tests?
5. What is the recommended command to validate the entire workspace?
6. Which changes are likely to trigger broad recompilation?
7. Can the workspace be configured to avoid compiling development tools during normal development?
8. Are there opportunities to create dedicated development profiles?

# 9. Final Report

Produce a structured report containing:

## A. Current Architecture

- Actual workspace structure.
- Crate responsibilities.
- Dependency relationships.
- Main executable entry point.

## B. Build Environment

- Rust and Cargo versions.
- CPU and memory.
- Build jobs.
- Linker.
- Cache configuration.

## C. Compilation Bottlenecks

Rank the identified bottlenecks by expected impact.

Distinguish measured compilation costs from estimated costs.

## D. Architectural Risks

Identify:

- Unnecessary crate coupling.
- Heavy dependencies in foundational crates.
- Unnecessary feature activation.
- UI dependencies leaking into unrelated crates.
- Potential sources of broad recompilation.

## E. Optimization Recommendations

Provide recommendations divided into:

1. Immediate improvements.
2. Cargo configuration improvements.
3. Dependency and feature improvements.
4. Workspace architecture improvements.
5. Long-term improvements.

For each recommendation include:

- Current situation.
- Proposed change.
- Expected benefit.
- Potential drawbacks.
- Files affected.
- How to measure the result.

## F. Proposed Development Commands

Provide an optimized command reference for:

- Fast type checking.
- Building the main application.
- Running the shell.
- Running UI previews.
- Running unit tests.
- Running integration tests.
- Full workspace validation.

## G. Questions and Unknowns

List any information that could not be verified and any decisions requiring developer input.

Remember: this is an investigation only. Do not modify the repository.
