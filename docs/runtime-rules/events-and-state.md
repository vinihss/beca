# Events, Commands and State

> Parte das regras de runtime do Beca. Índice: [README.md](README.md).

## 5. Event-Driven Architecture

The shell is event-driven, but not every interaction requires an event bus.

Choose the simplest communication mechanism that satisfies the requirements.

### 5.1 Communication mechanisms

Prefer the following based on the relationship:

| Relationship | Preferred mechanism |
|---|---|
| Local synchronous computation | Direct function call |
| Simple component interaction | Direct API |
| Async operation | Async function |
| Concurrent producer/consumer | Channel |
| Cross-component notification | Event or message |
| System integration | Adapter and event translation |
| Cross-process communication | IPC |
| State observation | Explicit subscription or query API |

Do not use events when a direct call is clearer.

Do not introduce a central event bus without a demonstrated need.

### 5.2 Event classification

Distinguish between event categories.

#### User events

Examples:

- Button clicked.
- Keyboard input.
- Pointer interaction.
- Widget interaction.
- User requested action.

#### UI events

Examples:

- Component mounted.
- Component state changed.
- Surface requested.
- Layout changed.
- Visibility changed.

#### System events

Examples:

- Compositor state changed.
- Monitor added or removed.
- Workspace changed.
- Window appeared or disappeared.
- Configuration changed.
- Audio device changed.

#### Runtime events

Examples:

- Service started.
- Service stopped.
- Task failed.
- Task completed.
- Runtime degraded.
- Runtime recovery started.

#### Process events

Examples:

- Process spawned.
- Process exited.
- Process failed.
- Process terminated.
- Process output received.

#### Domain events

Events representing meaningful internal system state changes.

Do not confuse events with commands.

A command expresses intent.

An event expresses something that has happened.

---

## 6. Commands and Events

Keep commands and events semantically distinct.

### Commands

Examples:

```text
LaunchApplication
TogglePanel
OpenSettings
ReloadConfiguration
StartVoiceCapture
StopVoiceCapture
TerminateProcess
```

Commands:

- Express intent.
- May fail.
- May require validation.
- May produce results.
- May trigger state changes.

### Events

Examples:

```text
ApplicationLaunched
PanelVisibilityChanged
ConfigurationReloaded
VoiceCaptureStarted
VoiceCaptureStopped
ProcessExited
MonitorAdded
WorkspaceChanged
```

Events:

- Represent completed or observed changes.
- Should have clear semantics.
- Should not ambiguously represent an unexecuted request.
- Should carry enough information for consumers.
- Should not expose unnecessary internal implementation details.

Avoid vague names such as:

```text
Update
Changed
Notification
Action
Message
```

unless the context makes their meaning unambiguous.

---

## 7. Event Ownership and Routing

Events must have a clear owner and routing strategy.

The agent must define:

- Event producer.
- Event consumers.
- Event scope.
- Event ordering requirements.
- Delivery semantics.
- Error handling.
- Backpressure behavior.
- Lifecycle.
- Cancellation behavior.

Do not allow every component to subscribe to every event.

Avoid uncontrolled event fan-out.

Prefer explicit subscriptions and bounded communication where possible.

### 7.1 Event boundaries

Keep event boundaries aligned with architectural boundaries.

Examples:

```text
Hyprland Adapter
       |
       v
Hyprland Events
       |
       v
Shell Runtime / State Adapter
       |
       v
Shell State
       |
       +-----> Panel
       |
       +-----> Workspace UI
       |
       +-----> Window UI
```

The UI should not directly parse raw Hyprland IPC events.

External events must be translated into internal representations before being consumed by general shell components.

### 7.2 Event payloads

Event payloads should:

- Be explicit.
- Be strongly typed.
- Contain only required information.
- Avoid unnecessary large data structures.
- Avoid exposing raw protocol types.
- Avoid embedding unrelated state.
- Avoid mutable shared objects unless explicitly justified.

Prefer stable IDs and meaningful domain types.

---

## 8. Event Loop Rules

The event loop is a critical runtime component.

Understand the existing event loop implementation before modifying it.

### 8.1 Event loop responsibilities

The event loop should:

- Process events.
- Coordinate relevant state updates.
- Dispatch work appropriately.
- Maintain responsiveness.
- Handle shutdown signals.
- Avoid blocking operations.
- Respect event ordering requirements.

It should not:

- Perform expensive computations directly.
- Perform blocking I/O.
- Execute long-running operations synchronously.
- Contain all application business logic.
- Become a giant switch statement for every system feature.

### 8.2 Event loop ownership

Define who owns the event loop.

Do not allow multiple components to compete for the same event loop or protocol connection unless the underlying architecture explicitly supports it.

Avoid uncontrolled nested event loops.

Avoid blocking the event loop waiting for another task that depends on the same loop.

### 8.3 Event processing

When processing events:

- Keep handlers short.
- Validate inputs.
- Update state explicitly.
- Delegate long-running operations.
- Avoid blocking.
- Avoid unnecessary allocations.
- Avoid excessive logging.
- Avoid recursive event processing.
- Avoid unbounded event accumulation.

---

## 9. State Management

State must have a clear owner.

Avoid creating multiple authoritative copies of the same state.

### 9.1 State ownership

For each important state variable, determine:

- Which component owns it?
- Who may modify it?
- Who may read it?
- How is it synchronized?
- How is it persisted?
- How is it reconstructed?
- How are transitions validated?

### 9.2 State categories

Distinguish:

#### Runtime state

Exists only while the shell is running.

Examples:

- Active tasks.
- Current panel visibility.
- Active UI interaction.
- Current surface state.

#### System-observed state

Derived from external systems.

Examples:

- Current monitor configuration.
- Active workspace.
- Window state.
- Compositor state.
- Audio device state.

#### User configuration

Persistent preferences.

Examples:

- Panel settings.
- Widget configuration.
- Appearance.
- Keybindings.

#### Derived state

Computed from other state.

Do not store derived state unnecessarily.

### 9.3 State transitions

State changes should be explicit.

Avoid arbitrary mutation from unrelated components.

Where useful, define:

- Valid transitions.
- Transition conditions.
- Side effects.
- Events emitted.
- Failure behavior.

Do not overengineer simple state transitions with elaborate state-machine frameworks unless the complexity justifies them.

### 9.4 External state reconciliation

External system state may change independently of Beca.

Examples:

- Hyprland restarts.
- Monitors are added or removed.
- Windows change.
- Workspaces change.
- Audio devices change.
- External services disconnect.

The shell must not assume that its state remains synchronized forever.

When necessary, implement reconciliation:

1. Receive event or detect connection loss.
2. Determine whether current state may be stale.
3. Query the authoritative system.
4. Reconstruct or update local state.
5. Resolve inconsistencies.
6. Publish the updated internal state.
7. Resume normal operation.

Events should not be the sole source of truth when events can be lost or connections can be interrupted.

---

## 21. System State Synchronization

Beca may observe state from multiple external sources.

Define which source is authoritative for each piece of state.

Examples:

| State | Authoritative source |
|---|---|
| Workspace state | Compositor / Hyprland |
| Window state | Compositor / Hyprland |
| Monitor topology | Wayland / compositor |
| Shell UI state | Beca runtime |
| User preferences | Beca configuration |
| Process status | OS process state |
| Audio device state | Audio subsystem |
| External service status | External service or adapter |

Do not let Beca's internal state permanently override authoritative external state.

When state becomes inconsistent, reconcile rather than assuming the internal copy is correct.

---

