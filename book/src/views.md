# Views and Nodes

A `View<S>` is a flat list of `ViewNode`s, each identified by a `NodeId` (a string). Nothing in a view touches the ECS -- it is plain data that the engine evaluates cheaply for many speculative states.

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
| Circle (2D) | `ViewNode::circle(id, diameter)` | None |
| Rectangle (2D) | `ViewNode::rectangle(id, size)` | None |
| Sphere | `ViewNode::sphere(id, radius)` | `gradmesh` |
| Cuboid | `ViewNode::cuboid(id, size, roundness)` | `gradmesh` |
| Cylinder | `ViewNode::cylinder(id, radius, height)` | `gradmesh` |
| Cone | `ViewNode::cone(id, radius, height)` | `gradmesh` |
| Torus | `ViewNode::torus(id, radius, thickness)` | `gradmesh` |
| Disc | `ViewNode::disc(id, radius)` | `gradmesh` |
| Ring | `ViewNode::ring(id, outer, inner)` | `gradmesh` |
| Star | `ViewNode::star(id, points, outer, inner)` | `gradmesh` |
| SDF shape | `ViewNode::sdf(id, shape)` | `gradmesh` |
| Custom mesh | `ViewNode::mesh(id, mesh_geometry)` | `pbr` |
| Group | `ViewNode::group(id)` | `gradmesh` |

## Transforms and Hierarchy

Nodes support full 3D transforms and parent-child hierarchy. A child node is positioned relative to its parent using `.with_parent()`:

```rust
View::new()
    .with(ViewNode::sphere("parent", 1.0).at(Vec3::new(0.0, 2.0, 0.0)))
    .with(ViewNode::sphere("child", 0.3)
        .with_parent("parent")
        .at(Vec3::new(1.5, 0.0, 0.0)))
```

Transform methods:
- `at(Vec3)` -- full 3D position
- `at_xy(Vec2)` -- 2D shorthand
- `with_rotation(Quat)` -- full rotation
- `with_rotation_xy(f32)` -- Z-axis rotation in radians
- `scaled(Vec3)` -- full scale
- `scaled_xy(Vec2)` -- uniform 2D scale

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
    .presence(Presence::Fade)            // fades in/out during blends
    .with_opacity(0.8)                   // semi-transparent
```

Presence options:
- `Presence::Fade` -- node fades in and out
- `Presence::Emerge` -- node emerges from the dragged node
- `Presence::Pop` -- node appears instantly

## Rendering Modes

```rust
// 3D spatial rendering (default)
ViewNode::sphere("spatial", 1.0).spatial()

// 2D flat rendering (requires gradmesh)
ViewNode::sphere("flat", 1.0).flat()
```

`ViewNode::spatial()` draws a node as a `Mesh3d` for a `Camera3d`, and `ViewNode::flat()` draws it as a `Mesh2d` for a `Camera2d`, overriding the plugin default. Together with `.on_layer()`, they build a HUD from ordinary nodes over a 3D scene: give the HUD nodes `.flat().on_layer(HUD)` and add a `Camera2d` with that layer and a higher `order` next to the scene camera. Each camera only draws the nodes on its own layers, and a press from it only picks those nodes, so the HUD and the scene never take presses from each other.

## Persistent ECS Components with `with_scene`

A node can carry persistent ECS components that survive state changes. `with_scene` takes a closure that returns a Bevy `Scene`, applied once when the node first appears. The node keeps its entity for as long as it is in the view, so the components and observers survive every change of state.

```rust
use bevy::prelude::*;

ViewNode::sphere("interactive", 1.0)
    .with_scene(|| (
        Name::new("My Node"),
        SomeCustomComponent,
    ))
```

The scene is applied only for nodes that are actually shown, never during speculative preview evaluation. The engine owns the transform, mesh, material, render layers, picking, and visibility -- the scene cannot override those. The scene must not describe children, because the engine owns the node hierarchy.

## Layers

Nodes live on render layers for picking and rendering isolation:

```rust
ViewNode::sphere("hud", 1.0).on_layer(2)
```

Gizmo handles use layer 31 (`GIZMO_LAYER`) so they are always picked first.

## Next Steps

- Learn how to make nodes draggable with [Operators](./operators.md)
- Control where drags happen with [Drag Spaces](./spaces.md)
