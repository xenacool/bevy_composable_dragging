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
use bevy_composable_dragging::ops::closest_states;

ViewNode::circle("item", 0.3).draggable(|| {
    closest_states(states).with_chaining()
})
```

The `Chaining` struct controls which node and spec continue the drag:

```rust
use bevy_composable_dragging::behavior::Chaining;

Chaining::<MyState>::new()
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

Use application-generated meshes as node geometry (requires the `pbr` feature):

```rust
use bevy_composable_dragging::MeshGeometry;

let mesh_geom = MeshGeometry::new(0, my_mesh);
ViewNode::mesh("terrain", mesh_geom)
```

The `key` (first argument) identifies the mesh: two `MeshGeometry` values with the same key share one mesh asset. Change the key when the mesh content changes.

## Mixing 2D and 3D

Overlay a 2D HUD on a 3D scene by mixing rendering modes:

```rust
use bevy_composable_dragging::SdfShape;

// 3D scene nodes (default)
ViewNode::sphere("object", 1.0)

// 2D HUD nodes
ViewNode::sdf("panel", SdfShape::rect(Vec2::new(200.0, 100.0)).with_corner_radius(8.0))
    .flat()
    .on_layer(2)
```

## Physics Integration

Combine dragging with physics engines like `avian3d`. The `physics_lab` example demonstrates dragging shapes onto a grid, simulating them with rigid bodies, and grabbing bodies during simulation.

## Performance

- Views are plain data — no ECS overhead for speculative evaluation
- Geometry morphing is quantized to 32 steps
- A heightmap is two flat buffers, so a copy is two memory copies and an edit names only the chunks it reaches
- Gizmo handles use a dedicated render layer for fast picking

## Differences from Dragology

The port aims to be faithful to the operators and behavior of Dragology. These differences are deliberate:

