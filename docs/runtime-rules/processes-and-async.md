# Processes, Async and Resources

> Parte das regras de runtime do Beca. Índice: [README.md](README.md).

## 13. Process Lifecycle and Supervision

Process management must be explicit.

### 13.1 Process ownership

Every process launched by Beca must have a defined ownership policy.

Determine whether it is:

- A one-shot command.
- A user-launched application.
- A supervised service.
- A child task.
- A temporary helper.

Do not assume all processes should be terminated when Beca shuts down.

### 13.2 Process categories

#### One-shot commands

Examples:

- Open URL.
- Toggle system setting.
- Launch utility.

Expected behavior:

- Execute.
- Report failure.
- Collect result when needed.
- Avoid unnecessary supervision.

#### User applications

Expected behavior:

- Launch.
- Do not unnecessarily interfere with lifecycle.
- Track only when needed.
- Respect desktop application behavior.
- Avoid terminating user applications during shell shutdown unless explicitly required.

#### Shell-owned services

Expected behavior:

- Supervise.
- Restart according to policy.
- Monitor health.
- Cancel or terminate during shutdown.
- Release resources.

#### Temporary helpers

Expected behavior:

- Use explicit timeout.
- Clean up.
- Avoid orphaning.
- Handle cancellation.

### 13.3 Process execution safety

- Prefer structured executable and argument handling.
- Avoid shell command construction.
- Avoid shell injection.
- Validate external input.
- Use timeouts where appropriate.
- Handle output limits.
- Handle exit codes.
- Handle signals.
- Avoid leaking file descriptors.
- Avoid zombie processes.
- Avoid uncontrolled process spawning.

### 13.4 Process supervision

Supervision policies must be explicit.

Possible policies:

- Never restart.
- Restart once.
- Restart with bounded backoff.
- Restart until shutdown.
- Restart only after specific failures.
- Fail the owning component.

Avoid infinite restart loops.

Use exponential backoff or another bounded retry policy when appropriate.

---

## 14. Background Services

Background services must have explicit lifecycle and responsibilities.

Each service should define:

- Purpose.
- Dependencies.
- Startup conditions.
- Shutdown behavior.
- Error behavior.
- Retry policy.
- Resource ownership.
- Task ownership.
- Health state.
- Observability.

Avoid creating a service for functionality that can be implemented as a simple function or task within an existing component.

Avoid services that communicate only through unnecessary layers of indirection.

### Service health

A service may have states such as:

```text
Stopped
Starting
Running
Degraded
Stopping
Failed
```

Use a health model only where it provides meaningful operational value.

Do not create health-state machinery for trivial tasks.

---

## 15. Async, Threads and Blocking Operations

Concurrency must be intentional.

### Async

Use async for:

- Non-blocking I/O.
- Network communication.
- Async IPC.
- Concurrent external operations.
- Event-driven workloads.

Do not use async merely because a function might eventually perform I/O.

### Threads

Use dedicated threads or worker pools when appropriate for:

- Blocking APIs.
- CPU-intensive operations.
- Native integrations.
- Long-running synchronous work.

### Blocking operations

Never block:

- UI rendering.
- Critical event processing.
- Wayland protocol processing.
- Async executors inappropriately.

Examples of potentially blocking operations:

- Filesystem operations.
- Process waiting.
- Network operations.
- Audio device operations.
- CPU-intensive processing.
- Database operations.

Evaluate each operation based on actual behavior.

### Cancellation

Long-running operations should support cancellation where appropriate.

Cancellation must:

- Propagate to owned work.
- Release resources.
- Avoid partial inconsistent state.
- Avoid leaving processes running unintentionally.
- Avoid blocking shutdown indefinitely.

---

## 16. Backpressure and Resource Limits

Do not allow unbounded resource growth.

Consider limits for:

- Event queues.
- Message queues.
- Process output.
- Background tasks.
- Concurrent requests.
- Audio buffers.
- Network requests.
- Reconnection attempts.
- Temporary data.

When queues are bounded:

- Define overflow behavior.
- Define whether messages are dropped.
- Define whether messages are coalesced.
- Define whether producers block.
- Define how overflow is observable.

For UI state events, coalescing may be appropriate when intermediate states are not meaningful.

For commands, silently dropping commands may be unacceptable.

Distinguish event semantics before selecting a queue policy.

---

## 20. Resource Management

All resources must have clear ownership and cleanup behavior.

Consider:

- Wayland connections.
- Surfaces.
- File descriptors.
- Child processes.
- Channels.
- Tasks.
- Audio devices.
- Network connections.
- Temporary files.
- Locks.
- Shared memory.
- GPU resources.

Prefer RAII and ownership-based cleanup.

Avoid manual resource management when Rust ownership can safely represent the lifecycle.

Do not rely exclusively on process termination for cleanup.

---

