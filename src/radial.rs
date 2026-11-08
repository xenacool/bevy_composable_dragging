//! A radial menu on a [`hud`]: a hub, rings of petals and a status panel.
//!
//! The menu is drawn by a `Camera2d` on the HUD layer from ordinary engine nodes, laid out by
//! the [`Layout`]: bordered signed distance discs with a small glyph each, and a text label on a
//! pill under each, which a shape cannot draw, so the labels are `Text2d` entities that follow
//! their petals ([`RadialMenuPlugin`]). Every petal of every ring is in every view; the petals
//! of rings that are not open are shrunk to nothing at the hub, so opening a ring blooms its
//! petals out of the hub when the engine blends the two views, and the overshoot of the nodes
//! makes it pop.
//!
//! The hub rests as a small button in a corner, or pops up over the element the menu edits and
//! shrinks to a chip above it when a tool edits that element in place, so it never covers what
//! it edits. The menu is a mode of the state: pressing the hub or a petal and releasing it
//! commits a fixed state, in which a ring opens, a choice is made, or an item is armed.
//!
//! Nothing in the menu knows what its items do. A third party plugs in by implementing three
//! traits for its own types:
//!
//! - [`Petal`] on the item type: what a petal is called, looks like and does when pressed;
//! - [`Ring`] on the type that names a ring: which items it holds, and which rings there are;
//! - [`RadialHost`] on the state: which ring is open, where the menu pops up and what the status
//!   panel says.
//!
//! [`push_menu`] then adds the whole menu to a view.
//!
//! ```no_run
//! use std::sync::Arc;
//!
//! use bevy::prelude::*;
//! use bevy_composable_dragging::hud::{HudHost, Layout, Screen, Theme};
//! use bevy_composable_dragging::radial::{
//!     Petal, RadialHost, RadialMenuPlugin, Ring, glyph, push_menu,
//! };
//! use bevy_composable_dragging::{DraggablePlugin, View};
//! use bevy_gradmesh::Shape;
//!
//! #[derive(Clone, Copy, PartialEq, Debug)]
//! enum Tool {
//!     Pen,
//!     Eraser,
//! }
//!
//! #[derive(Clone, Copy, PartialEq, Debug)]
//! enum Page {
//!     Tools,
//! }
//!
//! #[derive(Clone)]
//! struct Paint {
//!     screen: Screen,
//!     menu: Option<Page>,
//!     tool: Tool,
//! }
//!
//! impl Petal<Paint> for Tool {
//!     fn name(&self) -> String {
//!         format!("{self:?}")
//!     }
//!
//!     fn label(&self, _paint: &Paint) -> String {
//!         format!("{self:?}")
//!     }
//!
//!     fn accent(&self) -> Color {
//!         Color::srgb(0.24, 0.42, 0.8)
//!     }
//!
//!     fn glyph(&self) -> Shape {
//!         glyph::dots(5.0, &[(0.0, 0.0)])
//!     }
//!
//!     fn is_active(&self, paint: &Paint) -> bool {
//!         paint.tool == *self
//!     }
//!
//!     fn press(&self, paint: &Paint) -> Paint {
//!         Paint { tool: *self, menu: None, ..paint.clone() }
//!     }
//! }
//!
//! impl Ring<Paint> for Page {
//!     type Item = Tool;
//!
//!     fn name(&self) -> String {
//!         "tools".into()
//!     }
//!
//!     fn items(&self, _paint: &Paint) -> Vec<Tool> {
//!         vec![Tool::Pen, Tool::Eraser]
//!     }
//!
//!     fn all(_paint: &Paint) -> Vec<Self> {
//!         vec![Page::Tools]
//!     }
//! }
//!
//! impl HudHost for Paint {
//!     fn screen(&self) -> &Screen {
//!         &self.screen
//!     }
//!
//!     fn with_screen(&self, screen: Screen) -> Self {
//!         Self { screen, ..self.clone() }
//!     }
//! }
//!
//! impl RadialHost for Paint {
//!     type Ring = Page;
//!
//!     fn root(&self) -> Page {
//!         Page::Tools
//!     }
//!
//!     fn menu(&self) -> Option<Page> {
//!         self.menu
//!     }
//!
//!     fn with_menu(&self, menu: Option<Page>) -> Self {
//!         Self { menu, ..self.clone() }
//!     }
//!
//!     fn pop_center(&self, layout: &Layout) -> Vec2 {
//!         layout.home()
//!     }
//!
//!     fn status(&self) -> String {
//!         format!("Paint\n{:?}", self.tool)
//!     }
//! }
//!
//! fn view(paint: &Paint) -> View<Paint> {
//!     let mut view = View::new();
//!     let layout = Layout::new(paint.screen());
//!     push_menu(&mut view, &Arc::new(paint.clone()), &layout, &Theme::default());
//!     view
//! }
//!
//! let paint = Paint { screen: Screen::default(), menu: None, tool: Tool::Pen };
//! App::new()
//!     .add_plugins(DefaultPlugins)
//!     .add_plugins(DraggablePlugin::new(paint, view))
//!     .add_plugins(RadialMenuPlugin::<Paint>::new(Theme::default()))
//!     .run();
//! ```

use std::collections::HashSet;
use std::marker::PhantomData;
use std::sync::Arc;

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;
use bevy::sprite::{Anchor, Text2dShadow};
use bevy_gradmesh::{Shape, Style};

use crate::hud::{self, Footprint, HudHost, Layout, Theme};
use crate::ops::{button, state};
use crate::{DragModel, DragSpace, Geometry, View, ViewNode};

