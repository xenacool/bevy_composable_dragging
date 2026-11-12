# Gradient Shapes with bevy_gradmesh

With the `gradmesh` feature (or `shapes-2d` and `shapes-3d`, which enable it) a node can be drawn with any shape of [`bevy_gradmesh`](https://github.com/xenacool/bevy_gradmesh), and a gradient style of its own.

```toml
bevy_composable_dragging = { version = "0.3", features = ["shapes-3d"] }
```

## Shapes as Geometry

`Geometry::Grad(Shape)` draws a node with a gradmesh `Shape`. `ViewNode::grad` takes the shape directly, and the other constructors build common ones:

```rust
use bevy::math::Vec3;
use bevy_composable_dragging::ViewNode;

ViewNode::<()>::sphere("ball", 0.5);
ViewNode::<()>::cuboid("block", Vec3::new(1.0, 0.4, 1.0), 0.05);
ViewNode::<()>::cylinder("disk", 0.5, 0.2);
```

These constructors use medium shape detail. For another detail, or for any shape the constructors do not cover, build the `Shape` yourself and pass it to `ViewNode::grad`.

`ViewNode::group` is an invisible node: it has no look of its own and only gives its children a shared parent, so moving it moves them all.

`ViewNode::sdf` takes an `SdfMaterial2d` and draws a flat signed-distance shape for a `Camera2d`.

## Styles

A node without a style is drawn with a gradient shaded from its color. Give it a `Style` to control the gradient:

```rust
use bevy::prelude::Color;
use bevy_gradmesh::Style;

node.with_color(Color::hsl(200.0, 0.7, 0.55))
    .with_style(Style::dual_directional_up(
        Color::hsl(200.0, 0.8, 0.22),
        Color::hsl(250.0, 0.85, 0.6),
    ))
```

Any `Style` of gradmesh works, so see its documentation for the full list.

## Blending

When a drag previews several states, the engine blends the nodes that exist in both. Styles blend with `Style::lerp` and shapes with `Shape::lerp`, so a gradient shifts and a shape morphs as the pointer travels between states. A node with a style blends with a node without one by taking the gradient derived from its color.

Shape morphs are quantized to 32 steps, which bounds the number of distinct meshes a blend builds.

The `gradient_hanoi_3d` example styles each disk by the peg it rests on, and `gradient_kanban_2d` does the same for cards in columns.
