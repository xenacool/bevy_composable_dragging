# Heightmaps

The `heightmap` module provides an editable terrain grid that a brush raises, lowers, and cuts holes in. It needs no rendering feature — the heightmap is plain data, and a renderer builds meshes from the chunks it marks as dirty.

## Creating a Heightmap

```rust
use bevy::math::{UVec2, Vec2};
use bevy_composable_dragging::heightmap::{Brush, Grid, Heightmap};

// 4x4 chunks, 16 vertices per side, 0.5 unit spacing
let grid = Grid::new(UVec2::new(4, 4), 16, Vec2::ZERO, 0.5);

// Flat terrain at height 0, range -1 to 3
let terrain = Heightmap::new(grid, 0.0, -1.0, 3.0);
```

## Editing with a Brush

```rust
let edited = terrain.raised(&Brush {
    center: Vec2::new(4.0, 4.0),  // brush position on the ground
    radius: 2.0,                   // how far it reaches
    amount: 1.0,                   // positive raises, negative lowers
    falloff: 1.0,                  // 1.0 = smooth, 0.0 = hard edge
});
```

## Reading the Terrain

```rust
// Height at a point
let height = terrain.surface(Vec2::new(4.0, 4.0));

// Height and normal for collision
let (h, n) = terrain.height_and_normal_at(Vec2::new(4.0, 4.0));

// Ray intersection for picking
if let Some(hit) = terrain.hit(ray) {
    println!("Hit at {}", hit.position);
}
```

## Chunks and Dirty Tracking

The grid is divided into chunks shared via `Arc`. Copying a heightmap is cheap — only the edited chunks are copied. Track changes with `chunk_key`:

```rust
// Which chunks changed since the last key?
let changed = terrain.chunk_key(previous_key);

for chunk_key in changed {
    // Rebuild the mesh for this chunk
    let heights = terrain.chunk_heights(chunk_key);
    let holes = terrain.chunk_holes(chunk_key);
}
```

## Holes

Cut holes in the terrain without losing the height data underneath:

```rust
// Cut a hole
let with_hole = terrain.cut(&Brush {
    center: Vec2::new(2.0, 2.0),
    radius: 1.0,
    amount: 0.0,
    falloff: 0.0,
});

// Fill it back — the original height returns
let filled = with_hole.fill(&Brush {
    center: Vec2::new(2.0, 2.0),
    radius: 1.0,
    amount: 0.0,
    falloff: 0.0,
});
```

## Integration with Views

Drag a brush across the terrain using `across_plane`:

```rust
use bevy_composable_dragging::ops::across_plane;
use bevy_composable_dragging::DragSpace;

ViewNode::sphere("brush", 0.3)
    .draggable_in(
        DragSpace::plane(Vec3::ZERO, Dir3::Y),
        || across_plane(brush_param, |state, delta| {
            let mut s = state.clone();
            s.brush.center += delta;
            s.terrain = s.terrain.raised(&s.brush);
            s
        }),
    )
```

![Cozy Castle](screenshots/screenshot_cozy_castle_menu.webp)

*Terrain editing with heightmap and radial menu. Run `cargo run --example cozy_castle_3d --features shapes-3d`.*

## Next Steps

- Build UI with [HUDs](./huds.md)
- Add menus with [Radial Menus](./radial.md)