- **Solver.** Dragology minimizes with COBYLA. The only COBYLA crate for Rust declares no license, so `vary` uses a Nelder–Mead search with a penalty stage instead (`src/solver.rs`). It reproduces the cases of Dragology's `optimization.test.ts` that are ported into the unit tests, including equality lines and a pin on a fixed distance from a point. It is meant for the one to three parameters that drags vary; with many parameters or hard curved constraints COBYLA may do better.
- **`between` drop rule.** Dragology drops onto the target nearest the pointer. The default here drops onto the target with the largest blend weight; `DropRule::NearestTarget` selects Dragology's rule.
- **`drop_target` bounds.** The default measures the target in the state the drag started from, so it keeps working when a drop removes the target. Dragology measures it in the target state; `drop_target_with_bounds` does that.
- **`with_overlay`.** The overlay is a list of ordinary non-draggable nodes, in the `overlay-` namespace, instead of SVG. There is no tree view or overlay visualization of operators.
- **`with_chaining`** and the chaining of `with_snap_radius_options` need a state that can be compared with `PartialEq`, because Dragology compares drop and start state with deep equality.
- **`substate`** previews of the sub-spec are not draggable, because a drag factory of the part cannot be turned into one of the whole.
- **`tactile_tessellations`.** A new tile starts at its palette item instead of the canvas origin, the dragged tile is raised by the spec instead of through a z-index from the dragged id, the center of the tile follows the pointer by resetting the grab offset (Dragology's `withInitContext({ anchorPos })`), and the outline of a polygon is a larger polygon behind it. Snap candidates, the overlap test, rotation along the shortest arc, `when_far` and the trash bin behave as in the original.
- **3D drag spaces and gizmos are additions.** Dragology drags in the plane of the page. Here a drag resolves the pointer in a `DragSpace` (plane, axis or camera-facing plane), operators work on world positions and `Transform`s, nodes are picked by oriented bounds, and the value operators and `Gizmo` build on that. `Geometry::Grad` takes any `bevy_gradmesh` shape where Dragology draws SVG elements; text, filters and drop shadows have no counterpart.
- **Blending of different point counts.** Dragology morphs paths with different segment counts; `bevy_gradmesh` 0.3 resamples polygons of different lengths in `Shape::lerp`, so tiles that change their number of sides morph here too. Shapes of different kinds still come from the nearer state.
- **Not ported:** the SVG and React layer, the tree and path utilities, the studio and benchmark pages, `amb` (a helper of the demos), and the studio demos nodes and noodles, sticky notes, twisted trees, animate algebra and the spec designer.

The test cases of Dragology's `closest.test.tsx`, `userOps.test.tsx`, `math/delaunay.test.ts`, `math/natural-neighbor.test.ts` and `math/optimization.test.ts` are ported into the unit tests of `closest`, `custom`, `weights` and the solver.

## Gallery

| | | |
|---|---|---|
| ![toggle](screenshots/screenshot_toggle.webp) | ![three_way](screenshots/screenshot_three_way.webp) | ![reorder_list](screenshots/screenshot_reorder_list.webp) |
| `toggle`: `between` | `three_way`: `between` over three states | `reorder_list`: `closest` + floating |
| ![ring_of_beads](screenshots/screenshot_ring_of_beads.webp) | ![slider](screenshots/screenshot_slider.webp) | ![clock](screenshots/screenshot_clock.webp) |
| `ring_of_beads`: `closest` on a ring | `slider`: `vary` with a range constraint | `clock`: `vary` one angle |
| ![scheduler](screenshots/screenshot_scheduler.webp) | ![lists_in_lists](screenshots/screenshot_lists_in_lists.webp) | ![tessellations](screenshots/screenshot_tessellations.webp) |
| `scheduler`: `closest` + `vary` | `lists_in_lists`: rows nested to any depth, `when_far` snap-or-free, `with_overlay` insertion marker | `tessellations`: toolbar and trash bin |
| ![gradient_hanoi_3d](screenshots/screenshot_gradient_hanoi_3d.webp) | ![gradient_kanban_2d](screenshots/screenshot_gradient_kanban_2d.webp) | ![hello](screenshots/screenshot_hello.webp) |
| `gradient_hanoi_3d`: `shapes-3d` | `gradient_kanban_2d`: `shapes-2d` | `hello`: minimal `state` example |
| ![chain_of_cells](screenshots/screenshot_chain_of_cells.webp) | ![blend_square](screenshots/screenshot_blend_square.webp) | ![tactile_tessellations](screenshots/screenshot_tactile_tessellations.webp) |
| `chain_of_cells`: `with_chaining`, overlay underline, drop transition | `blend_square`: `between_with` natural neighbor and sharpness | `tactile_tessellations`: edge-to-edge snaps with rotation |
| ![gizmo_3d](screenshots/screenshot_gizmo_3d.webp) | ![physics_lab](screenshots/screenshot_physics_lab.webp) | ![cozy_castle](screenshots/screenshot_cozy_castle.webp) |
| `gizmo_3d`: translate, rotate and scale gizmos in 3D drag spaces | `physics_lab`: 3D physics shapes dragged onto a grid, simulated with avian | `cozy_castle`: radial menu, stacking rooms speculatively, an arch carved into a swept wall |
| ![kart_racer_editor](screenshots/screenshot_kart_racer_editor.webp) | | |
| `kart_racer_editor`: a pulled curve, a scaled sweep road, hills and a radial menu | | |

`tessellations` snaps tiles to a grid and shows a toolbar and chaining, while `tactile_tessellations` is a port of Dragology's demo of the same name: regular polygons that snap edge to edge with rotation, `closest` with `when_far`, a palette and a trash bin.

## Credits

- **Dragology** and the paper *Declarative Dragging* by Joshua Horowitz, Andrew Blinn and Jeffrey Heer (UIST '26) are the source of the model, the operators and several of the demos. Dragology is open source, and this crate is a port of its published source and paper: nothing here is reverse engineered. See <https://github.com/declarative-dragging/dragology>. Please cite the paper if you build on this.
- **[bevy_gradmesh](https://github.com/xenacool/bevy_gradmesh)** provides the optional gradient shapes. `examples/common/screenshot.rs` is copied from it, from the same maintainer, and keeps its original header.
- **[Bevy](https://bevyengine.org)** is the engine this crate is built on.
