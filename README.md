# bevy_composable_dragging

[![Crates.io](https://img.shields.io/crates/v/bevy_composable_dragging.svg)](https://crates.io/crates/bevy_composable_dragging)
[![docs.rs](https://img.shields.io/docsrs/bevy_composable_dragging)](https://docs.rs/bevy_composable_dragging)
[![Bevy](https://img.shields.io/badge/Bevy-0.20-232326)](https://bevyengine.org/)
[![Rust](https://img.shields.io/badge/rust-1.87%2B-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/crates/l/bevy_composable_dragging.svg)](LICENSE-MIT)
[![Last commit](https://img.shields.io/github/last-commit/xenacool/bevy_composable_dragging)](https://github.com/xenacool/bevy_composable_dragging/commits)

Declarative, composable dragging for [Bevy](https://bevyengine.org) 0.20, ported from
[Dragology](https://declarative-dragging.github.io).

You describe your interface as a pure function from state to a `View`, and attach a *drag spec* to
every node that can be dragged. The engine renders the target states speculatively, finds the
dragged node by id in each of them, and synthesizes the preview while dragging and the new state on
release. There are no hand-written drop zones and no manual hit testing.

```rust
use bevy::prelude::*;
use bevy_composable_dragging::ops::between;
use bevy_composable_dragging::{DraggablePlugin, View, ViewNode};

fn view(on: &bool) -> View<bool> {
    let x = if *on { 1.0 } else { -1.0 };
    View::new().with(
        ViewNode::circle("knob", 1.0)
            .at_xy(Vec2::new(x, 0.0))
            .draggable(|| between([false, true])),
    )
}

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(DraggablePlugin::new(false, view))
        .run();
}
```

Run any example with `cargo run --example <name>`; the gradient examples need their feature, for
example `cargo run --example gradient_hanoi_3d --features shapes-3d`.

## Operators

The operators follow Figure 4 of the paper. Specs are plain data that compile to a per-frame
behavior, so they compose. See the rustdoc in [`ops`](bevy_composable_dragging::ops) for the full
list: `state`, `between`, `closest`, `vary`, `substate`, `react_to`, `along_axis`, `around_axis`,
`across_plane`, `drop_target`, `switch_to_state_and_follow`, `custom`, `button`, and the builder
methods `.with_floating`, `.when_far`, `.with_snap_radius`, `.on_drop`, `.with_chaining`,
`.with_overlay`, `.during`, `.change_gap`, `.change_result`, `.change_frame`, and
`.with_init_context`.

## Drag spaces and transforms

A node has a geometry, a Bevy `Transform` and, if it can be dragged, a *drag space*. A drag
resolves the pointer ray to a world point in that space, and operators read and write the
transforms of nodes directly: there is no grid and no 2D projection step. `ViewNode::draggable`
drags in the XY plane (`DragPlane`); `ViewNode::draggable_in` takes a `DragSpace` instead:

- `DragSpace::plane(origin, normal)`: a plane with any origin and normal, such as the ground.
- `DragSpace::axis(origin, direction)`: the point of an axis line nearest to the pointer ray.
- `DragSpace::screen()`: a plane through the node that faces the camera, fixed when the drag
  starts.

`closest`, `between`, `with_floating` (which lifts along the normal of the space), `with_snap_radius`
and the other operators work unchanged in any space: gaps are measured after projecting positions
onto the space, and the 2D weights of `between` use the local basis of the space. A press picks the
node whose oriented bounds the pointer ray hits first (`View::hit_test`) and grabs its nearest
draggable ancestor. Builders take full vectors and quaternions (`at`, `scaled`, `with_rotation`,
`with_transform`), with `_xy` shorthands for 2D scenes (`at_xy`, `scaled_xy`, `with_rotation_xy`).

## Gizmos

`gizmo::Gizmo` (behind the `gradmesh` feature) adds translate, rotate and scale handles around a
node as ordinary draggable nodes: arrows and plane squares, rings, and cubes, with ids such as
`cube/gizmo/translate-x`. Each handle drags in its own axis or plane space through `along_axis`,
`around_axis` or `across_plane`, and the `Snap` you pass pulls the preview onto the steps and
commits the exact step on release. `scale_uniform` adds one more cube beside the pivot that scales
all three axes together: it drags in the plane that faces the camera, and its factor grows by one
for every unit the pointer moves away from the pivot. Because the handles are nodes, they fade in
and out when the tool changes. See `examples/gizmo_3d.rs`.

An editor usually shows the gizmo of the selection only for the tools that transform it. A state
says so by implementing `gizmo::GizmoReveal`, and `Gizmo::add_if_revealed` adds the handles only
then.

Gizmo handles live on render layer [`gizmo::GIZMO_LAYER`](bevy_composable_dragging::gizmo::GIZMO_LAYER)
(31) so they are always picked first, even when scene geometry covers them. The common camera
helpers (`spawn_cameras`, `spawn_perspective_scene`) include this layer automatically.

## Editor building blocks

Four more modules (behind the `gradmesh` feature) are generic over the state of your interface and
plug in through traits, so an editor reuses them instead of writing its own:

- `hud`: a HUD of ordinary nodes. `Screen` is the size of the HUD and the projection of the scene
  camera, `Layout` places corners and fans petals on screen by their `Footprint`s (the petal and
  its label) into a `Fan` of one or more rings, and `Theme` holds the colors, border,
  tween and render layer of the HUD with the builders of its bordered shapes. A state that
  implements `HudHost` keeps its `Screen` in step with the window through `HudPlugin`, which reads
  the camera marked `HudProjectionCamera` (or one you name).
- `radial`: a radial menu with a hub, rings of petals, a status panel and `Text2d` labels
  (`RadialMenuPlugin`). Implement `Petal` for the items, `Ring` for the rings and `RadialHost` for
  the state, and `push_menu` adds the whole menu to a view. The items can be anything: the menu does
  not know what a petal does.
- `heightmap`: an editable terrain that needs no rendering feature. A `Heightmap` is a grid of
  shared chunks over a `Grid` (chunk count, chunk side, origin and cell size), with a hole mask. A
  `Brush` raises and lowers it between limits you give, `surface`, `height_at` and `normal_at`
  read it, `hit` finds where a ray meets it, and `chunk_heights` and `chunk_key` tell a renderer
  what to build and when a chunk's mesh is out of date.
- `handles`: `CurveHandles` edits a 3D curve with gems that move control points across the ground,
  cones that raise them, gems that insert points and, for the chosen point, a width handle and a
  roll ring, all built on `across_plane`, `along_axis` and `around_axis`.

HUD elements are `Geometry::Sdf(SdfShape)` nodes (`ViewNode::sdf`): rounded rectangles and
ellipses with a border, drawn per pixel so their edges stay crisp at any size.
`ViewNode::with_overshoot` makes the transform of a node run past its target by a fraction of the
distance and settle back when an animation ends; the HUD uses twelve percent.

`ViewNode::group` creates an invisible parent node for organizing children without drawing anything.
`ViewNode::pickable(false)` makes a node invisible to picking, so it renders but never captures
presses.

## Blending

While you drag, the engine blends the views of the states the drag can reach. Every animatable property of a node implements the `Blend` trait: transforms (rotation along the shortest arc), colors (in Oklab, the closest Bevy space to Dragology's Lab), opacity, gradient `Style`s and `Geometry`. A node that exists in only one of the two views follows its `Presence`: `Fade` (the default, as in Dragology) scales its opacity and drops it once it is invisible, `Emerge` grows it from another node (Dragology's `dragologyEmergeFrom`), and `Pop` switches at the midpoint. Opacity is multiplied through a node's ancestors, as in SVG. When more than two views are blended, the parent, drag spec, presence, rendering mode and render layer come from the heaviest view. Implement `Blend` for your own types to animate them the same way.

## Rendering modes and a HUD

`DraggablePlugin::with_rendering` sets how nodes are drawn by default: `Rendering::Spatial` as `Mesh3d` entities for a `Camera3d`, or `Rendering::Flat` as `Mesh2d` entities for a `Camera2d`. A single node overrides the default with `ViewNode::spatial()` or `ViewNode::flat()` (behind `gradmesh`), and `ViewNode::on_layer(n)` puts it on a Bevy render layer. Together they build a HUD from ordinary nodes over a 3D scene: give the HUD nodes `.flat().on_layer(HUD)` and add a `Camera2d` with `RenderLayers::layer(HUD)` and a higher `order` next to the scene camera (`examples/common` has `spawn_hud_camera`). Each camera only draws the nodes on its own layers, and a press from it only picks those nodes, so the HUD and the scene never take presses from each other. The mode and layer are plain data, so they blend like everything else: they come from the nearer view of a pair and from the heaviest view of an n-way blend.

## Differences from Dragology

The port aims to be faithful to the operators and behavior of Dragology. These differences are
deliberate:

- **Solver.** Dragology minimizes with COBYLA. The only COBYLA crate for Rust declares no license,
  so `vary` uses a Nelder–Mead search with a penalty stage instead (`src/solver.rs`). It reproduces
  the cases of Dragology's `optimization.test.ts` that are ported into the unit tests, including
  equality lines and a pin on a fixed distance from a point. It is meant for the one to three
  parameters that drags vary; with many parameters or hard curved constraints COBYLA may do better.
- **`between` drop rule.** Dragology drops onto the target nearest the pointer. The default here
  drops onto the target with the largest blend weight; `DropRule::NearestTarget` selects
  Dragology's rule.
- **`drop_target` bounds.** The default measures the target in the state the drag started from, so
  it keeps working when a drop removes the target. Dragology measures it in the target state;
  `drop_target_with_bounds` does that.
- **`with_overlay`.** The overlay is a list of ordinary non-draggable nodes, in the `overlay-`
  namespace, instead of SVG. There is no tree view or overlay visualization of operators.
- **`with_chaining`** and the chaining of `with_snap_radius_options` need a state that can be
  compared with `PartialEq`, because Dragology compares drop and start state with deep equality.
- **`substate`** previews of the sub-spec are not draggable, because a drag factory of the part
  cannot be turned into one of the whole.
- **`tactile_tessellations`.** A new tile starts at its palette item instead of the canvas
  origin, the dragged tile is raised by the spec instead of through a z-index from the dragged
  id, the center of the tile follows the pointer by resetting the grab offset (Dragology's
  `withInitContext({ anchorPos })`), and the outline of a polygon is a larger polygon behind it.
  Snap candidates, the overlap test, rotation along the shortest arc, `when_far` and the trash
  bin behave as in the original. Only the maintainer can compare the animations side by side.
- **3D drag spaces and gizmos are additions.** Dragology drags in the plane of the page. Here a
  drag resolves the pointer in a `DragSpace` (plane, axis or camera-facing plane), operators work
  on world positions and `Transform`s, nodes are picked by oriented bounds, and the value
  operators and `Gizmo` build on that. `Geometry::Grad` takes any `bevy_gradmesh` shape where
  Dragology draws SVG elements; text, filters and drop shadows have no counterpart.
- **Blending of different point counts.** Dragology morphs paths with different segment counts;
  `bevy_gradmesh` 0.3 resamples polygons of different lengths in `Shape::lerp`, so tiles that
  change their number of sides morph here too. Shapes of different kinds still come from the
  nearer state.
- **Not ported:** the SVG and React layer, the tree and path utilities, the studio and benchmark
  pages, `amb` (a helper of the demos), and the studio demos nodes and noodles, sticky notes, twisted
  trees, animate algebra and the spec designer.

The test cases of Dragology's `closest.test.tsx`, `userOps.test.tsx`, `math/delaunay.test.ts`,
`math/natural-neighbor.test.ts` and `math/optimization.test.ts` are ported into the unit tests of
`closest`, `custom`, `weights` and the solver.

## Gallery

| | | |
|---|---|---|
| ![toggle](https://raw.githubusercontent.com/xenacool/bevy_composable_dragging/main/screenshots/screenshot_toggle.webp) | ![three_way](https://raw.githubusercontent.com/xenacool/bevy_composable_dragging/main/screenshots/screenshot_three_way.webp) | ![reorder_list](https://raw.githubusercontent.com/xenacool/bevy_composable_dragging/main/screenshots/screenshot_reorder_list.webp) |
| `toggle`: `between` | `three_way`: `between` over three states | `reorder_list`: `closest` + floating |
| ![ring_of_beads](https://raw.githubusercontent.com/xenacool/bevy_composable_dragging/main/screenshots/screenshot_ring_of_beads.webp) | ![slider](https://raw.githubusercontent.com/xenacool/bevy_composable_dragging/main/screenshots/screenshot_slider.webp) | ![clock](https://raw.githubusercontent.com/xenacool/bevy_composable_dragging/main/screenshots/screenshot_clock.webp) |
| `ring_of_beads`: `closest` on a ring | `slider`: `vary` with a range constraint | `clock`: `vary` one angle |
| ![scheduler](https://raw.githubusercontent.com/xenacool/bevy_composable_dragging/main/screenshots/screenshot_scheduler.webp) | ![lists_in_lists](https://raw.githubusercontent.com/xenacool/bevy_composable_dragging/main/screenshots/screenshot_lists_in_lists.webp) | ![tessellations](https://raw.githubusercontent.com/xenacool/bevy_composable_dragging/main/screenshots/screenshot_tessellations.webp) |
| `scheduler`: `closest` + `vary` | `lists_in_lists`: rows nested to any depth, `when_far` snap-or-free, `with_overlay` insertion marker | `tessellations`: toolbar and trash bin |
| ![gradient_hanoi_3d](https://raw.githubusercontent.com/xenacool/bevy_composable_dragging/main/screenshots/screenshot_gradient_hanoi_3d.webp) | ![gradient_kanban_2d](https://raw.githubusercontent.com/xenacool/bevy_composable_dragging/main/screenshots/screenshot_gradient_kanban_2d.webp) | ![hello](https://raw.githubusercontent.com/xenacool/bevy_composable_dragging/main/screenshots/screenshot_hello.webp) |
| `gradient_hanoi_3d`: `shapes-3d` | `gradient_kanban_2d`: `shapes-2d` | `hello`: minimal `state` example |
| ![chain_of_cells](https://raw.githubusercontent.com/xenacool/bevy_composable_dragging/main/screenshots/screenshot_chain_of_cells.webp) | ![blend_square](https://raw.githubusercontent.com/xenacool/bevy_composable_dragging/main/screenshots/screenshot_blend_square.webp) | ![tactile_tessellations](https://raw.githubusercontent.com/xenacool/bevy_composable_dragging/main/screenshots/screenshot_tactile_tessellations.webp) |
| `chain_of_cells`: `with_chaining`, overlay underline, drop transition | `blend_square`: `between_with` natural neighbor and sharpness | `tactile_tessellations`: edge-to-edge snaps with rotation |
| ![gizmo_3d](https://raw.githubusercontent.com/xenacool/bevy_composable_dragging/main/screenshots/screenshot_gizmo_3d.webp) | ![cozy_castle](https://raw.githubusercontent.com/xenacool/bevy_composable_dragging/main/screenshots/screenshot_cozy_castle_menu.webp) | ![kart_racer_editor](https://raw.githubusercontent.com/xenacool/bevy_composable_dragging/main/screenshots/screenshot_kart_racer_editor_menu.webp) |
| `gizmo_3d`: translate, rotate and scale gizmos in 3D drag spaces | `cozy_castle`: terrain, walls and radial menu | `kart_racer_editor`: track editor with sweep road and heightfield |
| ![physics_lab](https://raw.githubusercontent.com/xenacool/bevy_composable_dragging/main/screenshots/screenshot_physics_lab.webp) | ![cozy_castle](https://raw.githubusercontent.com/xenacool/bevy_composable_dragging/main/screenshots/screenshot_cozy_castle.webp) | ![kart_racer_editor](https://raw.githubusercontent.com/xenacool/bevy_composable_dragging/main/screenshots/screenshot_kart_racer_editor.webp) |
| `physics_lab`: 3D physics shapes dragged onto a grid, simulated with avian | `cozy_castle`: empty ground before building | `kart_racer_editor`: track view without menu |

`tessellations` snaps tiles to a grid and shows a toolbar and chaining, while `tactile_tessellations` is a port of Dragology's demo of the same name: regular polygons that snap edge to edge with rotation, `closest` with `when_far`, a palette and a trash bin.

`make screenshots` regenerates every image (headless with `xvfb-run` when no display is available).

## Features

| Feature | Description |
|---|---|
| `gradmesh` | Adds `Geometry::Grad(Shape)`: any [`bevy_gradmesh`](https://github.com/xenacool/bevy_gradmesh) 0.3 shape, 2D or 3D, as the geometry of a node (`ViewNode::grad`, `sphere`, `cuboid`, `cylinder`, `cone`, `torus`, `disc`, `ring`, `star`). Meshes are cached by shape, nodes are picked by the real bounds of the shape, and blending two views morphs matching shapes with `Shape::lerp` and their gradient `Style`s with `Style::lerp`. `ViewNode::with_style` gives a node its own gradient. |
| `pbr` | `Geometry::Mesh(MeshGeometry)` (`ViewNode::mesh`) draws a mesh you generate yourself, such as terrain. A `MeshGeometry` is a mesh with a `key`: equal keys are the same geometry and share one mesh asset, so give an edited mesh a new key. Bounds are measured from the positions once, picking uses them, and a blend takes the nearer of two meshes. With `gradmesh` it is drawn with the gradient material (give it positions, normals, `UV_0` and optionally vertex colors); it is a stopgap for `bevy_gradmesh` shapes built from meshes. |
| `shapes-3d` | Enables `gradmesh`; the name of the 3D examples. |
| `shapes-2d` | Enables `gradmesh`; the name of the flat examples. They use `DraggablePlugin::with_rendering(Rendering::Flat)`, which draws every node as a `Mesh2d` with an unlit 2D material under a `Camera2d`. The default `Rendering::Spatial` draws `Mesh3d` entities under a `Camera3d`; any `Geometry` works in both, with the same drags and picking, and single nodes can pick their own mode (see *Rendering modes and a HUD*). |

All three are off by default; the core examples use plain Bevy meshes (`Geometry::Rectangle` and `Geometry::Circle`).


## Credits

- **Dragology** and the paper *Declarative Dragging* by Joshua Horowitz, Andrew Blinn and Jeffrey
  Heer (UIST '26) are the source of the model, the operators and several of the demos. Dragology is
  open source, and this crate is a port of its published source and paper: nothing here is reverse
  engineered. See <https://github.com/declarative-dragging/dragology>. Please cite the paper if
  you build on this.
- **[bevy_gradmesh](https://github.com/xenacool/bevy_gradmesh)** provides the optional gradient
  shapes. `examples/common/screenshot.rs` is copied from it, from the same maintainer, and keeps
  its original header.
- **[Bevy](https://bevyengine.org)** is the engine this crate is built on.
