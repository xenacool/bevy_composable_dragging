# Operators

Operators build `DragSpec`s declaratively. They follow Figure 4 of the Dragology paper and compose freely — specs are plain data that compile into per-frame behavior.

## `state` — Switch to a Fixed State

The simplest operator: dragging switches to the given state.

```rust
use bevy_composable_dragging::ops::state;

ViewNode::circle("button", 0.3)
    .draggable(|| state(true))
```

## `between` — Toggle Between States

Drag between a list of discrete states. The preview interpolates between them, and the nearest state wins on release.

```rust
use bevy_composable_dragging::ops::between;

ViewNode::circle("knob", 0.5)
    .draggable(|| between([false, true]))
```

With custom interpolation and drop rules:

```rust
use bevy_composable_dragging::ops::{between_with, BetweenOptions, Interpolation};

between_with([a, b, c, d], BetweenOptions::new()
    .with_interpolation(Interpolation::NaturalNeighbor)
)

*Three states arranged in a triangle. Run `cargo run --example three_way --features pbr`.*

## `closest` — Pick the Nearest State

Evaluate many drag specs and pick the one whose dragged node is closest to the pointer.

```rust
use bevy_composable_dragging::ops::closest_states;

ViewNode::circle("item", 0.3)
    .draggable(|| closest_states((0..10).map(|i| {
        let mut s = state.clone();
        s.positions[i] = i as f32;
        s
    })))
```

*Drag items to reorder a list. Run `cargo run --example reorder_list --features pbr`.*

## `vary` — Continuously Change a Value

Continuously change a numeric parameter of your state as you drag.

```rust
use bevy_composable_dragging::ops::vary;
use bevy_composable_dragging::param;

ViewNode::circle("handle", 0.3)
    .draggable(|| vary(state.clone(), [param!(Slider, value)]))
```

With range constraints, use `vary_constrained`:

```rust
use bevy_composable_dragging::ops::vary_constrained;
use bevy_composable_dragging::{param, constraint};

vary_constrained(
    state.clone(),
    [param!(Slider, value)],
    constraint::in_range(param!(Slider, value), 0.0, 10.0),
)
```

*A continuous slider. Run `cargo run --example slider --features pbr`.*

## `substate` — Delegate to a Nested State

Apply an operator to a field of your state, using a `Lens`:

```rust
use bevy_composable_dragging::ops::{substate, between, Lens};

let lens = Lens::new(
    |s: &MyState| s.inner.clone(),
    |s: &MyState, inner| MyState { inner, ..s.clone() },
);

ViewNode::circle("nested", 0.3)
    .draggable(|| substate(lens.clone(), between([0, 1, 2])))
```

## `drop_target` — Define a Drop Zone

Mark a node as a drop target that accepts drops from other nodes.

```rust
use bevy_composable_dragging::ops::drop_target;

ViewNode::rectangle("zone", Vec2::splat(2.0))
    .draggable(|| drop_target("dropper", dropped_state))
```

## `react_to` — React to External Values

Rebuild the drag spec when an external value changes, such as a modifier key:

```rust
use bevy_composable_dragging::ops::react_to;

ViewNode::circle("reactor", 0.3)
    .draggable(|| react_to(
        || shift_held.load(std::sync::atomic::Ordering::Relaxed),
        |shift| if *shift { /* spec with shift */ } else { /* normal spec */ },
    ))
```

## `button` — Click Without Dragging

A node that activates on press-and-release without moving:

```rust
use bevy_composable_dragging::ops::button;

ViewNode::circle("btn", 0.3)
    .draggable(|| button(clicked_state, 0.1))
```

## `custom` — Full Control

Provide a custom compile function for complete control over drag behavior.

```rust
use bevy_composable_dragging::ops::custom;

ViewNode::circle("custom", 0.3)
    .draggable(|| custom(|context| {
        // Build a DragBehavior from scratch
        // ...
    }))
```

## Value Operators

Three operators for direct value editing along axes and planes:

- **`along_axis`** — Change a value by dragging along a line
- **`around_axis`** — Change a value by dragging around a ring (rotation)
- **`across_plane`** — Change a value by dragging across a plane

These are used by gizmos and curve handles for precise editing with snapping.

## Spec Modifiers

Every `DragSpec` supports modifier methods:

| Modifier | Purpose |
|----------|---------|
| `.with_floating()` | Lift the node along the space normal while dragging |
| `.when_far(background, gap)` | Switch to background spec when pointer is far |
| `.with_snap_radius(radius)` | Snap to the target within this radius |
| `.on_drop(state)` | Override the drop state |
| `.with_chaining()` | Continue dragging in the new state |
| `.with_overlay(fn)` | Show an overlay node during the drag |
| `.during(fn)` | Transform the state only during the drag |
| `.with_init_context(fn)` | Modify the drag context at start |

## Chaining

`.with_chaining()` lets a drag continue seamlessly after switching states:

```rust
use bevy_composable_dragging::ops::closest_states;

ViewNode::circle("item", 0.3)
    .draggable(|| closest_states(states).with_chaining())
```

## Next Steps

- Control where drags happen with [Drag Spaces](./spaces.md)
- Smooth transitions with [Blending](./blending.md)
