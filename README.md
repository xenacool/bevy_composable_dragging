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

## Documentation

Read the [book](https://xenacool.github.io/bevy_composable_dragging/) for a guided tour of the API,
or the [API reference on docs.rs](https://docs.rs/bevy_composable_dragging).

Run any example with `cargo run --example <name>`; the gradient examples need their feature, for
example `cargo run --example gradient_hanoi_3d --features shapes-3d`.

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

## Credits

- **Dragology** and the paper *Declarative Dragging* by Joshua Horowitz, Andrew Blinn and Jeffrey
  Heer (UIST '26) are the source of the model, the operators and several of the demos. Dragology is
  open source, and this crate is a port of its published source and paper: nothing here is reverse
  engineered. See <https://github.com/declarative-dragging/dragology>. Please cite the paper if
  you build on this.
- **[bevy_gradmesh](https://github.com/xenacool/bevy_gradmesh)** provides the optional gradient
  shapes.
- **[Bevy](https://bevyengine.org)** is the engine this crate is built on.
