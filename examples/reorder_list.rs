//! A reorderable list: the dragged item floats under the pointer while the others make room.
//!
//! The view function only places the items of an order. `closest` over "this item moved to
//! every slot" previews the nearest drop position, and `with_floating` keeps the dragged item
//! free of the slots.

use bevy::prelude::*;
use bevy_composable_dragging::ops::closest_states;
use bevy_composable_dragging::{DraggablePlugin, View, ViewNode};

mod common;
use common::move_item;
use common::screenshot::AutoScreenshotPlugin;
use common::script::ScriptedDragPlugin;

const ITEMS: usize = 5;
const SLOT_HEIGHT: f32 = 1.0;

/// The items of the list from top to bottom.
#[derive(Clone)]
struct Order(Vec<usize>);

fn slot_position(slot: usize) -> Vec2 {
    let top = (ITEMS - 1) as f32 / 2.0 * SLOT_HEIGHT;
    Vec2::new(0.0, top - slot as f32 * SLOT_HEIGHT)
}

fn view(order: &Order) -> View<Order> {
    let mut view = View::new();
    for (slot, &item) in order.0.iter().enumerate() {
        let order = order.0.clone();
        let hue = item as f32 * 360.0 / ITEMS as f32;
        view.push(
            ViewNode::rectangle(format!("item-{item}"), Vec2::new(3.0, 0.8))
                .at_xy(slot_position(slot))
                .with_color(Color::hsl(hue, 0.65, 0.55))
                .draggable(move || {
                    closest_states((0..ITEMS).map(|to| Order(move_item(&order, slot, to))))
                        .with_floating()
                }),
        );
    }
    view
}


/// Run this example. Called by the WASM runner.
pub fn run(app: &mut App) {
    app
        .add_plugins(common::default_plugins("reorder_list"))
        .add_plugins(DraggablePlugin::new(Order((0..ITEMS).collect()), view))
        .add_plugins(ScriptedDragPlugin::straight(
            slot_position(0),
            slot_position(3) + Vec2::new(0.6, 0.1),
        ))
        .add_plugins(
            AutoScreenshotPlugin::new("screenshot_reorder_list.webp")
                .with_title("reorder_list")
                .with_description("closest + floating")
                .animation(600),
        )
        .add_systems(Startup, |mut commands: Commands| {
            common::spawn_cameras(&mut commands, 7.0);
        })
        ;
}

fn main() {
    App::new()
        .add_plugins(common::default_plugins("reorder_list"))
        .add_plugins(DraggablePlugin::new(Order((0..ITEMS).collect()), view))
        .add_plugins(ScriptedDragPlugin::straight(
            slot_position(0),
            slot_position(3) + Vec2::new(0.6, 0.1),
        ))
        .add_plugins(
            AutoScreenshotPlugin::new("screenshot_reorder_list.webp")
                .with_title("reorder_list")
                .with_description("closest + floating")
                .animation(600),
        )
        .add_systems(Startup, |mut commands: Commands| {
            common::spawn_cameras(&mut commands, 7.0);
        })
        .run();
}