/// The scale of a petal that is not shown, which is as close to nothing as a scale can be.
const HIDDEN: f32 = 0.001;
/// The size of the text of a label, in HUD units.
const LABEL_SIZE: f32 = 14.0;
/// How far below its petal a label is, at full size.
const LABEL_GAP: f32 = 12.0;
/// The depth of the labels, in front of the petals.
const LABEL_DEPTH: f32 = 3.0;
/// The height of the pill behind a label, and the room it leaves around the text, at full size.
const PILL_HEIGHT: f32 = 20.0;
const PILL_PADDING: f32 = 14.0;
/// The scale of the hub once it has shrunk to a chip.
pub const CHIP_SCALE: f32 = 0.6;
/// How far above the hub the petal of an armed item hangs, at full size.
const ARMED_LIFT: f32 = 84.0;
/// The room around the text of the status panel, and the height of a line, at full size.
const STATUS_PADDING: f32 = 12.0;
const STATUS_LINE: f32 = 19.0;
/// How strongly the glyph of a petal shades towards its bottom.
const GLYPH_SHADE: f32 = 0.8;

/// A petal: one item of a ring.
///
/// Items are small values. A petal that opens another ring and a petal that makes a choice are
/// the same trait: [`press`](Self::press) gets the state and returns the state after the press.
pub trait Petal<S>: Clone + Send + Sync + 'static {
    /// Names the petal within its ring, so its nodes keep their identity from one view to the
    /// next. Two items of a ring must not share a name.
    fn name(&self) -> String;

    /// The text under the petal in `state`.
    fn label(&self, state: &S) -> String;

    /// The color of the petal, dark enough for its glyph to read on it.
    fn accent(&self) -> Color;

    /// A small shape in the plane of the HUD, about twenty units across, that says what the
    /// petal is. See [`glyph`] for helpers to draw one.
    fn glyph(&self) -> Shape;

    /// Whether the petal is the current choice, which marks it. Petals that open rings and
    /// petals that act at once are never the current choice.
    fn is_active(&self, _state: &S) -> bool {
        false
    }

    /// The state after pressing the petal: a ring opens, or the choice is made and the menu
    /// closes.
    #[must_use]
    fn press(&self, state: &S) -> S;
}

/// A ring of petals around the hub: the metadata the menu needs to lay it out.
pub trait Ring<S>: Clone + PartialEq + Send + Sync + 'static {
    /// The items the ring holds.
    type Item: Petal<S>;

    /// Names the ring, so its nodes keep their identity from one view to the next. Two rings
    /// must not share a name.
    fn name(&self) -> String;

    /// The petals of the ring in `state`, in the order they fan out. The items can depend on the
    /// state, like the tools that make sense for what is selected.
    fn items(&self, state: &S) -> Vec<Self::Item>;

    /// Every ring there is in `state`. They are all in every view, so that opening one blooms
    /// its petals out of the hub when the views blend.
    fn all(state: &S) -> Vec<Self>;
}

/// The item type of the rings of the state `S`.
pub type ItemOf<S> = <<S as RadialHost>::Ring as Ring<S>>::Item;

/// The state of an interface that has a radial menu. It tells the menu which ring is open, where
/// to pop up, and what to say; the items tell it what they do.
pub trait RadialHost: HudHost {
    /// The rings of the menu.
    type Ring: Ring<Self>;

    /// The ring the hub opens first, and the one it goes back to from the other rings.
    fn root(&self) -> Self::Ring;

    /// The ring that is open, if any.
    fn menu(&self) -> Option<Self::Ring>;

    /// This state with the ring `menu` open, or with the menu closed.
    #[must_use]
    fn with_menu(&self, menu: Option<Self::Ring>) -> Self;

    /// Where on the HUD the rings are centered: over the element the menu edits, which the hub
    /// pops up over, or else at [`Layout::home`], the corner the hub rests in.
    fn pop_center(&self, layout: &Layout) -> Vec2;

    /// Whether the hub is popped up over [`pop_center`](Self::pop_center) instead of resting in
    /// its corner.
    fn is_popped(&self) -> bool {
        false
    }

    /// Whether the popped hub has shrunk to a chip, which a tool that edits the element in
    /// place wants so the hub does not cover what it edits.
    fn is_chip(&self) -> bool {
        false
    }

    /// The item that is armed for the next drag, whose petal then hangs above the hub. Pressing
    /// it calls [`disarmed`](Self::disarmed).
    fn armed(&self) -> Option<ItemOf<Self>> {
        None
    }

    /// This state with nothing armed.
    #[must_use]
    fn disarmed(&self) -> Self {
        self.clone()
    }

    /// What the status panel says, one line of text per line of the panel.
    fn status(&self) -> String;

    /// This state with the hub pressed: it opens the root ring, goes back to it from another
    /// ring, and closes it from there.
    #[must_use]
    fn hub_pressed(&self) -> Self {
        let root = self.root();
        if self.menu().as_ref() == Some(&root) {
            self.with_menu(None)
        } else {
            self.with_menu(Some(root))
        }
    }
}

/// Helpers to draw the glyphs of petals: shapes in the plane of the HUD, with `y` up.
pub mod glyph {
    use bevy::prelude::*;
    use bevy_gradmesh::{Shape, ShapeDetail, ShapePart};

    /// The points with a counterclockwise winding.
    fn counterclockwise(mut points: Vec<Vec2>) -> Vec<Vec2> {
        let area: f32 = points
            .iter()
            .zip(points.iter().cycle().skip(1))
            .map(|(a, b)| a.perp_dot(*b))
            .sum();
        if area < 0.0 {
            points.reverse();
        }
        points
    }

    /// A polygon from points in the plane of the HUD, in either winding.
    #[must_use]
    pub fn outline(points: &[(f32, f32)]) -> Shape {
        let points = points.iter().map(|&(x, y)| Vec2::new(x, y)).collect();
        Shape::arbitrary_polygon(counterclockwise(points), ShapeDetail::Low)
    }

