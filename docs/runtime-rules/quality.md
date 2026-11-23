# Failure, Testing and Review

> Parte das regras de runtime do Beca. Índice: [README.md](README.md).

## 19. Failure Handling and Recovery

Failures must be classified.

### Fatal failures

Examples:

- Required runtime dependency unavailable.
- Core initialization cannot complete.
- Runtime invariants violated.
- Critical system integration unavailable when required.

Fatal failures may trigger shutdown.

### Recoverable failures

Examples:

- Optional widget failure.
- Temporary external service failure.
- Hyprland IPC disconnect.
- Network unavailable.
- Audio device unavailable.
- Non-critical background task failure.

Recoverable failures should not automatically terminate the entire shell.

### Recovery rules

- Identify the failed component.
- Determine whether the failure is transient.
- Apply a bounded retry policy.
- Avoid restart loops.
- Maintain a degraded state when appropriate.
- Report failures.
- Reconcile state after recovery.
- Preserve unrelated functionality.

Do not retry every error indefinitely.

Do not silently swallow failures.

---

## 22. Testing Runtime Behavior

Testing must not require a full desktop environment for every change.

### Unit tests

Test:

- State transitions.
- Lifecycle transitions.
- Event conversion.
- Event routing.
- Retry policies.
- Backoff.
- Process command construction.
- Configuration validation.
- Reconciliation logic.
- Cancellation logic.

### Integration tests

Test:

- Component interactions.
- IPC adapters.
- Process lifecycle.
- Service startup and shutdown.
- Event channel behavior.
- Failure recovery.

### Environment-dependent tests

Tests requiring:

- Wayland.
- Hyprland.
- Audio hardware.
- GPU.
- External services.

Must be explicitly identified and separated from routine unit tests.

Use mocks, fakes, or test adapters when they provide meaningful isolation.

Do not make ordinary unit tests require a running compositor.

### Lifecycle tests

Test scenarios where appropriate:

- Successful startup.
- Partial initialization failure.
- Component startup failure.
- Shutdown during startup.
- Shutdown during runtime.
- Repeated shutdown.
- External connection failure.
- External reconnection.
- Task cancellation.
- Service restart.
- Surface cleanup.
- Process cleanup.

---

## 23. Observability and Diagnostics

Runtime behavior must be diagnosable.

Use structured tracing where appropriate.

Consider:

- Component names.
- Lifecycle transitions.
- Task IDs.
- Process IDs.
- Event types.
- Connection state.
- Retry counts.
- Shutdown progress.
- Surface lifecycle.
- Runtime errors.

Avoid:

- Excessive logging.
- Logging secrets.
- Logging sensitive user data.
- High-volume event logging without filtering.
- Logging every UI event at info level.

Use appropriate log levels.

Lifecycle transitions and critical failures should be observable.

---

## 24. Architectural Review Checklist

Before implementing or reviewing runtime-related changes, consider:

### Lifecycle

- Is the component lifecycle explicit?
- Is startup order correct?
- Is shutdown order correct?
- Are partial initialization failures handled?
- Are resources cleaned up?

### Ownership

- Who owns the component?
- Who owns its tasks?
- Who owns its resources?
- Who is responsible for cancellation?
- Who is responsible for shutdown?

### Events

- Are commands and events distinct?
- Is event ownership clear?
- Is event routing appropriate?
- Is ordering required?
- Is backpressure defined?
- Are event payloads minimal and typed?

### State

- Is state ownership clear?
- Is there a single authoritative source?
- Are transitions explicit?
- Is reconciliation required?
- Is derived state unnecessarily stored?

### Wayland

- Is surface lifecycle respected?
- Are protocol events handled correctly?
- Are output changes handled?
- Is exclusive zone behavior correct?
- Are resources cleaned up?

### Hyprland

- Is IPC isolated?
- Are reconnects handled?
- Is state reconciled?
- Are commands validated?
- Are protocol details hidden?

### Processes

- Is process ownership clear?
- Are user applications distinguished from shell-owned services?
- Are commands safe?
- Are timeouts needed?
- Is cleanup correct?

### Concurrency

- Is async necessary?
- Are blocking operations isolated?
- Are tasks supervised?
- Is cancellation supported?
- Are locks necessary?
- Can deadlocks occur?

### Reliability

- Are failures classified?
- Are retry policies bounded?
- Can optional failures degrade gracefully?
- Can recovery occur without restarting the entire shell?

### Performance

- Can this block the UI?
- Can this block event processing?
- Are allocations necessary?
- Can queues grow without limits?
- Is work being duplicated?

### Testing

- Can the logic be tested without Wayland?
- Are lifecycle transitions tested?
- Are failure scenarios tested?
- Are tests deterministic?
- Are system tests isolated?

---

