# Curve Handles

The `handles` module (behind the `gradmesh` feature) provides `CurveHandles` for editing 3D curves with draggable control points, width handles, and roll rings.

## Adding Curve Handles

```rust
use bevy_composable_dragging::handles::{CurveHandles, Lateral};
use bevy_composable_dragging::ops::Snap;
use bevy_composable_dragging::{View, ViewNode};

#[derive(Clone)]
struct Road {
    points: Vec<Vec3>,
    half_widths: Vec<f32>,
    selected: Option<usize>,
}

fn view(road: &Road) -> View<Road> {
    let mut view = View::new();
    view.push(ViewNode::cuboid("road", Vec3::new(1.0, 0.1, 8.0), 0.0));
    
    let lateral: Vec<Lateral> = road.half_widths.iter()
        .map(|hw| Lateral { right: Vec3::X, half_width: *hw })
        .collect();
    
    CurveHandles::new("road", Transform::IDENTITY, &road.points)
        .selected(road.selected)
        .move_point(Snap::step(0.1), |road, index, offset| {
            let mut road = road.clone();
            road.points[index] += offset;
            road.selected = Some(index);
            road
        })
        .width(&lateral, Snap::step(0.1), |road, index, change| {
            let mut road = road.clone();
            road.half_widths[index] += change;
            road
        })
        .add_to(&mut view);
    view
}
```

## Handle Types

| Handle | Shape | Function |
|--------|-------|----------|
| Control point | Gem (sphere) | Move point across the ground plane |
| Raise | Cone above gem | Raise or lower the point |
| Insert | Small gem between points | Insert a new control point |
| Width | Cube (selected point) | Adjust road width at this point |
| Roll | Torus ring (selected point) | Rotate the cross-section at this point |

## Configuration

```rust
CurveHandles::new("road", transform, &points)
    // Show selected point's extra handles
    .selected(Some(0))
    
    // Move points across the ground
    .move_point(Snap::step(0.1), move_fn)
    
    // Raise/lower points
    .raise_point(Snap::step(0.1), raise_fn)
    
    // Insert new points between existing ones
    .insert_point(insert_fn)
    
    // Adjust width at selected point
    .width(&lateral, Snap::step(0.1), width_fn)
    
    // Roll cross-section at selected point
    .roll(Snap::step(0.05), roll_fn)
```

## Closures

Each handle takes a closure that receives the original state, the point index, and the measured value, and returns the new state. Pressing a handle also selects its point, so the closure can use this for selection.

## Footprint

`CurveHandles::footprint` returns the 2D bounds of all handles, useful for HUD layout.

![Kart Racer Editor](screenshots/screenshot_kart_racer_editor_menu.webp)

*Curve handles editing a race track. Run `cargo run --example kart_racer_editor --features shapes-3d`.*

## Next Steps

- Edit terrain with [Heightmaps](./heightmaps.md)
- Build UI with [HUDs](./huds.md)