    /// A rectangle from its corners, as a part of a compound shape.
    #[must_use]
    pub fn rectangle(left: f32, bottom: f32, right: f32, top: f32) -> ShapePart {
        ShapePart::new(outline(&[
            (left, bottom),
            (right, bottom),
            (right, top),
            (left, top),
        ]))
    }

    /// Small discs of `radius` at `centers`.
    #[must_use]
    pub fn dots(radius: f32, centers: &[(f32, f32)]) -> Shape {
        Shape::compound(
            centers
                .iter()
                .map(|&(x, y)| {
                    ShapePart::new(Shape::disc(radius, ShapeDetail::Low))
                        .with_transform(Transform::from_xyz(x, y, 0.0))
                })
                .collect::<Vec<_>>(),
        )
    }

    /// The glyph of the hub: three bars, which says that it opens a menu.
    #[must_use]
    pub fn hub() -> Shape {
        Shape::compound(vec![
            rectangle(-10.0, 5.0, 10.0, 8.0),
            rectangle(-10.0, -1.5, 10.0, 1.5),
            rectangle(-10.0, -8.0, 10.0, -5.0),
        ])
    }
}

/// The id of the node of the petal of `item` in `ring`: `menu/<ring>/<item>`.
pub fn petal_id<S: RadialHost>(ring: &S::Ring, item: &ItemOf<S>) -> String {
    format!("menu/{}/{}", ring.name(), item.name())
}

/// Adds a hub to `view`: a button that opens the first ring, or closes it. It rests in its
/// corner while nothing is selected and pops up over the selection otherwise, so there is one
/// hub of each kind in every view and `scale` shows one and hides the other. A hidden one takes
/// no presses and is not hit by the pointer.
fn push_hub<S: RadialHost>(
    view: &mut View<S>,
    snapshot: &Arc<S>,
    layout: &Layout,
    theme: &Theme,
    id: &str,
    (at, scale): (Vec2, f32),
) {
    let radius = layout.hub_radius();
    let pressed = Arc::clone(snapshot);
    let disc = theme
        .disc(id, radius, theme.hub, at.extend(hud::HUB_DEPTH))
        .scaled(Vec3::splat(scale));
    view.push(if scale > HIDDEN {
        disc.draggable_in(DragSpace::world_plane(0.0), move || {
            button(pressed.hub_pressed(), radius)
        })
    } else {
        disc.pickable(false)
    });
    view.push(
        theme
            .disc_border(
                format!("{id}/border"),
                radius,
                theme.border_width(layout) * 1.2,
                hud::edge(theme.hub),
                Vec3::new(0.0, 0.0, hud::BORDER_DEPTH),
            )
            .with_parent(id.to_owned())
            .pickable(false),
    );
    view.push(
        theme
            .on_hud(ViewNode::grad(format!("{id}/glyph"), glyph::hub()))
            .with_parent(id.to_owned())
            .at(Vec3::new(0.0, 0.0, hud::GLYPH_DEPTH))
            .scaled(Vec3::splat(layout.unit()))
            .with_style(Style::shaded(theme.ink, GLYPH_SHADE))
            .pickable(false),
    );
}

/// The size of the pill behind `label`.
fn pill_size(label: &str, layout: &Layout) -> Vec2 {
    Vec2::new(
        hud::text_width(label, LABEL_SIZE) + PILL_PADDING,
        PILL_HEIGHT,
    ) * layout.unit()
}

/// Adds a petal of `ring` to `view` at `at`, drawn at `scale`: a disc with a lighter border, its
/// glyph, a pill for its label, and a mark when it is the current choice. The disc is what
/// presses, unless the petal is hidden, and then it is not hit at all.
fn push_petal<S: RadialHost>(
    view: &mut View<S>,
    snapshot: &Arc<S>,
    layout: &Layout,
    theme: &Theme,
    (ring, item): (&S::Ring, &ItemOf<S>),
    (at, scale): (Vec2, f32),
) {
    let id = petal_id::<S>(ring, item);
    let radius = layout.petal_radius();
    let unit = layout.unit();
    let accent = item.accent();
    let pressed = Arc::clone(snapshot);
    let chosen = item.clone();
    let disc = theme
        .disc(id.clone(), radius, accent, at.extend(hud::PETAL_DEPTH))
        .scaled(Vec3::splat(scale));
    view.push(if scale > HIDDEN {
        disc.draggable_in(DragSpace::world_plane(0.0), move || {
            button(chosen.press(&pressed), radius)
        })
    } else {
        disc.pickable(false)
    });
    view.push(
        theme
            .disc_border(
                format!("{id}/border"),
                radius,
                theme.border_width(layout),
                hud::edge(accent),
                Vec3::new(0.0, 0.0, hud::BORDER_DEPTH),
            )
            .with_parent(id.clone())
            .pickable(false),
    );
    view.push(
        theme
            .on_hud(ViewNode::grad(format!("{id}/glyph"), item.glyph()))
            .with_parent(id.clone())
            .at(Vec3::new(0.0, 0.0, hud::GLYPH_DEPTH))
            .scaled(Vec3::splat(unit))
            .with_style(Style::shaded(theme.glyph, GLYPH_SHADE))
            .pickable(false),
    );
    let pill = pill_size(&item.label(snapshot), layout);
    let below = Vec3::new(0.0, -(radius + LABEL_GAP * unit), hud::GLYPH_DEPTH);
    view.push(
        theme
            .plate(format!("{id}/pill"), pill, pill.y / 2.0, theme.fill, below)
            .with_parent(id.clone())
            .pickable(false),
    );
    if item.is_active(snapshot) {
        view.push(
            theme
                .disc_border(
                    format!("{id}/active"),
                    radius + 5.0 * unit,
                    theme.border_width(layout) * 1.3,
                    theme.mark,
                    Vec3::new(0.0, 0.0, hud::MARK_DEPTH),
                )
                .with_parent(id)
                .pickable(false),
        );
    }
}

