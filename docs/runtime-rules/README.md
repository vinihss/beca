# Beca Runtime Rules

> Regras de runtime do Beca, divididas por assunto. Leia apenas o arquivo relevante para a área sendo alterada; não carregue as 25 seções de uma vez.

## 1. Purpose

Beca is a Wayland desktop shell primarily designed to operate in a Hyprland environment.

The agent must treat Beca as a long-running, event-driven desktop system with interaction between:

- Wayland compositor.
- Hyprland IPC.
- Wayland surfaces.
- Shell UI.
- Background services.
- Processes.
- User input.
- System state.
- Configuration.
- External integrations.
- Audio and voice systems.

The system must remain responsive, predictable, resource-efficient, and capable of handling lifecycle transitions, failures, asynchronous events, and graceful shutdown.

These rules establish engineering constraints for lifecycle management, event processing, runtime coordination, system integration, process management, and state ownership.

---

## Quando ler cada arquivo

| Arquivo | Seções originais | Leia quando mexer em |
|---|---|---|
| [lifecycle.md](lifecycle.md) | 2–4, 17–18 | startup, estados do shell, ownership de componentes/tasks, shutdown, reload de config |
| [events-and-state.md](events-and-state.md) | 5–9, 21 | event loop, comandos/eventos, roteamento, estado, sincronização com o sistema |
| [surfaces-and-hyprland.md](surfaces-and-hyprland.md) | 10–12 | layer-shell, superfícies Wayland, integração e reconexão Hyprland |
| [processes-and-async.md](processes-and-async.md) | 13–16, 20 | processos, serviços em background, threads/async, backpressure, recursos |
| [quality.md](quality.md) | 19, 22–24 | falhas e recuperação, testes, observabilidade, checklist de revisão |

---

## 25. Final Runtime Engineering Rule

Treat Beca as a coordinated, long-running, event-driven desktop system.

Lifecycle, ownership, event processing, system integration, state synchronization, concurrency, resource management, recovery, and shutdown are architectural concerns—not implementation details.

Before adding runtime complexity, identify the actual requirement.

Prefer explicit lifecycle contracts, clear ownership, typed events, bounded resources, isolated system adapters, deterministic state transitions, and graceful recovery.

**Keep the shell responsive, components independently understandable, runtime behavior predictable, and the architecture as simple as the requirements allow.**
