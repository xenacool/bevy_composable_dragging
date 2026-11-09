# Blending

The engine blends views to produce smooth animations. Every animatable property of a `ViewNode` implements the `Blend` trait, and `lerp_views` interpolates between two views by node id.

## How Blending Works

When a drag starts, the engine evaluates the view function for the current state and each target state. As the pointer moves, it blends between the start view and the target view, producing a smooth preview. On release, the state switches and the view animates to the new target.

```rust
use bevy_composable_dragging::lerp_views;

let preview = lerp_views(&start_view, &target_view, t);
```

## Blended Properties

| Property | Interpolation |
|----------|---------------|
| Translation | Linear interpolation |
| Rotation | Spherical interpolation (shortest arc) |
| Scale | Linear interpolation |
| Color | Oklab color space (perceptually uniform) |
| Geometry | Morph between compatible shapes |
| Opacity | Linear interpolation |
| Style (gradmesh) | Gradient interpolation |

## Presence

When a node exists in one view but not the other, its `Presence` determines how it appears or disappears:

```rust
use bevy_composable_dragging::Presence;

// Fade in/out with opacity
Presence::Fade

// Emerge from the dragged node's position
Presence::Emerge

// Appear instantly
Presence::Pop
```

## View Blending Functions

```rust
use bevy_composable_dragging::{lerp_views, blend_views, lerp_transform};

// Linear interpolation between two views
let blended = lerp_views(&a, &b, 0.5);

// Natural neighbor blending (for multiple views)
let blended = blend_views(&[&a, &b, &c], &[0.5, 0.3, 0.2]);

// Interpolate a single transform
let t = lerp_transform(&t1, &t2, 0.5);
```

## Overshoot

`ViewNode::with_overshoot` makes a node's transform run past its target by a fraction of the distance and settle back, creating a spring-like animation:

```rust
ViewNode::sphere("bouncy", 1.0)
    .with_overshoot(0.12)  // 12% overshoot
```

The HUD uses 12% overshoot for all its elements.

## Geometry Morphing

When two views use different geometries for the same node, the engine morphs between them. With `bevy_gradmesh`, shapes are quantized to 32 morph steps, bounding the number of distinct meshes.

![Blend square](screenshots/screenshot_blend_square.webp)

*Natural neighbor blending across four control points. Run `cargo run --example blend_square --features pbr`.*

## Next Steps

- Transform gizmos with [Gizmos](./gizmos.md)
- Edit curves with [Curve Handles](./handles.md)
