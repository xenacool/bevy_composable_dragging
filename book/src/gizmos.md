# Gizmos

The `gizmo` module (behind the `gradmesh` feature) adds translate, rotate, and scale handles around a node as ordinary draggable nodes. Each handle uses value operators (`along_axis`, `around_axis`, `across_plane`) with snapping.

## Adding a Gizmo

```rust
use bevy_composable_dragging::gizmo::Gizmo;
use bevy_composable_dragging::ops::Snap;
use bevy_composable_dragging::{View, ViewNode};

fn view(state: &MyState) -> View<MyState> {
    let mut view = View::new();
    view.push(ViewNode::sphere("object", 1.0).at(state.position));
    
    Gizmo::new("object", state.position.extend(Quat::IDENTITY).into())
        .translate(Snap::step(0.1))
        .rotate(Snap::step(0.1))
        .scale_uniform(Snap::step(0.1))
        .add_to(&mut view);
    view
}
```

## Handle Types

| Handle | Shape | Drag Space | Operator |
|--------|-------|------------|----------|
| Translate | Arrow along each axis | Axis | `along_axis` |
| Rotate | Ring around each axis | Axis (angular) | `around_axis` |
| Scale | Cube on each axis | Axis | `along_axis` |
| Uniform scale | Cube at diagonal | Screen plane | `across_plane` |

## Colors

Handles are color-coded by axis: red for X, green for Y, blue for Z.

## Snapping

Every handle accepts a `Snap` that pulls the preview onto discrete steps:

```rust
use bevy_composable_dragging::ops::Snap;

// No snapping — continuous values
Snap::NONE

// Snap to multiples of 0.1
Snap::step(0.1)

// Snap to multiples with an offset
Snap::step_with_offset(0.1, 0.05)
```

## Revealing Gizmos

An editor typically shows the gizmo only for the selected object and only when the active tool transforms it. Implement `GizmoReveal` on your state:

```rust
use bevy_composable_dragging::gizmo::GizmoReveal;

#[derive(Clone)]
struct Editor {
    selected: Option<String>,
    tool: Tool,
}

impl GizmoReveal for Editor {
    fn reveals_gizmo(&self) -> bool {
        self.selected.is_some() && self.tool == Tool::Transform
    }
}
```

Then use `Gizmo::add_if_revealed` instead of `add_to`:

```rust
Gizmo::new("object", transform)
    .translate(Snap::step(0.1))
    .add_if_revealed(&state, &mut view);
```

## Render Layer

Gizmo handles live on render layer 31 (`GIZMO_LAYER`) so they are always picked first, even when scene geometry covers them.

## Camera Helpers

The common camera helpers include the gizmo layer automatically:

```rust
use bevy_composable_dragging::spawn_perspective_scene;

fn setup(mut commands: Commands) {
    spawn_perspective_scene(&mut commands, Vec3::new(5.0, 5.0, 5.0));
}
```

## Footprint

`Gizmo::footprint` returns the 2D bounds of all handles, useful for HUD layout to avoid overlapping elements.

![Gizmo 3D](screenshots/screenshot_gizmo_3d.webp)

*Translate, rotate, and scale gizmo handles on a 3D object. Run `cargo run --example gizmo_3d --features shapes-3d`.*

## Next Steps

- Edit curves with [Curve Handles](./handles.md)
- Edit terrain with [Heightmaps](./heightmaps.md)
