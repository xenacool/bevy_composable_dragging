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
| `between([..])` | Blends continuously inside the convex hull of the targets. |
| `closest([..])` / `closest_states(..)` | Picks the nearest target; `closest_with_stickiness` resists flicker. |
| `vary(state, [params])` / `vary_constrained` | Minimizes the pointer-to-node distance over numeric parameters, under constraints. |
| `drop_target(..)` | Id-based drop zones, for example a trash bin. |
| `switch_to_state_and_follow(..)` | Switches state at the start of a drag, for example to create an item from a toolbar. |
| `.with_floating()` | The dragged node follows the pointer while the others preview the drop. |
| `.when_far(background, gap)` | Switches to another behavior when the pointer is far from every target. |
| `.with_snap_radius(r)` | Settles into a target once the pointer is within `r`. |
| `.on_drop(state)` / `.on_drop_with(f)` | Rewrites the state that is committed on release. |
| `.with_branch_transition(seconds)` | Animates the change of branch during a drag. |

## Gallery

| | | |
|---|---|---|
| ![toggle](screenshots/screenshot_toggle.webp) | ![three_way](screenshots/screenshot_three_way.webp) | ![reorder_list](screenshots/screenshot_reorder_list.webp) |
| `toggle`: `between` | `three_way`: `between` over three states | `reorder_list`: `closest` + floating |
| ![ring_of_beads](screenshots/screenshot_ring_of_beads.webp) | ![slider](screenshots/screenshot_slider.webp) | ![clock](screenshots/screenshot_clock.webp) |
| `ring_of_beads`: `closest` on a ring | `slider`: `vary` with a range constraint | `clock`: `vary` one angle |
| ![scheduler](screenshots/screenshot_scheduler.webp) | ![lists_in_lists](screenshots/screenshot_lists_in_lists.webp) | ![tessellations](screenshots/screenshot_tessellations.webp) |
| `scheduler`: `closest` + `vary` | `lists_in_lists`: `when_far` snap-or-free | `tessellations`: toolbar and trash bin |
| ![gradient_hanoi_3d](screenshots/screenshot_gradient_hanoi_3d.webp) | ![gradient_kanban_2d](screenshots/screenshot_gradient_kanban_2d.webp) | ![hello](screenshots/screenshot_hello.webp) |
| `gradient_hanoi_3d`: `shapes-3d` | `gradient_kanban_2d`: `shapes-2d` | `hello`: minimal `state` example |

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
