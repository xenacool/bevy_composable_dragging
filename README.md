# bevy_composable_dragging

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
            .at(Vec2::new(x, 0.0))
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
behavior, so they compose.

| Operator | Meaning |
|---|---|
| `state(s)` | A single reachable state. |
| `between([..])` / `between_with(.., options)` | Blends continuously inside the convex hull of the targets. `BetweenOptions` picks the weighting (`Hull`, `Delaunay` or `NaturalNeighbor`), a `sharpness` exponent that pulls towards the nearest target, and the `DropRule` (largest weight, or Dragology's nearest target). |
| `closest([..])` / `closest_states(..)` | Picks the nearest target; `closest_with_options` adds `stickiness` against flicker and `lock_past` to hold the branch that carried the node away. |
| `vary(state, [params])` / `vary_constrained` | Minimizes the pointer-to-node distance over numeric parameters, under constraints. |
| `vary_with(.., VaryOptions)` / `vary_func(..)` | `vary` with pinned values, or over raw numbers that a function turns into a state. |
| `substate(lens, spec)` | Runs a spec for part of the state through a `Lens`. |
| `react_to(getter, build)` | Recompiles a child spec whenever the getter's value changes. |
| `drop_target(..)` / `drop_target_with_bounds(..)` | Id-based drop zones, for example a trash bin; the second measures the target in a state you choose. |
| `switch_to_state_and_follow(..)` | Switches state at the start of a drag, for example to create an item from a toolbar. |
| `.with_floating()` / `.with_floating_options(FloatingOptions)` | The dragged node follows the pointer while the others preview the drop; the options add a ghost at the drop position and a tether on the float. |
| `.when_far(background, gap)` | Switches to another behavior when the pointer is far from every target. |
| `.with_snap_radius(r)` / `.with_snap_radius_options(r, SnapOptions)` | Settles into a target once the pointer is within `r`; the options blend the change over a transition and chain into a new drag once snapped. |
| `.on_drop(state)` / `.on_drop_with(f)` | Rewrites the state that is committed on release. |
| `.with_branch_transition(seconds)` | Animates the change of branch during a drag. |
| `.with_drop_transition(seconds)` | Sets how long the settle animation after a drop takes. |
| `.with_chaining()` / `.with_chaining_options(..)` | Continues the drag from the new state as soon as the drop state changes, so one drag can cross many states. |
| `.with_overlay(f)` | Draws extra, non-draggable nodes above the preview while the spec is active. |
| `.during(f)` | Transforms the drop state on every frame and previews the result (`on_drop_with` that also changes the preview). |
| `.change_gap(f)` / `.change_result(f)` / `.change_frame(f)` | Rewrites the reported gap, the whole frame, or the pointer position before it reaches the inner spec. |
| `.with_init_context(f)` | Changes the `DragContext` (dragged node, start state) before the inner spec compiles. |
| `custom(compile)` | Defines a behavior from scratch; the counterpart of Dragology's `custom` and the same as `DragSpec::new`. |

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
- **Not ported:** the SVG and React layer, the tree and path utilities, the studio and benchmark
  pages, `amb` (a helper of the demos), and the studio demos nodes and noodles, sticky notes, twisted
  trees, animate algebra and the spec designer.

The test cases of Dragology's `closest.test.tsx`, `userOps.test.tsx`, `math/delaunay.test.ts`,
`math/natural-neighbor.test.ts` and `math/optimization.test.ts` are ported into the unit tests of
`closest`, `custom`, `weights` and the solver.

## Gallery

| | | |
|---|---|---|
| ![toggle](screenshots/screenshot_toggle.webp) | ![three_way](screenshots/screenshot_three_way.webp) | ![reorder_list](screenshots/screenshot_reorder_list.webp) |
| `toggle`: `between` | `three_way`: `between` over three states | `reorder_list`: `closest` + floating |
| ![ring_of_beads](screenshots/screenshot_ring_of_beads.webp) | ![slider](screenshots/screenshot_slider.webp) | ![clock](screenshots/screenshot_clock.webp) |
| `ring_of_beads`: `closest` on a ring | `slider`: `vary` with a range constraint | `clock`: `vary` one angle |
| ![scheduler](screenshots/screenshot_scheduler.webp) | ![lists_in_lists](screenshots/screenshot_lists_in_lists.webp) | ![tessellations](screenshots/screenshot_tessellations.webp) |
| `scheduler`: `closest` + `vary` | `lists_in_lists`: `when_far` snap-or-free, `with_overlay` insertion marker | `tessellations`: toolbar and trash bin |
| ![gradient_hanoi_3d](screenshots/screenshot_gradient_hanoi_3d.webp) | ![gradient_kanban_2d](screenshots/screenshot_gradient_kanban_2d.webp) | ![hello](screenshots/screenshot_hello.webp) |
| `gradient_hanoi_3d`: `shapes-3d` | `gradient_kanban_2d`: `shapes-2d` | `hello`: minimal `state` example |

`chain_of_cells` shows `with_chaining`: a token walks across a row of cells in one drag, with an overlay underlining the target cell. Its image is produced by `make screenshots`.

`make screenshots` regenerates every image (headless with `xvfb-run` when no display is available).

## Features

| Feature | Description |
|---|---|
| `shapes-3d` | Adds `ShapeKey::Grad*` solids (sphere, cuboid, cylinder, cone) drawn with the gradient material of [`bevy_gradmesh`](https://github.com/xenacool/bevy_gradmesh) 0.2.0, with `ViewNode::with_style` to give a node its own gradient `Style`; dragging blends the styles of matched nodes with `Style::lerp`. |
| `shapes-2d` | Adds `ShapeKey::Grad*` flat shapes (disc, rectangle, star, ring). Until `bevy_gradmesh` offers `Mesh2d` output they are drawn as quads in a 3D scene under an orthographic camera. |

Both are off by default; the core examples use plain Bevy meshes.

## Credits

- **Dragology** and the paper *Declarative Dragging* by Joshua Horowitz, Andrew Blinn and Jeffrey
  Heer (UIST '26) are the source of the model, the operators and several of the demos. See
  <https://github.com/declarative-dragging/dragology>. Please cite the paper if you build on this.
- **[bevy_gradmesh](https://github.com/xenacool/bevy_gradmesh)** provides the optional gradient
  shapes. `examples/common/screenshot.rs` is copied from it, from the same maintainer, and keeps
  its original header.
- **[Bevy](https://bevyengine.org)** is the engine this crate is built on.
