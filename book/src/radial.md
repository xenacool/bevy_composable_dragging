# Radial Menus

The `radial` module (behind the `gradmesh` feature) builds a radial menu on the HUD: a hub, rings of petals, and a status panel. Every petal is an ordinary node that blooms out of the hub when its ring opens.

## Traits

Three traits connect your types to the menu:

### `Petal`

Defines what a menu item looks like and does:

```rust
use bevy_composable_dragging::radial::Petal;
use bevy_gradmesh::Shape;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Tool { Pen, Eraser }

impl Petal<MyState> for Tool {
    fn name(&self) -> String {
        format!("{self:?}")
    }

    fn label(&self, _state: &MyState) -> String {
        self.name()
    }

    fn accent(&self) -> Color {
        Color::srgb(0.24, 0.42, 0.8)
    }

    fn glyph(&self) -> Shape {
        // Return a small shape for the petal icon
        Shape::circle(0.5)
    }

    fn is_active(&self, state: &MyState) -> bool {
        state.tool == *self
    }

    fn press(&self, state: &MyState) -> MyState {
        let mut s = state.clone();
        s.tool = *self;
        s.menu = None;
        s
    }
}
```

### `Ring`

Defines a ring of items:

```rust
use bevy_composable_dragging::radial::Ring;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Page { Tools }

impl Ring<MyState> for Page {
    type Item = Tool;

    fn name(&self) -> String {
        "tools".into()
    }

    fn items(&self, _state: &MyState) -> Vec<Tool> {
        vec![Tool::Pen, Tool::Eraser]
    }

    fn all(_state: &MyState) -> Vec<Self> {
        vec![Page::Tools]
    }
}
```

### `RadialHost`

Connects the state to the menu. It extends `HudHost`:

```rust
use bevy_composable_dragging::radial::RadialHost;
use bevy_composable_dragging::hud::Layout;

impl RadialHost for MyState {
    type Ring = Page;

    fn root(&self) -> Page {
        Page::Tools
    }

    fn menu(&self) -> Option<Page> {
        self.menu.clone()
    }

    fn with_menu(&self, menu: Option<Page>) -> Self {
        Self { menu, ..self.clone() }
    }

    fn pop_center(&self, layout: &Layout) -> Vec2 {
        layout.home()
    }

    fn status(&self) -> String {
        format!("Tool: {:?}", self.tool)
    }
}
```

## Adding the Menu

`push_menu` builds the menu nodes; call it from your view function with the layout and theme:

```rust
use bevy_composable_dragging::hud::Layout;
use bevy_composable_dragging::radial::{push_menu, RadialMenuPlugin};

fn view(state: &MyState) -> View<MyState> {
    let mut view = View::new();
    // ... scene nodes ...
    let layout = Layout::new(state.screen());
    let theme = /* your theme */;
    push_menu(&mut view, &state_arc, &layout, &theme);
    view
}

// Register the plugin for text labels
App::add_plugins(RadialMenuPlugin::<MyState>::new(theme));
```

## Glyphs

The `glyph` submodule provides common icon shapes:

```rust
use bevy_composable_dragging::radial::glyph;

// Dots pattern
glyph::dots(5.0, &[(0.0, 0.0)])

// Arrow
glyph::arrow(5.0, Dir2::ZERO)
```

## Hub Behavior

The hub rests as a small button in a corner, or pops up over the element the menu edits. When a tool edits an element in place, the hub shrinks to a chip above it.

![Cozy Castle Menu](screenshots/screenshot_cozy_castle_menu.webp)

*Radial menu with tools ring. Run `cargo run --example cozy_castle_3d --features shapes-3d`.*

## Next Steps

- Advanced patterns in [Advanced Topics](./advanced.md)
