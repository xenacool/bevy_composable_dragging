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

To resist flicker at the boundary between two branches, add stickiness:

```rust
use bevy_composable_dragging::ops::closest_states_with_stickiness;

closest_states_with_stickiness(
    [State { pos: 0 }, State { pos: 1 }, State { pos: 2 }],
    1.0,  // world units of bias toward the current branch
)
```

The current branch is kept until another branch is nearer to the pointer by more than the
stickiness value. A value of zero behaves like plain `closest_states`.

*Three slots with a sticky dot. Run `cargo run --example stickiness --features pbr`.*

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

When the state depends on numbers that are not direct fields, use `vary_func` to vary raw
numbers and build the state from them. For example, a point constrained to a line `y = 2x` needs
only one solver variable:

```rust
use bevy_composable_dragging::ops::{VaryOptions, vary_func};

vary_func(
    [0.0],
    |v| Point { x: v[0], y: v[0] * 2.0 },
    VaryOptions::new().with_scale(10.0),
)
```

The `build` closure receives the current solver values and returns the complete state. The `scale`
option sizes the solver's first steps to match the typical magnitude of those values. `vary_func`
also accepts constraints and pins via [`VaryOptions`](#varyoptions).

## Constraints

The `constraint` module provides helpers that restrict how `vary` changes parameters. Each constraint reports a violation score: zero when satisfied, positive when violated. The solver minimizes both the pointer distance and the total violation.

| Constraint | Meaning |
|------------|---------|
| `constraint::less_than(a, b)` | `a <= b` |
| `constraint::more_than(a, b)` | `a >= b` |
| `constraint::equal(a, b)` | `a == b` |
| `constraint::in_order([a, b, c])` | `a <= b <= c` |
| `constraint::at_least(a, min)` | `a >= min` |
| `constraint::at_most(a, max)` | `a <= max` |
| `constraint::in_range(a, min, max)` | `min <= a <= max` |
| `constraint::and([c1, c2])` | All constraints at once |

For example, `equal` locks two parameters together so dragging one forces the other to match:

```rust
use bevy_composable_dragging::constraint::equal;
use bevy_composable_dragging::ops::vary_constrained;

vary_constrained(
    state.clone(),
    [param!(State, a), param!(State, b)],
    equal(param!(State, a), param!(State, b)),
)
```

*Two knobs locked together. Run `cargo run --example equal_constraint --features pbr`.*

## `substate` — Isolate One Item with a Lens (the Zipper Pattern)

`substate` is the most important operator for collections. When you drag one item in a list, grid, or graph, `substate` isolates that item's state so the inner drag spec operates on a small value instead of the entire collection. This is called the **zipper pattern**: you zoom into one element, manipulate it, and zip back out.

### Why Use Substate

**1. Sibling drag factories are preserved.** This is the primary benefit. Without `substate`, the preview frame strips drag factories from every node except the one being dragged. With `substate`, the library renders the full view (all nodes with their drag factories intact) and only merges the dragged node's transform from the inner spec. The result: while dragging one item, every sibling remains fully interactive and draggable.

**2. Smaller state means less cloning.** Each snap branch or `vary_constrained` baseline clones the state. With `substate`, each branch clones a small struct (e.g., a `Vec2` position or a `Pose { pos, rot }`) instead of the entire collection. For a board with 20 tiles and 15 snap positions, this reduces cloning from 15 full boards to 15 small structs.

**3. Scoped constraints.** Constraints operate on the isolated state, not the full collection. A `no_overlap` constraint sees only the dragged tile's pose, not the entire board. It cannot accidentally read or write another item's state.

### How It Works

A `Lens` defines how to extract and inject the isolated state:

```rust
use bevy_composable_dragging::ops::{substate, closest, state, Lens};

// Extract one tile's position from the full board.
let tile = Lens::new(
    |board: &Board| board.tiles[dragged_id].pos,
    |board: &Board, pos| {
        let mut b = board.clone();
        b.tiles[dragged_id].pos = pos;
        b
    },
);

// The inner spec operates on Vec2, not Board.
substate(tile, closest(branches))
```

The `substate` operator's `lift` function (the zipper) works like this:

1. Render the full view via the view function — this includes all drag factories from all nodes.
2. Run the inner spec to get the dragged node's new transform.
3. Merge only that transform into the full view, leaving every other node untouched.

### Nested Substate

For operations like `when_far` inside `closest`, nest `substate` calls:

```rust
use bevy_composable_dragging::ops::{substate, closest, state, vary_constrained, Lens};

// Outer lens: Board -> Option<TilePose> (None if dropped on trash)
let tile = Lens::new(
    |board: &Board| board.tile_pose(id),
    |board: &Board, pose| board.with_tile(id, pose),
);

// Inner lens: Option<TilePose> -> TilePose (unwrap or use default)
let pose = Lens::new(
    |opt: &Option<TilePose>| opt.unwrap_or(default),
    |_, pose| Some(pose),
);

substate(
    tile,
    closest(branches)
        .when_far(substate(pose, free_drag), GAP),
)
```

The outer `substate` isolates the tile from the board. The inner `substate` unwraps the `Option` so `vary_constrained` operates on a concrete `TilePose`.

### Real Examples

- **`constellation`** — each star's position is isolated via `substate`. While dragging one star, the others stay draggable and their connecting lines stretch naturally.
- **`tactile_tessellations`** — one tile's pose (position + rotation) is isolated. Snap branches are `Some(TilePose)` instead of full board clones. The `no_overlap` constraint scores only the dragged tile's pose.
- **`tactile_tessellations_3d`** — same pattern in 3D: one tile's `Pose` isolates position and rotation on a face grid.
- **`dag`** — an arrowhead's target is isolated via `substate`, snapping to nodes that keep the graph acyclic.
- **`scheduler`** — one meeting's `(lane, start, end)` is isolated from the full schedule. While dragging one meeting, others remain interactive.

### When Not to Use It

If your state IS the value you're dragging (a single scalar, a toggle, a slider), `substate` adds unnecessary indirection. Use it when your state is a collection and you're dragging one element.

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
