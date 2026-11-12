# Getting Started

## Installation

Add `bevy_composable_dragging` to your `Cargo.toml`:

```toml
[dependencies]
bevy = "0.20"
bevy_composable_dragging = { version = "0.3", features = ["pbr"] }
```

The `pbr` feature enables 3D scenes with PBR materials and mesh picking. For gradient shapes, add `shapes-3d` or `shapes-2d`:

```toml
bevy_composable_dragging = { version = "0.3", features = ["shapes-3d"] }
```

## Your First Draggable Interface

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

This creates a circle that toggles between `x = -1` and `x = 1`. Dragging it smoothly animates between the two states, and releasing commits the nearest one.

## Understanding the API

The core API has three main types:

1. **`View<S>`** -- A flat list of `ViewNode`s describing your scene. Build it as a pure function of your state type `S`.
2. **`ViewNode<S>`** -- A single element with an id, geometry, transform, color, and optional drag spec.
3. **`DragSpec<S>`** -- A declarative description of how a node can be dragged, built with operators from the `ops` module.

## State and View Functions

Your state type `S` holds everything your interface needs. The view function `fn(&S) -> View<S>` must be pure -- no side effects, no external state. The engine calls it many times per frame with speculative states to compute drag previews.

```rust
#[derive(Clone)]
struct Slider {
    value: f32,
}

fn view(state: &Slider) -> View<Slider> {
    View::new()
        .with(ViewNode::circle("handle", 0.3).at_xy(Vec2::new(state.value, 0.0)))
}
```

## Features

| Feature | Description |
|---------|-------------|
| `pbr` | PBR materials and mesh picking for 3D scenes |
| `gradmesh` | Gradient shapes from `bevy_gradmesh` as node geometry |
| `shapes-2d` | Flat gradient shapes for 2D scenes with `Camera2d` |
| `shapes-3d` | Gradient shapes for 3D scenes with `Camera3d` |

## Next Steps

- Learn about [Views and Nodes](./views.md) -- building scenes with geometry, transforms, and hierarchy
- Explore [Operators](./operators.md) -- the declarative drag behaviors
- Try [Drag Spaces](./spaces.md) -- planes, axes, and screen-facing dragging