/// The size and the center of the status panel, which hangs from the top left corner.
fn status_panel<S: RadialHost>(snapshot: &S, layout: &Layout) -> (Vec2, Vec2) {
    let text = snapshot.status();
    let unit = layout.unit();
    let padding = STATUS_PADDING * unit;
    let lines = text.lines().count() as f32;
    let size = Vec2::new(
        hud::text_width(&text, LABEL_SIZE) * unit + padding * 2.0,
        lines * STATUS_LINE * unit + padding * 2.0,
    );
    (size, layout.top_left() + Vec2::new(size.x, -size.y) / 2.0)
}

/// Adds the status panel to `view`. It takes presses, so none fall through it to the scene.
fn push_status<S: RadialHost>(
    view: &mut View<S>,
    snapshot: &Arc<S>,
    layout: &Layout,
    theme: &Theme,
) {
    let (size, center) = status_panel(snapshot.as_ref(), layout);
    let [plate, border] = theme.panel("menu/status", size, center, layout);
    let held = Arc::clone(snapshot);
    view.push(plate.draggable_in(DragSpace::world_plane(0.0), move || state(S::clone(&held))));
    view.push(border.pickable(false));
}

/// Adds the petal of the armed item above the hub at `center`. Pressing it disarms the item.
fn push_armed<S: RadialHost>(
    view: &mut View<S>,
    snapshot: &Arc<S>,
    layout: &Layout,
    theme: &Theme,
    center: Vec2,
) {
    let Some(item) = snapshot.armed() else {
        return;
    };
    let radius = layout.petal_radius();
    let accent = item.accent();
    let at = layout.keep_inside(center + Vec2::Y * ARMED_LIFT * layout.unit());
    let disarm = Arc::clone(snapshot);
    view.push(
        theme
            .disc("menu/armed", radius, accent, at.extend(hud::PETAL_DEPTH))
            .draggable_in(DragSpace::world_plane(0.0), move || {
                button(disarm.disarmed(), radius)
            }),
    );
    view.push(
        theme
            .disc_border(
                "menu/armed/border",
                radius,
                theme.border_width(layout),
                hud::edge(accent),
                Vec3::new(0.0, 0.0, hud::BORDER_DEPTH),
            )
            .with_parent("menu/armed")
            .pickable(false),
    );
    view.push(
        theme
            .on_hud(ViewNode::grad("menu/armed/glyph", item.glyph()))
            .with_parent("menu/armed")
            .at(Vec3::new(0.0, 0.0, hud::GLYPH_DEPTH))
            .scaled(Vec3::splat(layout.unit()))
            .with_style(Style::shaded(theme.glyph, GLYPH_SHADE))
            .pickable(false),
    );
}

/// Adds the menu to `view`: a bordered status panel, the hub that rests in its corner and the
/// one that pops up over the selection, the petal of the armed item, and every petal of every
/// ring, fanned out around the hub that is in use when its ring is open and shrunk into it
/// otherwise.
///
/// The ids of its nodes start with `menu/`: `menu/status`, `menu/hub`, `menu/pop`, `menu/armed`
/// and `menu/<ring>/<item>` (see [`petal_id`]), each with its parts as children.
pub fn push_menu<S: RadialHost>(
    view: &mut View<S>,
    snapshot: &Arc<S>,
    layout: &Layout,
    theme: &Theme,
) {
    let center = snapshot.pop_center(layout);
    let popped = snapshot.is_popped();
    push_status(view, snapshot, layout, theme);
    let resting = if popped { HIDDEN } else { 1.0 };
    push_hub(
        view,
        snapshot,
        layout,
        theme,
        "menu/hub",
        (layout.home(), resting),
    );
    let shown = match (popped, snapshot.is_chip()) {
        (false, _) => HIDDEN,
        (true, true) => CHIP_SCALE,
        (true, false) => 1.0,
    };
    push_hub(view, snapshot, layout, theme, "menu/pop", (center, shown));
    push_armed(view, snapshot, layout, theme, center);
    let open = snapshot.menu();
    for ring in S::Ring::all(snapshot) {
        let items = ring.items(snapshot);
        let slots = (open.as_ref() == Some(&ring)).then(|| {
            let footprints: Vec<Footprint> = items
                .iter()
                .map(|item| {
                    let pill = pill_size(&item.label(snapshot), layout);
                    Footprint::labelled(layout, pill, LABEL_GAP * layout.unit())
                })
                .collect();
            layout.fan_footprints(center, &footprints).centers
        });
        for (index, item) in items.iter().enumerate() {
            let place = slots
                .as_ref()
                .map_or((center, HIDDEN), |slots| (slots[index], 1.0));
            push_petal(view, snapshot, layout, theme, (&ring, item), place);
        }
    }
}

/// A text on the HUD.
#[derive(Clone, Debug, PartialEq)]
pub struct Label {
    /// Names the label, so its entity is kept from one frame to the next.
    pub key: String,
    /// The text.
    pub text: String,
    /// Where the text is, on the HUD, with its depth.
    pub at: Vec3,
    /// How large the text is drawn. A label of a petal that is not shown is not drawn.
    pub scale: f32,
    /// Whether the text starts at `at` instead of being centered on it.
    pub left: bool,
}

impl Label {
    /// Whether the label is big enough to be drawn.
    #[must_use]
    pub fn is_shown(&self) -> bool {
        self.scale > 0.05
    }
}

