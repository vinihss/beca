# Lifecycle, Ownership and Shutdown

> Parte das regras de runtime do Beca. Índice: [README.md](README.md).

## 2. Runtime Architecture Principles

Always consider the following principles:

1. The shell is a long-running application.
2. Runtime lifecycle must be explicit.
3. Initialization order must be deterministic.
4. Shutdown must be graceful.
5. Components must have clear ownership.
6. Resources must have explicit lifecycles.
7. Events must have defined semantics.
8. UI rendering must remain responsive.
9. Blocking operations must not interfere with UI or event processing.
10. Background tasks must have owners.
11. Cancellation must be supported where appropriate.
12. State ownership must be clear.
13. Cross-component communication must be intentional.
14. Failure handling must be designed.
15. External system dependencies must be isolated.
16. Compositor-specific behavior must not spread throughout the application.
17. Runtime coordination must not become a central god-object.
18. The system must not depend on uncontrolled global mutable state.
19. Components must not create unmanaged background work.
20. Architectural complexity must remain proportional to the requirements.

---

## 3. Shell Lifecycle

The shell must have an explicit lifecycle model.

Do not allow lifecycle behavior to emerge accidentally from initialization code, arbitrary tasks, or implicit global state.

### 3.1 Lifecycle states

Use a lifecycle model appropriate to the application.

A possible conceptual lifecycle:

```text
Created
   |
   v
Initializing
   |
   v
Starting
   |
   v
Running
   |
   +-------> Degraded
   |            |
   |            +-------> Running
   |            |
   |            +-------> Stopping
   |
   v
Stopping
   |
   v
Stopped
```

Failure may transition the runtime into an appropriate failure or shutdown state.

Do not necessarily implement this exact enum or state machine unless the project needs it. The lifecycle must nevertheless be well-defined.

### 3.2 Lifecycle responsibilities

Each lifecycle phase must have clear responsibilities.

#### Created

- Runtime objects exist.
- Configuration has not necessarily been loaded.
- External resources have not necessarily been acquired.

#### Initializing

- Load and validate configuration.
- Initialize logging and tracing.
- Initialize required foundational services.
- Prepare dependencies.
- Validate runtime requirements.
- Detect environment capabilities where appropriate.

#### Starting

- Initialize runtime components.
- Connect to external services.
- Initialize system integrations.
- Start required background tasks.
- Initialize UI subsystems.
- Register required event handlers.
- Create initial shell surfaces.
- Establish initial runtime state.

#### Running

- Process events.
- Update state.
- Render UI.
- Monitor services.
- Handle user interactions.
- Maintain system integrations.
- Execute background work.

#### Degraded

- One or more non-critical components have failed.
- The shell should continue operating where possible.
- Failure should be observable.
- Recovery may be attempted according to component policy.

#### Stopping

- Stop accepting new work where appropriate.
- Notify components of shutdown.
- Cancel owned tasks.
- Terminate or finalize processes where required.
- Destroy surfaces and release resources.
- Disconnect external integrations.
- Flush required logs or state.

#### Stopped

- Runtime resources have been released.
- Owned tasks have completed or been terminated according to policy.
- No unmanaged background work remains.

### 3.3 Lifecycle rules

- Startup must be deterministic.
- Initialization dependencies must be explicit.
- Do not start dependent components before their prerequisites are ready.
- Do not create UI surfaces before the required Wayland connection and integration are initialized.
- Do not start background tasks before their owning component is initialized.
- Do not allow partially initialized components to appear as fully operational.
- Define what happens when initialization fails.
- Define which failures are fatal and which are recoverable.
- Define how shutdown propagates.
- Avoid duplicated lifecycle logic across crates.

---

## 4. Runtime Composition and Ownership

The runtime composition root is responsible for assembling the system.

It must:

- Construct components.
- Configure dependencies.
- Establish communication channels.
- Define component ownership.
- Start components in the correct order.
- Manage runtime lifecycle.
- Coordinate shutdown.
- Handle fatal failures.

It must not contain business logic or become a giant runtime manager implementing every feature.

### 4.1 Component ownership

Every runtime component must have a clear owner.

The owner is responsible for:

- Initialization.
- Runtime execution.
- Resource management.
- Cancellation.
- Error reporting.
- Shutdown.
- Cleanup.

Avoid creating background tasks without a clearly defined owner.

### 4.2 Task ownership

Every spawned task must belong to a component or task group.

The agent must identify:

- Who creates the task.
- Who supervises it.
- How its errors are handled.
- How it is cancelled.
- Whether it may restart.
- How it terminates.
- Whether it owns resources.

Do not spawn detached tasks without a deliberate lifecycle policy.

### 4.3 Component isolation

Components must not directly control the lifecycle of unrelated components.

Use runtime coordination, explicit contracts, or communication channels where appropriate.

Avoid tightly coupled lifecycle chains in which failure of an unrelated component unexpectedly shuts down the entire shell.

---

## 17. Shutdown and Cleanup

Shutdown must be deliberate.

### Shutdown sequence

A conceptual shutdown sequence:

```text
Shutdown Requested
        |
        v
Stop Accepting New Work
        |
        v
Notify Components
        |
        v
Cancel Owned Tasks
        |
        v
Stop Owned Services
        |
        v
Finalize Processes
        |
        v
Destroy Surfaces
        |
        v
Release System Connections
        |
        v
Flush Required State / Logs
        |
        v
Runtime Stopped
```

Actual order must respect dependency relationships.

### Shutdown rules

- Shutdown must be idempotent where possible.
- Avoid multiple conflicting shutdown paths.
- Do not wait indefinitely.
- Use appropriate timeouts.
- Avoid deadlocks.
- Do not leave unmanaged tasks.
- Do not leave shell-owned processes running unintentionally.
- Do not terminate unrelated user applications.
- Release Wayland resources safely.
- Close IPC connections.
- Flush required state.
- Handle partial initialization.
- Handle failures during shutdown.

Shutdown must work even if initialization did not complete successfully.

---

## 18. Configuration Reload

Configuration reload must be explicit.

When configuration can be reloaded:

1. Read the new configuration.
2. Validate it.
3. Compare it with current configuration.
4. Identify affected components.
5. Determine whether changes can be applied dynamically.
6. Apply changes in a controlled sequence.
7. Roll back where appropriate.
8. Update authoritative configuration state.
9. Publish relevant state changes.

Do not mutate configuration partially without a defined failure strategy.

Distinguish:

- Reloadable configuration.
- Restart-required configuration.
- Immutable runtime configuration.

Do not automatically restart the entire shell for every configuration change.

---

