# bevy_composable_dragging

[![Crates.io](https://img.shields.io/crates/v/bevy_composable_dragging.svg)](https://crates.io/crates/bevy_composable_dragging)
[![docs.rs](https://img.shields.io/docsrs/bevy_composable_dragging)](https://docs.rs/bevy_composable_dragging)
[![Bevy](https://img.shields.io/badge/Bevy-0.20-232326)](https://bevyengine.org/)
[![Rust](https://img.shields.io/badge/rust-1.87%2B-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/crates/l/bevy_composable_dragging.svg)](LICENSE-MIT)
[![Last commit](https://img.shields.io/github/last-commit/xenacool/bevy_composable_dragging)](https://github.com/xenacool/bevy_composable_dragging/commits)

A generic drag-and-drop editor toolkit for 2D and 3D shapes in [Bevy](https://bevyengine.org) 0.20,
ported from [Dragology](https://declarative-dragging.github.io). Gradient meshes are provided by
[bevy_gradmesh](https://github.com/xenacool/bevy_gradmesh).

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

## Screenshots

All example screenshots are packaged in a zip archive attached to each
[GitHub Release](https://github.com/xenacool/bevy_composable_dragging/releases). Download the
latest `screenshots-v*.zip` for still PNGs and animated GIFs.

### Examples

- `blend_square` — `between_with` natural neighbor and sharpness
- `chain_of_cells` — `with_chaining`, overlay underline, drop transition
- `clock` — `vary` one angle
- `constellation` — `substate` — focus one star while lines stretch and others stay draggable
- `cozy_castle` — Pieces are chosen from a radial menu and dragged onto one another, and every
  position along the drag is evaluated speculatively, so the rooms of the first floor change while
  the pointer is still down. An archway dropped on a swept wall carves a hole in it.
- `dag` — `substate` — drag nodes and arrowheads while keeping the graph acyclic
- `gizmo_3d` — Translate, rotate and scale gizmos in 3D drag spaces
- `gradient_hanoi_3d` — `shapes-3d`
- `gradient_kanban_2d` — `shapes-2d`
- `kart_racer_editor` — A track editor whose sweep road is scaled, whose curve is pulled by its
  handles while the road follows, and whose tools are picked from a radial menu.
- `lists_in_lists` — Rows nested to any depth, `when_far` snap-or-free, `with_overlay` insertion marker
- `physics_lab` — 3D physics shapes dragged onto a grid, simulated with avian
- `reorder_list` — `closest` + floating
- `ring_of_beads` — `closest` on a ring
- `scheduler` — `closest` + `vary`
- `slider` — `vary` with a range constraint
- `tactile_tessellations` — Edge-to-edge snaps with rotation
- `tessellations` — Toolbar and trash bin
- `three_way` — `between` over three states
- `toggle` — `between`

## Interactive Demos

Try these examples in your browser at the [demo gallery](https://xenacool.github.io/demos/gallery.html).

## Credits

- **Dragology** and the paper *Declarative Dragging* by Joshua Horowitz, Andrew Blinn and Jeffrey
  Heer (UIST '26) are the source of the model, the operators and several of the demos. Dragology is
  open source, and this crate is a port of its published source and paper: nothing here is reverse
  engineered. See <https://github.com/declarative-dragging/dragology>. Please cite the paper if
  you build on this.
- **[bevy_gradmesh](https://github.com/xenacool/bevy_gradmesh)** provides the optional gradient
  shapes.
- **[Bevy](https://bevyengine.org)** is the engine this crate is built on.

