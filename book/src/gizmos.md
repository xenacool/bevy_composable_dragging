# Gizmos

The `gizmo` module (behind the `gradmesh` feature) adds translate, rotate, and scale handles around a node as ordinary draggable nodes. Each handle uses value operators (`along_axis`, `around_axis`, `across_plane`) with snapping.

## Adding a Gizmo

```rust
use bevy::prelude::Transform;
use bevy_composable_dragging::gizmo::Gizmo;
use bevy_composable_dragging::ops::Snap;
use bevy_composable_dragging::{View, ViewNode};

fn view(state: &MyState) -> View<MyState> {
    let mut view = View::new();
    view.push(ViewNode::sphere("object", 1.0).at(state.position));
    
    let transform = Transform::from_translation(state.position);
    Gizmo::new("object", transform)
        .translate(Snap::step(0.1), |s, offset| {
            let mut s = s.clone();
            s.position += offset;
            s
        })
        .rotate(Snap::step(0.1), |s, rotation| {
            let mut s = s.clone();
            s.rotation *= rotation;
            s
        })
        .scale_uniform(Snap::step(0.1), |s, scale| {
            let mut s = s.clone();
            s.scale *= scale;
            s
        })
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
    .translate(Snap::step(0.1), |s, offset| {
        let mut s = s.clone();
        s.position += offset;
        s
    })
    .add_if_revealed(&mut view, &state);
```

## Render Layer

Gizmo handles live on render layer 31 (`GIZMO_LAYER`) so they are always picked first, even when scene geometry covers them.

## Camera Helpers

The example helpers in `examples/common` include the gizmo layer automatically:

```rust
use bevy_composable_dragging::examples::common::spawn_perspective_scene;

fn setup(mut commands: Commands) {
    spawn_perspective_scene(&mut commands, Vec3::new(5.0, 5.0, 5.0), Vec3::ZERO);
}
```

## Footprint

`Gizmo::footprint` returns the world positions of all handle centers, useful for HUD layout to keep elements clear of the gizmo.

*Translate, rotate, and scale gizmo handles on a 3D object. Run `cargo run --example gizmo_3d --features shapes-3d`.*

## Next Steps

- Edit curves with [Curve Handles](./handles.md)
- Edit terrain with [Heightmaps](./heightmaps.md)