/// The texts of the menu in `view`, each under its petal where the petal is drawn, so a label
/// blooms with its petal. The key of the label of a petal is its [`petal_id`], and the key of the
/// status line is `status`.
pub fn labels<S: RadialHost>(snapshot: &S, view: &View<S>) -> Vec<Label> {
    let layout = Layout::new(snapshot.screen());
    let unit = layout.unit();
    let mut labels = Vec::new();
    if let Some(panel) = view.get("menu/status") {
        let width = match &panel.geometry {
            Geometry::Sdf(shape) => shape.size().x,
            _ => 0.0,
        };
        let left = panel.transform.translation.x - width / 2.0 + STATUS_PADDING * unit;
        labels.push(Label {
            key: "status".into(),
            text: snapshot.status(),
            at: Vec3::new(left, panel.transform.translation.y, LABEL_DEPTH),
            scale: unit,
            left: true,
        });
    }
    for ring in S::Ring::all(snapshot) {
        for item in ring.items(snapshot) {
            let id = petal_id::<S>(&ring, &item);
            let Some(node) = view.get(&id) else {
                continue;
            };
            let scale = node.transform.scale.x;
            let below = Vec2::new(0.0, -(layout.petal_radius() + LABEL_GAP * unit) * scale);
            labels.push(Label {
                key: id,
                text: item.label(snapshot),
                at: (node.transform.translation.truncate() + below).extend(LABEL_DEPTH),
                scale: scale * unit,
                left: false,
            });
        }
    }
    labels
}

/// Marks the text entity of the label with the key it holds.
#[derive(Component, Debug)]
pub struct MenuLabel(pub String);

/// The text entities of the labels of the menu.
type LabelQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static MenuLabel,
        &'static mut Text2d,
        &'static mut Transform,
        &'static mut Visibility,
    ),
>;

