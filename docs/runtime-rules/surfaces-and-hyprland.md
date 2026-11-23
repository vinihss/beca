# Wayland Surfaces and Hyprland

> Parte das regras de runtime do Beca. Índice: [README.md](README.md).

## 10. Wayland Surface Lifecycle

Wayland surfaces have a lifecycle that must be handled explicitly.

Do not treat surfaces as ordinary UI objects without considering protocol state.

### 10.1 Surface lifecycle

A conceptual lifecycle:

```text
Create
  |
  v
Configure
  |
  v
Map
  |
  v
Visible / Active
  |
  +-----> Update
  |
  v
Unmap
  |
  v
Destroy
```

Actual lifecycle details depend on the protocol and UI toolkit.

The agent must verify the implementation and protocol behavior before assuming this exact sequence.

### 10.2 Surface ownership

Every surface must have a clear owner.

The owner must manage:

- Creation.
- Configuration.
- Mapping.
- Updates.
- Unmapping.
- Destruction.
- Associated resources.
- Event handling.
- Error handling.

Avoid unmanaged surfaces.

Avoid creating duplicate surfaces unintentionally.

Avoid destroying surfaces while callbacks or tasks still reference them.

### 10.3 Surface state

Track only the state necessary for the implementation.

Possible states include:

- Not created.
- Created.
- Awaiting configuration.
- Configured.
- Mapped.
- Unmapped.
- Destroying.
- Destroyed.

Do not implement an elaborate state machine if the toolkit already provides correct lifecycle abstractions.

### 10.4 Surface lifecycle rules

- Respect compositor configuration.
- Handle configure events correctly.
- Avoid assuming initial dimensions.
- Avoid assuming the surface is immediately ready.
- Handle mapping and unmapping explicitly.
- Handle destruction safely.
- Avoid blocking protocol processing.
- Avoid accessing destroyed resources.
- Avoid duplicate lifecycle transitions.
- Avoid unnecessary recreation.
- Ensure cleanup occurs during shutdown.

---

## 11. Layer Shell Rules

Layer Shell integration must remain isolated.

Layer Shell surfaces may be used for:

- Panels.
- Bars.
- Backgrounds.
- Overlays.
- Notifications.
- Widgets.
- Popups where appropriate.

When creating a layer surface, explicitly consider:

- Layer.
- Anchors.
- Margins.
- Exclusive zone.
- Keyboard interactivity.
- Output selection.
- Size.
- Surface lifecycle.
- Visibility.
- Compositor behavior.

Do not assume every layer surface should reserve space.

Do not assume all shell surfaces should receive keyboard input.

Do not assume every compositor supports identical behavior.

### Exclusive zones

Use exclusive zones intentionally.

- Panels may reserve screen space.
- Overlays may not.
- Hidden surfaces should not unnecessarily reserve space.
- Dynamic visibility changes may require exclusive-zone updates.
- Avoid overlapping reserved regions unintentionally.

### Output selection

When supporting multiple monitors:

- Handle output removal.
- Handle output addition.
- Handle output configuration changes.
- Avoid references to invalid outputs.
- Reconcile surfaces after topology changes.
- Define behavior when the selected output disappears.

---

## 12. Hyprland Integration

Hyprland integration must be isolated behind a dedicated adapter or integration boundary.

Do not allow Hyprland-specific IPC details to leak into every crate.

### 12.1 Integration responsibilities

The Hyprland integration layer should handle:

- IPC connection management.
- Command execution.
- Event subscription.
- Event parsing.
- Connection failures.
- Reconnection.
- Version and capability detection where needed.
- Protocol-specific data conversion.

### 12.2 Internal representation

Convert external Hyprland data into internal types.

Avoid exposing raw JSON, raw IPC strings, or protocol-specific types to general application components.

Use typed representations for:

- Monitors.
- Workspaces.
- Windows.
- Layout state.
- Focus state.
- Relevant system events.

### 12.3 IPC connection lifecycle

The integration must:

- Establish connections explicitly.
- Detect disconnects.
- Handle failures.
- Reconnect when appropriate.
- Avoid busy loops.
- Use bounded retry strategies.
- Avoid unbounded retry accumulation.
- Report connection status.
- Reconcile state after reconnection.

### 12.4 Reconnection

When a connection is lost:

1. Detect failure.
2. Mark integration state appropriately.
3. Notify dependent components.
4. Stop operations that require the connection.
5. Apply retry policy.
6. Reconnect.
7. Reinitialize subscriptions.
8. Query authoritative state.
9. Reconcile local state.
10. Resume normal operation.

Do not assume reconnecting the socket automatically restores all previous subscriptions or state.

### 12.5 Command execution

Commands sent to Hyprland must:

- Be validated.
- Be explicit.
- Be observable.
- Handle errors.
- Avoid unsafe string construction.
- Avoid uncontrolled retries.
- Avoid blocking UI.
- Respect the integration lifecycle.

Do not create a generic command execution mechanism accessible to every component without considering safety and architectural boundaries.

---

