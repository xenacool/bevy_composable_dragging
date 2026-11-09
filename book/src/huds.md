# HUDs

The `hud` module (behind the `gradmesh` feature) provides a heads-up display built from ordinary nodes. Elements are rounded signed distance fields — fills and borders drawn per pixel so edges stay crisp at any size.

## Screen and Layout

The `Screen` holds the HUD size and the projection of the scene camera. The `Layout` computes where corner anchors and radial menu petals go from the screen size.

```rust
use bevy_composable_dragging::hud::{HudHost, HudPlugin, HudProjectionCamera, Screen};

#[derive(Clone)]
struct Editor {
    screen: Screen,
}

impl HudHost for Editor {
    fn screen(&self) -> &Screen {
        &self.screen
    }

    fn with_screen(&self, screen: Screen) -> Self {
        Self { screen }
    }
}
```

Register `HudPlugin` to keep the screen in sync with the window:

```rust
App::new()
    .add_plugins(DefaultPlugins)
    .add_plugins(DraggablePlugin::new(Editor { screen: Screen::default() }, view))
    .add_plugins(HudPlugin::<Editor>::new(720.0))
    .add_systems(Startup, |mut commands: Commands| {
        commands.spawn((Camera3d::default(), HudProjectionCamera));
    })
    .run();
```

## Theme

A `Theme` holds colors, border width, tween settings, and the render layer. The HUD builders are methods of the theme:

```rust
use bevy_composable_dragging::hud::{Layout, Theme};

let theme = Theme::default();
let layout = Layout::new(state.screen());

// Build a bordered panel (returns 2 nodes: plate + border)
let panel = theme.panel("panel", Vec2::new(200.0, 100.0), Vec2::ZERO, &layout);

// Build a filled disc
let disc = theme.disc("action", 20.0, theme.fill, Vec3::ZERO);
```

## Depth Layers

HUD elements are layered by depth constants:

| Layer | Depth | Purpose |
|-------|-------|---------|
| Panel | 0.0 | Background panels |
| Petal | 1.0 | Menu petals |
| Hub | 2.0 | Menu hubs |
| Border | 0.05 | Element borders |
| Glyph | 0.1 | Icons and glyphs |
| Mark | 0.2 | Selection marks |

## SDF Shapes

HUD elements use `SdfShape` for crisp edges:

```rust
use bevy_composable_dragging::SdfShape;

ViewNode::sdf("panel", SdfShape::rect(Vec2::new(200.0, 100.0)).with_corner_radius(8.0))
```

Available SDF shapes:
- `rect(size).with_corner_radius(r)` — rounded rectangle
- `ellipse(size)` — ellipse

## Overshoot

All HUD elements use 12% overshoot for spring-like animations when the view changes.

## Render Layer

The default HUD layer is 2, leaving layer 0 for the scene and layer 1 for overlays. Gizmo handles use layer 31.

## Next Steps

- Add menus with [Radial Menus](./radial.md)
- Advanced patterns in [Advanced Topics](./advanced.md)
