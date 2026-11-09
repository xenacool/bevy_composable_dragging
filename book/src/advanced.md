# Advanced Topics

## Custom Operators

Build a custom operator by returning a `DragSpec` from a function:

```rust
use bevy_composable_dragging::{DragSpec, DragBehavior, DragContext};
use bevy_composable_dragging::behavior::Frame;

fn my_operator<S: Clone>(states: impl IntoIterator<Item = S>) -> DragSpec<S> {
    DragSpec::new(move |context: &DragContext<S>| {
        // Build a DragBehavior that produces Frame values
        // Each frame has a view, drop_state, and gap
        DragBehavior::new(/* ... */)
    })
}
```

## Parameters and Constraints

The `param!` macro builds getter/setter pairs for `f32` fields:

```rust
use bevy_composable_dragging::{param, Param};
use bevy_composable_dragging::constraint;

struct Meeting { start: f32 }
struct Schedule { meetings: Vec<Meeting> }

let second_start = param!(Schedule, meetings[1].start);

// Constrain a parameter to a range
let bounded = constraint::in_range(
    param!(Schedule, meetings[0].start),
    0.0, 24.0,
);
```

Combine constraints:

```rust
use bevy_composable_dragging::constraint::{and, less_than, at_least};

let ordered = and([
    at_least(param!(State, x), 0.0),
    less_than(param!(State, x), param!(State, y)),
]);
```

Set parameter scale for faster solver convergence:

```rust
let scaled = param!(State, pixel_x).with_scale(100.0);
```

## Chaining Drags

`.with_chaining()` continues a drag after switching states:

```rust
use bevy_composable_dragging::ops::closest;
use bevy_composable_dragging::behavior::Chaining;

ViewNode::circle("item", 0.3).draggable(|| {
    closest(|state| /* states */)
        .with_chaining()
})
```

The `Chaining` struct controls which node and spec continue the drag:

```rust
Chaining::new()
    .dragging("new_node")
    .following(new_spec)
    .with_transition(0.2)
```

## Branch Transitions

When a spec switches between branches (e.g., from one target to another), the preview blends:

```rust
DragSpec::new(|ctx| /* ... */)
    .with_branch_transition(0.15)  // 150ms blend
```

## Drop Transitions

Control the animation duration after a drop:

```rust
DragSpec::new(|ctx| /* ... */)
    .with_drop_transition(0.3)  // 300ms animation
```

## Custom Meshes

Use application-generated meshes as node geometry:

```rust
use bevy_composable_dragging::MeshGeometry;

let mesh_geom = MeshGeometry::new(0, my_mesh);
ViewNode::mesh("terrain", mesh_geom)
```

The `key` identifies the mesh: two `MeshGeometry` values with the same key share one mesh asset. Change the key when the mesh changes.

## Mixing 2D and 3D

Overlay a 2D HUD on a 3D scene by mixing rendering modes:

```rust
use bevy_composable_dragging::Rendering;

// 3D scene nodes (default)
ViewNode::sphere("object", 1.0)

// 2D HUD nodes
ViewNode::sdf("panel", SdfShape::rounded_rect(Vec2::new(200.0, 100.0), 8.0))
    .flat()
    .on_layer(2)
```

## Physics Integration

Combine dragging with physics engines like `avian3d`. The `physics_lab` example demonstrates dragging shapes onto a grid, simulating them with rigid bodies, and grabbing bodies during simulation.

## Performance

- Views are plain data — no ECS overhead for speculative evaluation
- Geometry morphing is quantized to 32 steps
- Heightmap chunks are shared via `Arc` — only edited chunks copy
- Gizmo handles use a dedicated render layer for fast picking

## Differences from Dragology

| Dragology | bevy_composable_dragging |
|-----------|--------------------------|
| SVG elements | Bevy meshes and transforms |
| JavaScript | Rust |
| Browser | Bevy renderer |
| SVG hit testing | Ray casting with oriented bounds |
| `lerpSvgx` | `lerp_views` with `Blend` trait |

## Gallery

| Example | Feature | Screenshot |
|---------|---------|------------|
| Hello | `pbr` | ![Hello](screenshots/screenshot_hello.webp) |
| Toggle | `pbr` | ![Toggle](screenshots/screenshot_toggle.webp) |
| Three-way | `pbr` | ![Three-way](screenshots/screenshot_three_way.webp) |
| Reorder list | `pbr` | ![Reorder](screenshots/screenshot_reorder_list.webp) |
| Slider | `pbr` | ![Slider](screenshots/screenshot_slider.webp) |
| Clock | `pbr` | ![Clock](screenshots/screenshot_clock.webp) |
| Scheduler | `pbr` | ![Scheduler](screenshots/screenshot_scheduler.webp) |
| Blend square | `pbr` | ![Blend](screenshots/screenshot_blend_square.webp) |
| Chain of cells | `pbr` | ![Chain](screenshots/screenshot_chain_of_cells.webp) |
| Tessellations | `pbr` | ![Tessellations](screenshots/screenshot_tessellations.webp) |
| Gradient Hanoi 3D | `shapes-3d` | ![Hanoi](screenshots/screenshot_gradient_hanoi_3d.webp) |
| Gradient Kanban 2D | `shapes-2d` | ![Kanban](screenshots/screenshot_gradient_kanban_2d.webp) |
| Gizmo 3D | `shapes-3d` | ![Gizmo](screenshots/screenshot_gizmo_3d.webp) |
| Cozy Castle | `shapes-3d` | ![Castle](screenshots/screenshot_cozy_castle_menu.webp) |
| Kart Racer | `shapes-3d` | ![Kart](screenshots/screenshot_kart_racer_editor_menu.webp) |
