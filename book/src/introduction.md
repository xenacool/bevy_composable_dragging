# Introduction

Bevy Composable Dragging is a declarative dragging library for [Bevy](https://bevyengine.org/) 0.20, ported from [Dragology](https://declarative-dragging.github.io).

## What It Solves

Instead of writing hand-crafted drop zones, hit tests, and drag state machines, you describe your interface as a pure function from state to a [`View`](https://docs.rs/bevy_composable_dragging/latest/bevy_composable_dragging/struct.View.html), and attach a *drag spec* to every node that can be dragged. The engine renders the target states speculatively, finds the dragged node by id in each of them, and synthesizes the preview while dragging and the new state on release.

*A single draggable circle toggling between two states. Run `cargo run --example toggle --features pbr` to reproduce it.*

## Key Concepts

- **Views**: Plain-data descriptions of scenes -- flat lists of nodes with geometry, transforms, colors, and optional drag specs
- **Operators**: Declarative drag behaviors (`between`, `closest`, `vary`, etc.) that compile into per-frame behavior
- **Drag Spaces**: Planes, axes, and screen-facing planes that resolve pointer rays to world points
- **Blending**: Smooth interpolation between views, with presence animations for emerging and disappearing nodes
- **Gizmos**: Transform handles (translate, rotate, scale) built from ordinary draggable nodes
- **Editor Tools**: Heightmaps, curve handles, HUDs, and radial menus for building complete editors

## Design Philosophy

Bevy Composable Dragging follows these principles:

1. **Declarative**: Describe what a drag does, not how it works frame by frame
2. **Composable**: Specs are plain data that combine freely -- no state machines or enums
3. **Pure views**: The view function is pure; state holds everything, including HUD screen size
4. **Bevy-native**: Outputs standard `Mesh`, `Transform`, and `StandardMaterial` types
5. **Feature-gated**: The core library needs only `bevy_color`; PBR and gradient features are optional

## Compatibility

| bevy_composable_dragging | Bevy | Rust |
|--------------------------|------|------|
| 0.3.x                    | 0.20 | 1.87+|
