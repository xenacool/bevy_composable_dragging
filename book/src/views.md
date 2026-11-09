# Views and Nodes

A `View<S>` is a flat list of `ViewNode`s, each identified by a `NodeId` (a string). Nothing in a view touches the ECS — it is plain data that the engine evaluates cheaply for many speculative states.

## Building a View

```rust
use bevy_composable_dragging::{View, ViewNode};

fn view(state: &MyState) -> View<MyState> {
    View::new()
        .with(ViewNode::circle("a", 0.5).at_xy(Vec2::new(-1.0, 0.0)))
        .with(ViewNode::circle("b", 0.5).at_xy(Vec2::new(1.0, 0.0)))
}
```

## Node Geometry

Nodes support several geometry types:

| Geometry | Method | Feature |
|----------|--------|---------|
| Circle (2D) | `ViewNode::circle(id, radius)` | `pbr` |
| Rounded rectangle | `ViewNode::rounded_rect(id, size, radius)` | `pbr` |
| Sphere | `ViewNode::sphere(id, radius)` | `shapes-3d` |
| Cuboid | `ViewNode::cuboid(id, size, radius)` | `shapes-3d` |
| Cylinder | `ViewNode::cylinder(id, radius, height)` | `shapes-3d` |
| Cone | `ViewNode::cone(id, radius, height)` | `shapes-3d` |
| Torus | `ViewNode::torus(id, radius, tube)` | `shapes-3d` |
| SDF shape | `ViewNode::sdf(id, shape)` | `gradmesh` |
| Custom mesh | `ViewNode::mesh(id, mesh_geometry)` | `pbr` |

## Transforms and Hierarchy

Nodes support full 3D transforms and parent-child hierarchy:

```rust
ViewNode::sphere("parent", 1.0)
    .at(Vec3::new(0.0, 2.0, 0.0))
    .with(ViewNode::sphere("child", 0.3).at(Vec3::new(1.5, 0.0, 0.0)))
```

Transform methods:
- `at(Vec3)` — full 3D position
- `at_xy(Vec2)` — 2D shorthand
- `with_rotation(Quat)` — full rotation
- `with_rotation_xy(f32)` — Z-axis rotation in radians
- `scaled(Vec3)` — full scale
- `scaled_xy(Vec2)` — uniform 2D scale

## Colors and Styles

```rust
ViewNode::sphere("colored", 1.0)
    .with_color(Color::srgb(0.9, 0.3, 0.2))
```

With the `gradmesh` feature, nodes can carry a `Style` for gradient shading:

```rust
use bevy_gradmesh::Style;

ViewNode::sphere("gradient", 1.0)
    .with_style(Style::dual_directional([0.3, 0.2, 0.1], [0.9, 0.7, 0.5], [0.0, 1.0, 0.0]))
```

## Presence and Opacity

Control how nodes appear and disappear during blends:

```rust
use bevy_composable_dragging::Presence;

ViewNode::sphere("fade", 1.0)
    .with_presence(Presence::Fade)       // fades in/out during blends
    .with_opacity(0.8)                   // semi-transparent
```

Presence options:
- `Presence::Fade` — node fades in and out
- `Presence::Emerge` — node emerges from the dragged node
- `Presence::Pop` — node appears instantly

## Rendering Modes

```rust
use bevy_composable_dragging::Rendering;

// 3D spatial rendering (default)
ViewNode::sphere("spatial", 1.0).spatial()

// 2D flat rendering (requires gradmesh)
ViewNode::sphere("flat", 1.0).flat()
```

## Layers

Nodes live on render layers for picking and rendering isolation:

```rust
ViewNode::sphere("hud", 1.0).on_layer(2)
```

Gizmo handles use layer 31 (`GIZMO_LAYER`) so they are always picked first.

## Next Steps

- Learn how to make nodes draggable with [Operators](./operators.md)
- Control where drags happen with [Drag Spaces](./spaces.md)