/// Keeps one `Text2d` on the HUD layer for every label of the menu, following the petals as they
/// bloom, and removes the labels of petals that are gone.
fn sync_menu_labels<S: RadialHost>(
    layer: usize,
    mut commands: Commands,
    model: &DragModel<S>,
    mut existing: LabelQuery,
) {
    let wanted = labels(model.state(), model.view());
    let mut seen = HashSet::new();
    for (entity, key, mut text, mut transform, mut visibility) in &mut existing {
        let Some(label) = wanted.iter().find(|label| label.key == key.0) else {
            commands.entity(entity).despawn();
            continue;
        };
        seen.insert(label.key.clone());
        if text.0 != label.text {
            text.0.clone_from(&label.text);
        }
        transform.translation = label.at;
        transform.scale = Vec3::splat(label.scale.max(HIDDEN));
        *visibility = if label.is_shown() {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
    for label in wanted.iter().filter(|label| !seen.contains(&label.key)) {
        commands.spawn((
            MenuLabel(label.key.clone()),
            Text2d::new(label.text.clone()),
            TextFont {
                font_size: FontSize::Px(LABEL_SIZE),
                ..default()
            },
            TextColor(Color::WHITE),
            TextLayout::justify(if label.left {
                Justify::Left
            } else {
                Justify::Center
            }),
            if label.left {
                Anchor::CENTER_LEFT
            } else {
                Anchor::CENTER
            },
            Text2dShadow::default(),
            RenderLayers::layer(layer),
            Transform::from_translation(label.at).with_scale(Vec3::splat(label.scale.max(HIDDEN))),
        ));
    }
}

/// Draws the labels of the radial menu of the state `S` as `Text2d` entities on the layer of
/// the [`Theme`], following their petals. It needs the [`DragModel<S>`] that the
/// [`DraggablePlugin`](crate::DraggablePlugin) inserts.
pub struct RadialMenuPlugin<S> {
    layer: usize,
    marker: PhantomData<fn() -> S>,
}

impl<S: RadialHost> RadialMenuPlugin<S> {
    /// The plugin for a menu built with `theme`, whose labels go on its layer.
    #[must_use]
    pub fn new(theme: Theme) -> Self {
        Self {
            layer: theme.layer,
            marker: PhantomData,
        }
    }
}

impl<S: RadialHost> Plugin for RadialMenuPlugin<S> {
    fn build(&self, app: &mut App) {
        let layer = self.layer;
        app.add_systems(
            Update,
            move |commands: Commands, model: Res<DragModel<S>>, existing: LabelQuery| {
                sync_menu_labels(layer, commands, &model, existing);
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hud::Screen;
    use crate::{DragContext, Pointer, Rendering, lerp_views};

    /// The render layer of the menu of the demo.
    const LAYER: usize = 5;

    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Page {
        Root,
        Colors,
    }

    #[derive(Clone, Copy, PartialEq, Eq, Debug)]
    enum Entry {
        Open(Page),
        Color(u8),
        Reset,
    }

    /// A tiny interface with two rings: the root opens the colors, which are armed when pressed.
    #[derive(Clone, PartialEq, Debug)]
    struct Demo {
        screen: Screen,
        menu: Option<Page>,
        armed: Option<u8>,
        selected: bool,
        chip: bool,
    }

    impl Demo {
        fn new() -> Self {
            Self {
                screen: Screen::default(),
                menu: None,
                armed: None,
                selected: false,
                chip: false,
            }
        }

        fn open(&self, page: Page) -> Self {
            self.with_menu(Some(page))
        }

        fn selecting(&self) -> Self {
            Self {
                selected: true,
                ..self.clone()
            }
        }
    }

    impl Petal<Demo> for Entry {
        fn name(&self) -> String {
            match self {
                Self::Open(Page::Root) => "root".into(),
                Self::Open(Page::Colors) => "colors".into(),
                Self::Color(index) => format!("color-{index}"),
                Self::Reset => "reset".into(),
            }
        }

        fn label(&self, _demo: &Demo) -> String {
            match self {
                Self::Open(Page::Root) => "Menu".into(),
                Self::Open(Page::Colors) => "Colors".into(),
                Self::Color(index) => format!("Color {index}"),
                Self::Reset => "Reset".into(),
            }
        }

        fn accent(&self) -> Color {
            Color::srgb(0.2, 0.4, 0.7)
        }

        fn glyph(&self) -> Shape {
            glyph::dots(5.0, &[(0.0, 0.0)])
        }

        fn is_active(&self, demo: &Demo) -> bool {
            matches!(self, Self::Color(index) if demo.armed == Some(*index))
        }

        fn press(&self, demo: &Demo) -> Demo {
            match self {
                Self::Open(page) => demo.open(*page),
                Self::Color(index) => Demo {
                    armed: Some(*index),
                    menu: None,
                    ..demo.clone()
                },
                Self::Reset => Demo {
                    armed: None,
                    menu: None,
                    ..demo.clone()
                },
            }
        }
    }

    impl Ring<Demo> for Page {
        type Item = Entry;

        fn name(&self) -> String {
            match self {
                Self::Root => "root".into(),
                Self::Colors => "colors".into(),
            }
        }

        fn items(&self, demo: &Demo) -> Vec<Entry> {
            match self {
                Self::Root => vec![Entry::Open(Self::Colors), Entry::Reset],
                Self::Colors if demo.selected => vec![Entry::Color(0), Entry::Color(1)],
                Self::Colors => (0..3).map(Entry::Color).collect(),
            }
        }

        fn all(_demo: &Demo) -> Vec<Self> {
            vec![Self::Root, Self::Colors]
        }
    }

    impl HudHost for Demo {
        fn screen(&self) -> &Screen {
            &self.screen
        }

        fn with_screen(&self, screen: Screen) -> Self {
            Self {
                screen,
                ..self.clone()
            }
        }
    }

    impl RadialHost for Demo {
        type Ring = Page;

        fn root(&self) -> Page {
            Page::Root
        }

        fn menu(&self) -> Option<Page> {
            self.menu
        }

        fn with_menu(&self, menu: Option<Page>) -> Self {
            Self {
                menu,
                ..self.clone()
            }
        }

        fn pop_center(&self, layout: &Layout) -> Vec2 {
            if !self.selected {
                return layout.home();
            }
            let lift = if self.is_chip() {
                Vec2::Y * layout.chip_lift()
            } else {
                Vec2::ZERO
            };
            layout.keep_inside(Vec2::new(150.0, 80.0) + lift)
        }

        fn is_popped(&self) -> bool {
            self.selected
        }

        fn is_chip(&self) -> bool {
            self.selected && self.chip && self.menu.is_none()
        }

        fn armed(&self) -> Option<Entry> {
            self.armed.map(Entry::Color)
        }

        fn disarmed(&self) -> Self {
            Self {
                armed: None,
                ..self.clone()
            }
        }

        fn status(&self) -> String {
            format!("Demo\n{:?}", self.armed)
        }
    }

    fn theme() -> Theme {
        Theme::new(LAYER)
    }

    fn view(demo: &Demo) -> View<Demo> {
        let mut view = View::new();
        let layout = Layout::new(demo.screen());
        push_menu(&mut view, &Arc::new(demo.clone()), &layout, &theme());
        view
    }

    fn ids(demo: &Demo) -> HashSet<String> {
        view(demo)
            .nodes()
            .iter()
            .map(|node| node.id.to_string())
            .collect()
    }

    fn every_state() -> Vec<Demo> {
        let demo = Demo::new();
        vec![
            demo.clone(),
            demo.open(Page::Root),
            demo.open(Page::Colors),
            demo.selecting(),
            Demo {
                armed: Some(1),
                ..demo
            },
        ]
    }

    /// Drags `node` of `demo` by the spec of the node, to `to`, and returns the drop state.
    fn press(demo: &Demo, node: &str, to: Vec3) -> Demo {
        let view = view(demo);
        let node_ref = view.get(node).unwrap();
        let spec = (node_ref.drag.as_ref().unwrap())();
        let context = DragContext::new(Arc::new(self::view), node.to_owned().into(), demo.clone())
            .with_space(node_ref.drag_space.unwrap());
        spec.compile(&context)(Pointer::at(to)).drop_state
    }

    /// Presses `node` of `demo` and releases it where it stands, as a click does.
    fn tap(demo: &Demo, node: &str) -> Demo {
        let at = view(demo).world_position(node).unwrap();
        press(demo, node, at)
    }

    fn scale_of(view: &View<Demo>, id: &str) -> f32 {
        view.get(id).unwrap().transform.scale.x
    }

    #[test]
    fn every_petal_is_present_in_every_state_but_the_armed_one() {
        let all = ids(&Demo::new());
        for id in [
            "menu/status",
            "menu/hub",
            "menu/pop",
            "menu/root/colors",
            "menu/root/reset",
            "menu/colors/color-0",
            "menu/colors/color-2",
        ] {
            assert!(all.contains(id), "{id}");
        }
        for demo in every_state() {
            let present = ids(&demo);
            let petals = |ids: &HashSet<String>| -> HashSet<String> {
                ids.iter()
                    .filter(|id| !id.starts_with("menu/armed") && !id.contains("/active"))
                    .cloned()
                    .collect()
            };
            if demo.selected {
                assert!(
                    !present.contains("menu/colors/color-2"),
                    "the ring follows the state"
                );
                assert!(present.contains("menu/colors/color-1"));
            } else {
                assert_eq!(petals(&present), petals(&all), "{:?}", demo.menu);
            }
        }
    }

    #[test]
    fn node_ids_are_unique_in_every_state() {
        for demo in every_state() {
            let view = view(&demo);
            let ids: HashSet<_> = view.nodes().iter().map(|node| &node.id).collect();
            assert_eq!(ids.len(), view.len());
        }
    }

    #[test]
    fn the_open_ring_fans_out_and_the_others_wait_at_the_hub_without_size() {
        for demo in [Demo::new(), Demo::new().selecting()] {
            for open in Page::all(&demo) {
                let state = demo.open(open);
                let view = view(&state);
                let layout = Layout::new(state.screen());
                let center = state.pop_center(&layout);
                let half = state.screen.hud / 2.0;
                let mut seen: Vec<Vec2> = Vec::new();
                for ring in Page::all(&state) {
                    for item in ring.items(&state) {
                        let id = petal_id::<Demo>(&ring, &item);
                        let node = view.get(&id).unwrap();
                        let at = node.transform.translation.truncate();
                        if ring == open {
                            assert!((node.transform.scale.x - 1.0).abs() < 1e-6, "{id}");
                            assert!(at.x.abs() < half.x && at.y.abs() < half.y, "{id}: {at}");
                            assert!(
                                at.distance(center) > layout.hub_radius() + layout.petal_radius()
                            );
                            assert!(
                                seen.iter()
                                    .all(|other| other.distance(at) > layout.petal_radius() * 2.0),
                                "{id} does not overlap a sibling"
                            );
                            seen.push(at);
                        } else {
                            assert!(node.transform.scale.x < 0.01, "{id}");
                            assert!(at.distance(center) < 1e-3, "{id}");
                        }
                    }
                }
                assert_eq!(seen.len(), open.items(&state).len());
            }
        }
    }

    #[test]
    fn opening_a_ring_blooms_its_petals_out_of_the_hub_when_the_views_blend() {
        let closed = view(&Demo::new());
        let open = view(&Demo::new().open(Page::Root));
        let id = "menu/root/colors";
        let hub = Demo::new().pop_center(&Layout::new(&Screen::default()));
        let middle = lerp_views(&closed, &open, 0.5);
        let scale = scale_of(&middle, id);
        assert!(scale > 0.2 && scale < 0.8, "{scale}");
        let from_hub = |view: &View<Demo>| {
            view.get(id)
                .unwrap()
                .transform
                .translation
                .truncate()
                .distance(hub)
        };
        assert!(from_hub(&middle) > 0.0 && from_hub(&middle) < from_hub(&open));
    }

    #[test]
    fn the_menu_is_flat_on_the_layer_of_the_theme_and_only_what_is_shown_presses() {
        for demo in every_state() {
            let view = view(&demo);
            assert!(!view.is_empty());
            for node in view.nodes() {
                assert_eq!(node.layer, LAYER, "{}", node.id);
                assert_eq!(node.rendering, Some(Rendering::Flat));
                assert_eq!(node.overshoot, theme().overshoot);
                let part = ["/glyph", "/active", "/border", "/pill"]
                    .iter()
                    .any(|suffix| node.id.ends_with(suffix));
                let hidden = !part && node.transform.scale.x < 0.01;
                assert_eq!(node.drag.is_none(), part || hidden, "{}", node.id);
                assert_eq!(node.pickable, !(part || hidden), "{}", node.id);
                assert_eq!(node.parent.is_some(), part, "{}", node.id);
            }
        }
    }

    #[test]
    fn the_hub_opens_the_first_ring_and_closes_it_and_leaves_other_rings() {
        let start = Demo::new();
        let open = start.hub_pressed();
        assert_eq!(open.menu, Some(Page::Root));
        assert_eq!(open.hub_pressed().menu, None);
        assert_eq!(
            start.open(Page::Colors).hub_pressed().menu,
            Some(Page::Root)
        );
        assert_eq!(tap(&start, "menu/hub").menu, Some(Page::Root));
        let selected = start.selecting();
        assert_eq!(tap(&selected, "menu/pop").menu, Some(Page::Root));
    }

    #[test]
    fn a_petal_opens_a_ring_or_makes_its_choice_and_closes_the_menu() {
        let start = Demo::new();
        assert_eq!(
            Entry::Open(Page::Colors).press(&start).menu,
            Some(Page::Colors)
        );
        let armed = Entry::Color(2).press(&start.open(Page::Colors));
        assert_eq!((armed.armed, armed.menu), (Some(2), None));
        assert!(Entry::Color(2).is_active(&armed) && !Entry::Color(1).is_active(&armed));
        assert!(!Entry::Open(Page::Colors).is_active(&armed));
        let through_the_engine = tap(&start.open(Page::Colors), "menu/colors/color-1");
        assert_eq!(through_the_engine.armed, Some(1));
        let reset = tap(&armed.open(Page::Root), "menu/root/reset");
        assert_eq!((reset.armed, reset.menu), (None, None));
    }

    #[test]
    fn the_current_choice_is_marked_and_the_armed_item_hangs_above_the_hub_until_pressed() {
        let start = Demo::new();
        assert!(!ids(&start).contains("menu/armed"));
        let armed = Demo {
            armed: Some(1),
            ..start
        };
        let view = view(&armed);
        assert!(view.get("menu/colors/color-1/active").is_some());
        assert!(view.get("menu/colors/color-0/active").is_none());
        let hub = view
            .get("menu/hub")
            .unwrap()
            .transform
            .translation
            .truncate();
        let hung = view
            .get("menu/armed")
            .unwrap()
            .transform
            .translation
            .truncate();
        assert!(hung.y > hub.y, "{hung} above {hub}");
        assert_eq!(
            view.get("menu/armed/border").unwrap().parent.as_deref(),
            Some("menu/armed")
        );
        assert!(view.get("menu/armed/glyph").is_some());
        assert_eq!(tap(&armed, "menu/armed").armed, None);
    }

    #[test]
    fn a_press_that_is_dragged_off_its_button_does_nothing_and_the_view_stays_put() {
        let start = Demo::new().open(Page::Colors);
        let id = "menu/colors/color-1";
        let at = view(&start).world_position(id).unwrap();
        assert_eq!(press(&start, id, at + Vec3::X * 500.0), start);
        let view_fn = view(&start);
        let spec = (view_fn.get(id).unwrap().drag.as_ref().unwrap())();
        let context = DragContext::new(Arc::new(self::view), id.to_owned().into(), start.clone())
            .with_space(view_fn.get(id).unwrap().drag_space.unwrap());
        let held = spec.compile(&context)(Pointer::at(at + Vec3::X * 2.0)).view;
        for node in view_fn.nodes() {
            assert_eq!(
                held.world_position(&node.id),
                view_fn.world_position(&node.id),
                "{}",
                node.id
            );
        }
    }

    #[test]
    fn the_status_panel_takes_presses_and_changes_nothing() {
        let demo = Demo::new().open(Page::Colors);
        assert_eq!(press(&demo, "menu/status", Vec3::new(3.0, 4.0, 0.0)), demo);
        let view = view(&demo);
        let layout = Layout::new(demo.screen());
        let panel = view
            .get("menu/status")
            .unwrap()
            .transform
            .translation
            .truncate();
        assert!(
            panel.x < 0.0 && panel.y > 0.0,
            "it hangs in the top left: {panel}"
        );
        let corner = layout.top_left();
        assert!(panel.x > corner.x && panel.y < corner.y);
    }

    fn label<'a>(labels: &'a [Label], key: &str) -> &'a Label {
        labels.iter().find(|label| label.key == key).unwrap()
    }

    #[test]
    fn every_petal_has_a_label_under_it_and_the_keys_are_unique() {
        for state in [Demo::new(), Demo::new().selecting()] {
            let state = state.open(Page::Colors);
            let view = view(&state);
            let labels = labels(&state, &view);
            let keys: HashSet<&String> = labels.iter().map(|label| &label.key).collect();
            assert_eq!(keys.len(), labels.len());
            for ring in Page::all(&state) {
                for item in ring.items(&state) {
                    let id = petal_id::<Demo>(&ring, &item);
                    let petal = view.get(&id).unwrap().transform.translation;
                    let text = label(&labels, &id);
                    assert_eq!(text.text, item.label(&state));
                    assert!(text.at.y < petal.y && (text.at.x - petal.x).abs() < 1e-3);
                    assert!(text.at.z > petal.z, "the text is in front");
                    assert_eq!(text.is_shown(), ring == Page::Colors, "{id}");
                }
            }
            let status = label(&labels, "status");
            assert!(status.left && status.text == state.status());
            assert!(status.is_shown());
        }
    }

    #[test]
    fn the_hub_rests_in_its_corner_and_pops_up_over_the_selection_instead() {
        let ground = Demo::new();
        let layout = Layout::new(ground.screen());
        let view_on_ground = view(&ground);
        assert!(scale_of(&view_on_ground, "menu/hub") > 0.99);
        assert!(scale_of(&view_on_ground, "menu/pop") < 0.01);
        assert_eq!(ground.pop_center(&layout), layout.home());

        let selected = ground.selecting();
        let view_selected = view(&selected);
        assert!(
            scale_of(&view_selected, "menu/hub") < 0.01,
            "the corner hub hides"
        );
        assert!(
            scale_of(&view_selected, "menu/pop") > 0.99,
            "the hub pops up"
        );
        let pop = view_selected
            .get("menu/pop")
            .unwrap()
            .transform
            .translation
            .truncate();
        assert!(pop.distance(selected.pop_center(&layout)) < 1e-3);
        assert!(pop.distance(layout.home()) > layout.hub_radius());
    }

    #[test]
    fn the_hub_shrinks_to_a_chip_that_hovers_above_the_element_while_the_menu_is_closed() {
        let mut selected = Demo::new().selecting();
        let layout = Layout::new(selected.screen());
        let over = selected.pop_center(&layout);
        selected.chip = true;
        assert!(selected.is_chip());
        let view_chip = view(&selected);
        assert!((scale_of(&view_chip, "menu/pop") - CHIP_SCALE).abs() < 1e-4);
        assert!(selected.pop_center(&layout).y > over.y, "it lifts clear");
        let open = selected.open(Page::Root);
        assert!(!open.is_chip(), "an open menu is full size");
        assert!(scale_of(&view(&open), "menu/pop") > 0.99);
    }

    #[test]
    fn the_glyph_helpers_accept_either_winding() {
        let square = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)];
        let mut reversed = square;
        reversed.reverse();
        let a = glyph::outline(&square).build();
        let b = glyph::outline(&reversed).build();
        assert_eq!(a.count_vertices(), b.count_vertices());
        assert!(glyph::hub().build().count_vertices() > 0);
        assert!(
            glyph::dots(2.0, &[(0.0, 0.0), (5.0, 0.0)])
                .build()
                .count_vertices()
                > 0
        );
    }

    fn labels_of(demo: Demo) -> Vec<(String, bool, RenderLayers)> {
        let mut app = App::new();
        app.insert_resource(DragModel::new(demo, view))
            .add_plugins(RadialMenuPlugin::<Demo>::new(theme()));
        app.update();
        app.update();
        let mut query = app
            .world_mut()
            .query::<(&MenuLabel, &Visibility, &RenderLayers)>();
        query
            .iter(app.world())
            .map(|(label, visibility, layers)| {
                (
                    label.0.clone(),
                    *visibility != Visibility::Hidden,
                    layers.clone(),
                )
            })
            .collect()
    }

    #[test]
    fn the_plugin_keeps_one_text_on_the_layer_of_the_theme_for_every_label() {
        let demo = Demo::new().open(Page::Colors);
        let texts = labels_of(demo.clone());
        let expected = labels(&demo, &view(&demo));
        assert_eq!(texts.len(), expected.len());
        for (key, shown, layers) in &texts {
            assert_eq!(*layers, RenderLayers::layer(LAYER));
            let wanted = label(&expected, key);
            assert_eq!(*shown, wanted.is_shown(), "{key}");
        }
        assert!(
            texts
                .iter()
                .any(|(key, shown, _)| key == "menu/colors/color-1" && *shown)
        );
        assert!(
            texts
                .iter()
                .any(|(key, shown, _)| key == "menu/root/reset" && !*shown)
        );
    }
}
