# Drag Spaces

A drag space resolves the pointer ray to a world point. Operators then work on world positions and node transforms, so drags on the ground, along an axis, and on a transform gizmo share one code path.

## Types of Drag Spaces

### Plane

The default space: a plane with any origin and normal.

```rust
use bevy_composable_dragging::DragSpace;
use bevy::math::{Dir3, Vec3};

// XY plane at z = 0 (default)
DragSpace::world_plane(0.0)

// Ground plane at y = 0
DragSpace::plane(Vec3::ZERO, Dir3::Y)
```

### Axis

Drag along a line. The pointer position is the nearest point on the line to the pointer ray.

```rust
// Drag along the Y axis
DragSpace::axis(Vec3::ZERO, Dir3::Y)
```

### Screen

A plane through the dragged node that faces the camera, fixed when the drag starts.

```rust
DragSpace::screen()
```

## Setting a Drag Space

```rust
use bevy_composable_dragging::{DragSpace, ViewNode};
use bevy::math::{Dir3, Vec3};

// Default: drags in the XY plane
ViewNode::sphere("default", 1.0)
    .draggable(|| /* spec */)

// Custom: drag on the ground
ViewNode::sphere("ground", 1.0)
    .draggable_in(
        DragSpace::plane(Vec3::ZERO, Dir3::Y),
        || /* spec */,
    )
```

## How Spaces Work

When you press a node, the engine resolves the pointer ray against the node's drag space:

1. **Plane**: intersect the ray with the plane
2. **Axis**: find the nearest point on the axis line to the ray
3. **Screen**: create a plane through the node facing the camera, then intersect

Operators like `closest`, `between`, and `with_floating` work unchanged in any space. Gaps are measured after projecting positions onto the space, and 2D weights use the local basis of the space.

## Picking

A press picks the node whose oriented bounds the pointer ray hits first (`View::hit_test`), then grabs its nearest draggable ancestor. Nodes on higher render layers are picked first — gizmo handles use layer 31 so they are always picked above scene geometry.

## Next Steps

- Smooth transitions with [Blending](./blending.md)
- Transform gizmos with [Gizmos](./gizmos.md)
