//! A heads-up display: its screen, its layout, its theme and the builders of its shapes.
//!
//! Everything on a HUD is a node of a HUD camera, which shows a fixed height and a width that
//! follows the window. Its elements are rounded signed distance fields: a fill and a border drawn
//! over it, in the colors of one [`Theme`], so panels, hubs and petals look like one family.
//! Every element tweens with a small overshoot when the view changes.
//!
//! Nothing here is placed by a pixel position of its own. The [`Layout`] works out where the
//! corner anchors and the rings of petals go from the size of the [`Screen`], and keeps every
//! petal on it. The [`Screen`] says how big the HUD is and how the scene camera projects, so a
//! menu can pop up over the element it edits. It is part of the state of the interface, which
//! keeps the view a pure function of that state: the state says so by implementing [`HudHost`],
//! and the [`HudPlugin`] keeps it in step with the window and the camera.
//!
//! The builders are methods of the [`Theme`], which also names the render layer the HUD camera
//! draws, so no layer number is fixed by the library. The [`radial`](crate::radial) module builds
//! a radial menu out of them.
//!
//! ```no_run
//! use bevy::prelude::*;
//! use bevy_composable_dragging::hud::{HudHost, HudPlugin, HudProjectionCamera, Screen};
//! use bevy_composable_dragging::{DraggablePlugin, View};
//!
//! #[derive(Clone)]
//! struct Editor {
//!     screen: Screen,
//! }
//!
//! impl HudHost for Editor {
//!     fn screen(&self) -> &Screen {
//!         &self.screen
//!     }
//!
//!     fn with_screen(&self, screen: Screen) -> Self {
//!         Self { screen }
//!     }
//! }
//!
//! fn view(_editor: &Editor) -> View<Editor> {
//!     View::new()
//! }
//!
//! fn setup(mut commands: Commands) {
//!     commands.spawn((Camera3d::default(), HudProjectionCamera));
//! }
//!
//! App::new()
//!     .add_plugins(DefaultPlugins)
//!     .add_plugins(DraggablePlugin::new(Editor { screen: Screen::default() }, view))
//!     .add_plugins(HudPlugin::<Editor>::new(720.0))
//!     .add_systems(Startup, setup)
//!     .run();
//! ```

use std::f32::consts::{FRAC_PI_2, PI, TAU};
use std::marker::PhantomData;

use bevy::camera::CameraProjection;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::{DragModel, ViewNode};

#[cfg(feature = "gradmesh")]
use bevy_gradmesh::SdfMaterial2d;

/// The height of the HUD camera that [`Screen::default`] assumes, in HUD units.
pub const DEFAULT_HEIGHT: f32 = 720.0;
/// The render layer of the HUD in [`Theme::default`]. Layer `0` is the default layer of Bevy
/// cameras, so the HUD takes the second free one and leaves `1` for overlays.
pub const DEFAULT_LAYER: usize = 2;
/// The depth of panels, the back of the layers of the HUD.
pub const PANEL_DEPTH: f32 = 0.0;
/// The depth of the petals of a menu, in front of its panels.
pub const PETAL_DEPTH: f32 = 1.0;
/// The depth of the hubs of a menu, in front of its petals.
pub const HUB_DEPTH: f32 = 2.0;
/// Where the border of an element sits in front of it.
pub const BORDER_DEPTH: f32 = 0.05;
/// Where the glyph of an element sits in front of it.
pub const GLYPH_DEPTH: f32 = 0.1;
/// Where the mark of the chosen item sits in front of it.
pub const MARK_DEPTH: f32 = 0.2;

/// The width of the HUD at which it is drawn at its full size. Narrower windows shrink it.
const REFERENCE_WIDTH: f32 = 1100.0;
/// The smallest scale of the HUD, so it stays legible in a narrow window.
const MIN_UNIT: f32 = 0.62;
/// The gap kept free at the edges of the screen, in HUD units at full size.
const MARGIN: f32 = 16.0;
/// The radius of a petal, of a hub, and of the ring petals fan out on, at full size.
const PETAL_RADIUS: f32 = 24.0;
const HUB_RADIUS: f32 = 30.0;
const RING_RADIUS: f32 = 124.0;
/// The free space kept between two petals, and between a petal and the hub, at full size.
const SPACING: f32 = 6.0;
/// The room below a petal that its label needs, at full size.
const LABEL_ROOM: f32 = 30.0;
/// How far above the element it edits a hub that has shrunk to a chip hovers, at full size.
const CHIP_LIFT: f32 = 70.0;
/// How many directions around an element a chip can hover in.
const CHIP_DIRECTIONS: usize = 16;
/// How much farther from the handles another spot has to be to be preferred, in HUD units, so
/// that a chip keeps straight up unless another way is clearly better.
const CLEARANCE_GAIN: f32 = 1.0;
/// The directions around the hub a fan tries, one after the other, nearest to its preferred
/// direction first.
const FAN_DIRECTIONS: usize = 24;
/// How many places along half a circle are tried, from the nearest, for the next petal of a
/// ring, before the first one that is free is narrowed down.
const ARC_SCAN: usize = 16;
/// How many times the gap before the first free place is halved to find the nearest one.
const ARC_REFINEMENTS: usize = 10;
/// How much of the accent of a button is mixed into white for its border.
const EDGE_WHITENESS: f32 = 0.62;
/// How big a corner of a panel is, at full size.
const PANEL_CORNER: f32 = 12.0;

/// How big the HUD is, and how the scene camera projects, which is what the HUD needs from the
/// window and the camera. It is part of the state, so the view stays a pure function of it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Screen {
    /// The width and height of the HUD camera, in HUD units.
    pub hud: Vec2,
    /// Takes a point of the scene to the clip space of the scene camera.
    pub clip_from_world: Mat4,
}

impl Default for Screen {
    /// A 16:9 window with a HUD of [`DEFAULT_HEIGHT`], looking through a default perspective
    /// camera that stands on the Z axis and looks at the origin.
    fn default() -> Self {
        let camera = Transform::from_xyz(0.0, 0.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y);
        Self::for_camera(DEFAULT_HEIGHT, camera, 16.0 / 9.0)
    }
}

impl Screen {
    /// A screen whose HUD is `hud_height` high and `aspect` times as wide, seen through a scene
    /// camera that takes a point of the scene to `clip_from_world`.
    #[must_use]
    pub fn new(hud_height: f32, aspect: f32, clip_from_world: Mat4) -> Self {
        Self {
            hud: Vec2::new(hud_height * aspect, hud_height),
            clip_from_world,
        }
    }

    /// A screen whose HUD is `hud_height` high and `aspect` times as wide, seen through a
    /// default perspective scene camera at `camera`.
    #[must_use]
    pub fn for_camera(hud_height: f32, camera: Transform, aspect: f32) -> Self {
        let projection = PerspectiveProjection {
            aspect_ratio: aspect,
            ..default()
        };
        Self::new(
            hud_height,
            aspect,
            projection.get_clip_from_view() * camera.to_matrix().inverse(),
        )
    }

    /// Where the world point `world` appears on the HUD, or `None` behind the camera.
    #[must_use]
    pub fn project(&self, world: Vec3) -> Option<Vec2> {
        let clip = self.clip_from_world * world.extend(1.0);
        (clip.w > 0.0).then(|| clip.truncate().truncate() / clip.w * self.hud / 2.0)
    }

    /// A point from which the camera sees `world`: on the line of sight through it, toward the
    /// camera. A perspective camera stands on that line, and an orthographic one looks along it
    /// from anywhere on it, so this is what [`Gizmo::seen_from`](crate::gizmo::Gizmo::seen_from)
    /// and [`CurveHandles::seen_from`](crate::handles::CurveHandles::seen_from) need to know
    /// which handles are seen edge-on, in either projection. It is `None` when `world` is behind
    /// the camera.
    #[must_use]
    pub fn eye_for(&self, world: Vec3) -> Option<Vec3> {
        let clip = self.clip_from_world * world.extend(1.0);
        if clip.w <= 0.0 {
            return None;
        }
        let ndc = clip.truncate() / clip.w;
        let near = self.clip_from_world.inverse() * Vec4::new(ndc.x, ndc.y, 1.0, 1.0);
        (near.w.abs() > f32::EPSILON).then(|| near.truncate() / near.w)
    }

    /// How many world units one HUD unit spans across the screen at the world point `world`, or
    /// `None` behind the camera. A handle that should be `n` HUD units wide, whatever the zoom or
    /// the projection, is `n` times this wide in the scene; in a perspective view it grows with
    /// the distance from the camera.
    #[must_use]
    pub fn world_per_hud(&self, world: Vec3) -> Option<f32> {
        let clip = self.clip_from_world * world.extend(1.0);
        if clip.w <= 0.0 {
            return None;
        }
        let across = clip + Vec4::new(2.0 / self.hud.x * clip.w, 0.0, 0.0, 0.0);
        let inverse = self.clip_from_world.inverse();
        let (from, to) = (inverse * clip, inverse * across);
        Some((from.truncate() / from.w - to.truncate() / to.w).length())
    }
}

/// The state of an interface that has a HUD: it carries the [`Screen`] the HUD lays itself out
/// from. Implement it for the state of the [`DraggablePlugin`](crate::DraggablePlugin) and add a
/// [`HudPlugin`] to keep the screen in step with the window.
pub trait HudHost: Clone + Send + Sync + 'static {
    /// The screen the HUD of this state is laid out for.
    fn screen(&self) -> &Screen;

    /// This state with another `screen`.
    #[must_use]
    fn with_screen(&self, screen: Screen) -> Self;
}

/// Marks the camera whose projection the HUD pops its menus up through: the scene camera.
///
/// The [`HudPlugin`] reads it from the one camera that carries the marker. A scene camera that
/// is already marked with a component of its own is used through [`HudPlugin::camera`] instead.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct HudProjectionCamera;

/// Keeps the [`Screen`] of the state `S` in step with the primary window and the scene camera
/// marked with `M`, which is [`HudProjectionCamera`] unless [`camera`](Self::camera) says
/// otherwise.
///
/// A change of the screen replaces the state, so the plugin waits until nothing is being dragged
/// or animated. It needs the [`DragModel<S>`] that the [`DraggablePlugin`](crate::DraggablePlugin)
/// inserts, and exactly one camera with the marker.
pub struct HudPlugin<S, M = HudProjectionCamera> {
    hud_height: f32,
    marker: PhantomData<fn() -> (S, M)>,
}

impl<S: HudHost> HudPlugin<S> {
    /// The plugin for a HUD camera that shows `hud_height` HUD units vertically.
    #[must_use]
    pub fn new(hud_height: f32) -> Self {
        Self {
            hud_height,
            marker: PhantomData,
        }
    }
}

impl<S: HudHost, M> HudPlugin<S, M> {
    /// Reads the projection from the camera that carries the component `N` instead.
    #[must_use]
    pub fn camera<N: Component>(self) -> HudPlugin<S, N> {
        HudPlugin {
            hud_height: self.hud_height,
            marker: PhantomData,
        }
    }
}

impl<S: HudHost, M: Component> Plugin for HudPlugin<S, M> {
    fn build(&self, app: &mut App) {
        let hud_height = self.hud_height;
        app.add_systems(
            Update,
            move |window: Single<&Window, With<PrimaryWindow>>,
                  camera: Single<(&Camera, &GlobalTransform), With<M>>,
                  mut model: ResMut<DragModel<S>>| {
                let size = window.size();
                if size.min_element() <= 0.0 || model.is_dragging() || model.is_transitioning() {
                    return;
                }
                let (camera, transform) = *camera;
                let clip_from_world = camera.clip_from_view() * transform.to_matrix().inverse();
                let screen = Screen::new(hud_height, size.x / size.y, clip_from_world);
                if let Some(synced) = synced(model.state(), screen) {
                    model.set_state(synced);
                }
            },
        );
    }
}

/// The state with `screen`, or `None` when it has that screen already.
fn synced<S: HudHost>(state: &S, screen: Screen) -> Option<S> {
    (*state.screen() != screen).then(|| state.with_screen(screen))
}

/// Where things go on the HUD of a screen: the same numbers for every element, scaled to the
/// window.
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    size: Vec2,
    unit: f32,
    theme: Theme,
}

impl Layout {
    /// The layout of the HUD of `screen` using the design values from `theme`.
    #[must_use]
    pub fn new(screen: &Screen, theme: Theme) -> Self {
        Self {
            size: screen.hud,
            unit: (screen.hud.x / theme.reference_width).clamp(theme.min_unit, 1.0),
            theme,
        }
    }

    /// The theme this layout is based on.
    #[must_use]
    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// How much the HUD is scaled, from the smallest unit to one.
    #[must_use]
    pub fn unit(&self) -> f32 {
        self.unit
    }

    /// The gap kept free at the edges of the screen.
    #[must_use]
    pub fn margin(&self) -> f32 {
        self.theme.margin * self.unit
    }

    /// The radius of a petal.
    #[must_use]
    pub fn petal_radius(&self) -> f32 {
        self.theme.petal_radius * self.unit
    }

    /// The radius of a hub.
    #[must_use]
    pub fn hub_radius(&self) -> f32 {
        self.theme.hub_radius * self.unit
    }

    /// How far above the element it edits a hub that has shrunk to a chip hovers.
    #[must_use]
    pub fn chip_lift(&self) -> f32 {
        self.theme.chip_lift * self.unit
    }

    /// Where a hub that has shrunk to a chip hovers over the element at `at`, which has handles
    /// at the screen points `avoid`: a chip lift away from the element, in whichever of sixteen
    /// directions is farthest from all of them, the one straight up when it is as good as any.
    /// The handles around an element are of any number and in any direction, and a chip that
    /// sat on one of them would keep it from being pressed.
    #[must_use]
    pub fn chip_spot(&self, at: Vec2, avoid: &[Vec2]) -> Vec2 {
        let lift = self.chip_lift();
        let mut best: Option<(f32, Vec2)> = None;
        for step in 0..CHIP_DIRECTIONS {
            let angle = FRAC_PI_2 + TAU * step as f32 / CHIP_DIRECTIONS as f32;
            let spot = self.keep_inside(at + Vec2::from_angle(angle) * lift);
            let clearance = avoid
                .iter()
                .chain(Some(&at))
                .map(|point| point.distance(spot))
                .fold(f32::INFINITY, f32::min);
            if best.is_none_or(|(most, _)| clearance > most + CLEARANCE_GAIN) {
                best = Some((clearance, spot));
            }
        }
        best.map_or(at, |(_, spot)| spot)
    }

    /// The corner the hub rests in while nothing is selected.
    #[must_use]
    pub fn home(&self) -> Vec2 {
        let inset = self.margin() + self.hub_radius();
        -self.size / 2.0 + Vec2::splat(inset)
    }

    /// The top left corner of the HUD inside its margin, where a status panel hangs.
    #[must_use]
    pub fn top_left(&self) -> Vec2 {
        Vec2::new(-self.size.x / 2.0, self.size.y / 2.0) + Vec2::new(1.0, -1.0) * self.margin()
    }

    /// The area the center of a petal stays in, so its label and border stay on screen.
    fn reach(&self) -> (Vec2, Vec2) {
        let inset = self.margin() + self.petal_radius();
        (
            -self.size / 2.0 + Vec2::new(inset, inset + self.theme.label_room * self.unit),
            self.size / 2.0 - Vec2::splat(inset),
        )
    }

    /// The point nearest to `point` where a petal fits on screen.
    #[must_use]
    pub fn keep_inside(&self, point: Vec2) -> Vec2 {
        let (low, high) = self.reach();
        point.max(low).min(high)
    }

    /// The area a whole petal, label included, stays in: the screen inside its margin.
    fn span(&self) -> (Vec2, Vec2) {
        (
            -self.size / 2.0 + Vec2::splat(self.margin()),
            self.size / 2.0 - Vec2::splat(self.margin()),
        )
    }

    /// The free space kept between two petals, and between a petal and the hub.
    fn spacing(&self) -> f32 {
        self.theme.spacing * self.unit
    }

    /// How far from `center` the farthest place a petal can be is.
    fn farthest(&self, center: Vec2) -> f32 {
        let (low, high) = self.span();
        [
            low,
            high,
            Vec2::new(low.x, high.y),
            Vec2::new(high.x, low.y),
        ]
        .into_iter()
        .map(|corner| corner.distance(center))
        .fold(0.0, f32::max)
    }

    /// The centers of `count` bare petals fanned out around `center`. It is
    /// [`fan_footprints`](Self::fan_footprints) for petals that are only a disc.
    #[must_use]
    pub fn fan(&self, center: Vec2, count: usize) -> Vec<Vec2> {
        let discs = vec![Footprint::disc(self); count];
        self.fan_footprints(center, &discs).centers
    }

    /// Where petals with the given footprints go around the hub at `center`.
    ///
    /// The petals sit on a circle around the hub, in order, each one at the first place along
    /// the circle where its footprint clears the one before it: a petal with a long label takes
    /// more of the arc than one with a short label. The arc faces the middle of the screen, or
    /// turns away from it as far as it must to fit.
    ///
    /// When the petals do not fit on a circle of the usual size, the circle grows by the
    /// least that makes them fit. A circle can only grow so far before it stops being a menu
    /// around a hub, half the shorter side of the screen. If the petals still do not fit, the
    /// first ring keeps as many as it can hold, in order, and the rest go on a second ring
    /// outside it, on the same side, and on a third if that is not enough.
    ///
    /// No two petals ever overlap, and no petal leaves the screen or touches the hub. The
    /// only petal that has no place is one bigger than the screen itself, and it is moved
    /// onto the screen.
    #[must_use]
    pub fn fan_footprints(&self, center: Vec2, footprints: &[Footprint]) -> Fan {
        let first = self.theme.ring_radius * self.unit;
        let mut toward = if center.length() > 1.0 {
            (-center).to_angle()
        } else {
            FRAC_PI_2
        };
        let mut radius = first;
        let mut ceiling = (self.size.min_element() / 2.0).max(first);
        let mut placed = Vec::new();
        let mut fan = Fan {
            centers: Vec::with_capacity(footprints.len()),
            rings: Vec::with_capacity(footprints.len()),
        };
        let mut ring = 0;
        while fan.centers.len() < footprints.len() {
            let rest = &footprints[fan.centers.len()..];
            let (count, points, middle, used) = (1..=rest.len())
                .rev()
                .find_map(|count| {
                    self.grow(center, &rest[..count], toward, &placed, (radius, ceiling))
                        .map(|(points, middle, used)| (count, points, middle, used))
                })
                .unwrap_or_else(|| {
                    let lone = center + Vec2::from_angle(toward) * radius;
                    (1, vec![self.keep_inside(lone)], toward, radius)
                });
            placed.extend(
                points
                    .iter()
                    .zip(rest)
                    .map(|(point, footprint)| footprint.around(*point)),
            );
            fan.centers.extend(points);
            fan.rings.extend(std::iter::repeat_n(ring, count));
            ring += 1;
            toward = middle;
            radius = used + self.petal_radius();
            ceiling = self.farthest(center).max(radius);
        }
        fan
    }

    /// The ring of `footprints` on the smallest circle from the radius `from` up to `ceiling`
    /// where it fits, as the centers of its petals, the direction its arc faces and the radius.
    fn grow(
        &self,
        center: Vec2,
        footprints: &[Footprint],
        toward: f32,
        placed: &[Area],
        (from, ceiling): (f32, f32),
    ) -> Option<(Vec<Vec2>, f32, f32)> {
        let step = self.petal_radius() / 2.0;
        let last = ceiling.max(from);
        std::iter::successors(Some(from), |radius| {
            (*radius < last).then(|| (radius + step).min(last))
        })
        .find_map(|radius| {
            self.ring(center, radius, footprints, toward, placed)
                .map(|(points, middle)| (points, middle, radius))
        })
    }

    /// The ring of `footprints` on the circle of `radius`, in the direction nearest to `toward`
    /// where it fits, as the centers of its petals and the direction its arc faces.
    fn ring(
        &self,
        center: Vec2,
        radius: f32,
        footprints: &[Footprint],
        toward: f32,
        placed: &[Area],
    ) -> Option<(Vec<Vec2>, f32)> {
        let turn = TAU / FAN_DIRECTIONS as f32;
        (0..FAN_DIRECTIONS).find_map(|attempt| {
            let side = if attempt % 2 == 0 { 1.0 } else { -1.0 };
            let middle = toward + side * attempt.div_ceil(2) as f32 * turn;
            let rough = self.lay(radius, footprints, middle);
            let sweep = rough.last()? - rough.first()?;
            let points: Vec<Vec2> = self
                .lay(radius, footprints, middle - sweep / 2.0)
                .into_iter()
                .map(|angle| center + Vec2::from_angle(angle) * radius)
                .collect();
            self.fits(center, &points, footprints, placed)
                .then_some((points, middle))
        })
    }

    /// The angles on the circle of `radius` of petals that follow each other, the first at
    /// `start`, each at the first angle where it clears the one before it.
    fn lay(&self, radius: f32, footprints: &[Footprint], start: f32) -> Vec<f32> {
        let mut angles: Vec<f32> = Vec::with_capacity(footprints.len());
        for (index, footprint) in footprints.iter().enumerate() {
            let angle = match index {
                0 => start,
                _ => self.next_angle(
                    radius,
                    angles[index - 1],
                    (&footprints[index - 1], footprint),
                ),
            };
            angles.push(angle);
        }
        angles
    }

    /// The first angle after `from` on the circle of `radius` where `footprint` clears `behind`
    /// sitting at `from`.
    fn next_angle(
        &self,
        radius: f32,
        from: f32,
        (behind, footprint): (&Footprint, &Footprint),
    ) -> f32 {
        let gap = self.spacing();
        let there = behind.around(Vec2::from_angle(from) * radius);
        let clear = |turn: f32| {
            apart(
                there,
                footprint.around(Vec2::from_angle(from + turn) * radius),
                gap,
            )
        };
        let coarse = PI / ARC_SCAN as f32;
        let Some(first) = (1..=ARC_SCAN)
            .map(|scan| scan as f32 * coarse)
            .find(|turn| clear(*turn))
        else {
            return from + PI;
        };
        let (mut low, mut high) = (first - coarse, first);
        for _ in 0..ARC_REFINEMENTS {
            let middle = low.midpoint(high);
            if clear(middle) {
                high = middle;
            } else {
                low = middle;
            }
        }
        from + high
    }

    /// Whether petals at `points` stay on screen, clear of the hub at `center`, of each other
    /// and of the areas of petals that are `placed` already.
    fn fits(
        &self,
        center: Vec2,
        points: &[Vec2],
        footprints: &[Footprint],
        placed: &[Area],
    ) -> bool {
        let gap = self.spacing();
        let (low, high) = self.span();
        let areas: Vec<Area> = points
            .iter()
            .zip(footprints)
            .map(|(point, footprint)| footprint.around(*point))
            .collect();
        let clear_of_hub = |(from, to): &Area| {
            center.clamp(*from, *to).distance(center) >= self.hub_radius() + gap
        };
        let on_screen = |(from, to): &Area| from.cmpge(low).all() && to.cmple(high).all();
        areas
            .iter()
            .all(|area| on_screen(area) && clear_of_hub(area))
            && areas.iter().enumerate().all(|(index, area)| {
                areas[index + 1..]
                    .iter()
                    .chain(placed)
                    .all(|other| apart(*area, *other, gap))
            })
    }
}

/// A rectangle on the HUD, from its lowest corner to its highest.
type Area = (Vec2, Vec2);

/// Whether two areas are `gap` apart or more.
fn apart(a: Area, b: Area, gap: f32) -> bool {
    a.1.x + gap <= b.0.x || b.1.x + gap <= a.0.x || a.1.y + gap <= b.0.y || b.1.y + gap <= a.0.y
}

/// The room one petal takes up around its center: its disc, and the label that hangs below it.
///
/// The [`Layout`] keeps the footprints of neighbouring petals apart, so a label never runs
/// into the next petal, however long it is.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct Footprint {
    /// The width of the box, centered on the petal.
    pub width: f32,
    /// How far the box reaches above the center of the petal.
    pub above: f32,
    /// How far the box reaches below the center of the petal.
    pub below: f32,
}

impl Footprint {
    /// The footprint of a petal that is only a disc.
    #[must_use]
    pub fn disc(layout: &Layout) -> Self {
        let radius = layout.petal_radius();
        Self {
            width: radius * 2.0,
            above: radius,
            below: radius,
        }
    }

    /// The footprint of a petal with a label in a pill of size `pill`, whose middle hangs `gap`
    /// below the edge of the disc.
    #[must_use]
    pub fn labelled(layout: &Layout, pill: Vec2, gap: f32) -> Self {
        let disc = Self::disc(layout);
        Self {
            width: disc.width.max(pill.x),
            below: disc.below + (gap + pill.y / 2.0).max(0.0),
            ..disc
        }
    }

    /// The box of the footprint of a petal centered at `at`.
    fn around(&self, at: Vec2) -> Area {
        (
            Vec2::new(at.x - self.width / 2.0, at.y - self.below),
            Vec2::new(at.x + self.width / 2.0, at.y + self.above),
        )
    }
}

/// Where the petals of a menu go: the result of [`Layout::fan_footprints`].
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Fan {
    /// The center of every petal, in the order of the footprints it was made from.
    pub centers: Vec<Vec2>,
    /// The ring every petal is on, which is `0` for the ring nearest the hub. A ring is only
    /// started when the one inside it is full.
    pub rings: Vec<usize>,
}

/// The border color that goes with a fill of `accent`.
#[must_use]
pub fn edge(accent: Color) -> Color {
    accent.mix(&Color::WHITE, EDGE_WHITENESS)
}

/// How far a letter advances, as a fraction of the font size, in the monospace font that Bevy
/// draws text with unless an app says otherwise. Every letter takes this much, so a measure that
/// counts letters is exact for it and an upper bound for a proportional font.
const GLYPH_ADVANCE: f32 = 0.6;

/// How wide `text` is when it is drawn at full size with a font of `size`, which is measured
/// from the advance of a letter since the HUD has no font metrics of its own.
#[must_use]
pub fn text_width(text: &str, size: f32) -> f32 {
    text.lines()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0) as f32
        * size
        * GLYPH_ADVANCE
}

/// The look of a HUD, and the builders of its nodes: one set of colors, one border, one
/// tween, and the render layer its camera draws.
///
/// [`Theme::default`] is a dark glass look with warm hubs. Change a field to restyle every
/// panel, hub and petal that is built with the theme.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    /// The render layer of the HUD camera. Every node the theme builds is on it.
    pub layer: usize,
    /// The fill of panels and of the pills behind labels.
    pub fill: Color,
    /// The border of panels and pills.
    pub border: Color,
    /// The color that marks the chosen item.
    pub mark: Color,
    /// The color of the hub, which wears [`ink`](Self::ink) on it.
    pub hub: Color,
    /// The color of the glyphs on petals, which sit on dark accents.
    pub glyph: Color,
    /// The color of the glyph on the hub, which is light enough to need dark ink.
    pub ink: Color,
    /// The width of the border of a panel or a button, at full size.
    pub border_width: f32,
    /// How far the position, scale and rotation of a HUD element run past their target before
    /// they settle, as a fraction of the distance travelled.
    pub overshoot: f32,
    /// How long a change of the HUD takes to settle, in seconds.
    pub tween_seconds: f32,
    /// The radius of a petal, at full size.
    pub petal_radius: f32,
    /// The radius of a hub, at full size.
    pub hub_radius: f32,
    /// The radius of the ring petals fan out on, at full size.
    pub ring_radius: f32,
    /// The free space kept between two petals, and between a petal and the hub, at full size.
    pub spacing: f32,
    /// The room below a petal that its label needs, at full size.
    pub label_room: f32,
    /// How far above the element it edits a hub that has shrunk to a chip hovers, at full size.
    pub chip_lift: f32,
    /// How big a corner of a panel is, at full size.
    pub panel_corner: f32,
    /// The width of the HUD at which it is drawn at its full size. Narrower windows shrink it.
    pub reference_width: f32,
    /// The smallest scale of the HUD, so it stays legible in a narrow window.
    pub min_unit: f32,
    /// The gap kept free at the edges of the screen, in HUD units at full size.
    pub margin: f32,
}

impl Default for Theme {
    fn default() -> Self {
        Self::new(DEFAULT_LAYER)
    }
}

impl Theme {
    /// The default look on the render layer `layer`.
    #[must_use]
    pub fn new(layer: usize) -> Self {
        Self {
            layer,
            fill: Color::srgba(0.07, 0.08, 0.11, 0.86),
            border: Color::srgba(0.82, 0.86, 0.95, 0.9),
            mark: Color::srgb(1.0, 0.97, 0.85),
            hub: Color::srgb(0.98, 0.8, 0.3),
            glyph: Color::WHITE,
            ink: Color::srgb(0.1, 0.1, 0.14),
            border_width: 2.5,
            overshoot: 0.12,
            tween_seconds: 0.32,
            petal_radius: PETAL_RADIUS,
            hub_radius: HUB_RADIUS,
            ring_radius: RING_RADIUS,
            spacing: SPACING,
            label_room: LABEL_ROOM,
            chip_lift: CHIP_LIFT,
            panel_corner: PANEL_CORNER,
            reference_width: REFERENCE_WIDTH,
            min_unit: MIN_UNIT,
            margin: MARGIN,
        }
    }

    /// The width of a border in `layout`.
    #[must_use]
    pub fn border_width(&self, layout: &Layout) -> f32 {
        self.border_width * layout.unit()
    }

    /// Makes `node` a node of the HUD: flat, on the layer only the HUD camera draws and picks,
    /// and with the overshoot. Every node of the HUD needs it, children too, or a child would
    /// ease differently from its parent and slide off it while they animate.
    #[must_use]
    pub fn on_hud<S>(&self, node: ViewNode<S>) -> ViewNode<S> {
        node.flat()
            .on_layer(self.layer)
            .with_overshoot(self.overshoot)
    }

    fn node<S>(
        &self,
        id: impl Into<String>,
        material: SdfMaterial2d,
        color: Color,
        at: Vec3,
    ) -> ViewNode<S> {
        self.on_hud(ViewNode::sdf(id.into(), material).with_color(color).at(at))
    }

    /// A filled disc.
    pub fn disc<S>(
        &self,
        id: impl Into<String>,
        radius: f32,
        color: Color,
        at: Vec3,
    ) -> ViewNode<S> {
        let material =
            SdfMaterial2d::ellipse(Vec2::splat(radius)).with_padding(crate::sdf::PADDING);
        self.node(id, material, color, at)
    }

    /// The border of a disc, drawn inside its outline.
    pub fn disc_border<S>(
        &self,
        id: impl Into<String>,
        radius: f32,
        width: f32,
        color: Color,
        at: Vec3,
    ) -> ViewNode<S> {
        let material = SdfMaterial2d::ellipse(Vec2::splat(radius))
            .with_rim(width)
            .with_padding(crate::sdf::PADDING);
        self.node(id, material, color, at)
    }

    /// A filled rounded rectangle.
    pub fn plate<S>(
        &self,
        id: impl Into<String>,
        size: Vec2,
        corner: f32,
        color: Color,
        at: Vec3,
    ) -> ViewNode<S> {
        let material = SdfMaterial2d::rect(size)
            .with_corner_radius(corner)
            .with_padding(crate::sdf::PADDING);
        self.node(id, material, color, at)
    }

    /// The border of a rounded rectangle, drawn inside its outline.
    pub fn plate_border<S>(
        &self,
        id: impl Into<String>,
        size: Vec2,
        corner: f32,
        width: f32,
        color: Color,
        at: Vec3,
    ) -> ViewNode<S> {
        let material = SdfMaterial2d::rect(size)
            .with_corner_radius(corner)
            .with_rim(width)
            .with_padding(crate::sdf::PADDING);
        self.node(id, material, color, at)
    }

    /// The nodes of a bordered panel with its center at `center`: the plate, and its border as a
    /// child so the two tween as one. The plate is the first node.
    #[must_use]
    pub fn panel<S>(
        &self,
        id: &str,
        size: Vec2,
        center: Vec2,
        layout: &Layout,
    ) -> [ViewNode<S>; 2] {
        let corner = (self.panel_corner * layout.unit()).min(size.min_element() / 2.0);
        [
            self.plate(id, size, corner, self.fill, center.extend(PANEL_DEPTH)),
            self.plate_border(
                format!("{id}/border"),
                size,
                corner,
                self.border_width(layout),
                self.border,
                Vec3::new(0.0, 0.0, BORDER_DEPTH),
            )
            .with_parent(id.to_owned()),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scene camera above the ground, looking a little ahead of its feet.
    fn camera() -> Transform {
        Transform::from_xyz(0.0, 9.5, 8.5).looking_at(Vec3::new(0.0, 0.0, 0.2), Vec3::Y)
    }

    fn screen(width: f32) -> Screen {
        Screen {
            hud: Vec2::new(width, DEFAULT_HEIGHT),
            ..Screen::default()
        }
    }

    #[derive(Clone)]
    struct Demo {
        screen: Screen,
    }

    impl HudHost for Demo {
        fn screen(&self) -> &Screen {
            &self.screen
        }

        fn with_screen(&self, screen: Screen) -> Self {
            Self { screen }
        }
    }

    #[test]
    fn a_screen_is_as_high_as_asked_and_as_wide_as_the_window() {
        let screen = Screen::for_camera(500.0, camera(), 2.0);
        assert_eq!(screen.hud, Vec2::new(1000.0, 500.0));
        let made = Screen::new(500.0, 2.0, screen.clip_from_world);
        assert_eq!(made, screen);
        assert_eq!(
            Screen::default().hud,
            Vec2::new(DEFAULT_HEIGHT * 16.0 / 9.0, DEFAULT_HEIGHT)
        );
    }

    #[test]
    fn a_hud_unit_spans_the_same_world_length_everywhere_in_an_orthographic_view() {
        let mut orthographic = OrthographicProjection {
            near: 0.0,
            far: 100.0,
            scaling_mode: bevy::camera::ScalingMode::Fixed {
                width: 16.0,
                height: 9.0,
            },
            ..OrthographicProjection::default_3d()
        };
        orthographic.update(16.0, 9.0);
        let projection = orthographic.get_clip_from_view();
        let screen = Screen::new(450.0, 16.0 / 9.0, projection);
        for at in [
            Vec3::ZERO,
            Vec3::new(3.0, 1.0, -40.0),
            Vec3::new(-7.0, 4.0, -90.0),
        ] {
            let span = screen.world_per_hud(at).unwrap();
            assert!((span - 0.02).abs() < 1e-5, "{at}: {span}");
        }
    }

    #[test]
    fn a_hud_unit_spans_more_of_the_world_the_further_it_is_in_a_perspective_view() {
        let screen = Screen::default();
        let near = screen.world_per_hud(Vec3::new(0.0, 0.0, 5.0)).unwrap();
        let far = screen.world_per_hud(Vec3::new(0.0, 0.0, -10.0)).unwrap();
        assert!(far > near);
        assert!((far / near - 20.0 / 5.0).abs() < 1e-3, "{near} {far}");
        let flat = screen.project(Vec3::new(near, 0.0, 5.0)).unwrap().x;
        assert!((flat - 1.0).abs() < 1e-3, "one HUD unit across: {flat}");
        assert_eq!(screen.world_per_hud(Vec3::new(0.0, 0.0, 20.0)), None);
    }

    #[test]
    fn the_point_the_camera_looks_at_projects_to_the_middle_of_the_hud() {
        let screen = Screen::for_camera(DEFAULT_HEIGHT, camera(), 16.0 / 9.0);
        let middle = screen.project(Vec3::new(0.0, 0.0, 0.2)).unwrap();
        assert!(middle.length() < 1.0, "{middle}");
        let right = screen.project(Vec3::new(2.0, 0.0, 0.2)).unwrap();
        assert!(right.x > 20.0 && right.y.abs() < 5.0, "{right}");
        let up = screen.project(Vec3::new(0.0, 2.0, 0.2)).unwrap();
        assert!(up.y > 20.0, "higher things appear higher: {up}");
    }

    #[test]
    fn a_point_behind_the_camera_is_nowhere() {
        let screen = Screen::for_camera(DEFAULT_HEIGHT, camera(), 16.0 / 9.0);
        assert_eq!(screen.project(Vec3::new(0.0, 9.5, 30.0)), None);
    }

    #[test]
    fn a_state_follows_its_screen_only_when_the_screen_changes() {
        let demo = Demo {
            screen: Screen::default(),
        };
        assert!(synced(&demo, Screen::default()).is_none());
        let wide = Screen::new(DEFAULT_HEIGHT, 3.0, Mat4::IDENTITY);
        let followed = synced(&demo, wide).unwrap();
        assert_eq!(*followed.screen(), wide);
    }

    #[test]
    fn the_unit_shrinks_with_the_window_and_never_grows_or_vanishes() {
        let theme = Theme::default();
        assert_eq!(Layout::new(&screen(2400.0), theme).unit(), 1.0);
        assert_eq!(Layout::new(&screen(1280.0), theme).unit(), 1.0);
        let narrow = Layout::new(&screen(800.0), theme).unit();
        assert!((theme.min_unit..1.0).contains(&narrow));
        assert_eq!(Layout::new(&screen(100.0), theme).unit(), theme.min_unit);
    }

    #[test]
    fn the_anchors_stay_in_their_corners_whatever_the_window() {
        let theme = Theme::default();
        for width in [480.0, 800.0, 1280.0, 2400.0] {
            let layout = Layout::new(&screen(width), theme);
            let half = layout.size / 2.0;
            let home = layout.home();
            assert!(home.x < 0.0 && home.y < 0.0);
            assert!(home.x - layout.hub_radius() >= -half.x);
            assert!(home.y - layout.hub_radius() >= -half.y);
            let top_left = layout.top_left();
            assert!(top_left.x > -half.x && top_left.y < half.y);
        }
    }

    /// The footprint of a petal whose label is `letters` letters long, as the radial menu
    /// builds it.
    fn labelled(layout: &Layout, letters: usize) -> Footprint {
        let unit = layout.unit();
        let pill = Vec2::new(text_width(&"x".repeat(letters), 14.0) + 14.0, 20.0) * unit;
        Footprint::labelled(layout, pill, 12.0 * unit)
    }

    /// The centers the fan of `count` petals with labels of `letters` letters has around
    /// `center`.
    fn fan_of(layout: &Layout, center: Vec2, count: usize, letters: usize) -> Fan {
        layout.fan_footprints(center, &vec![labelled(layout, letters); count])
    }

    /// The places a hub is tried at: the corners it rests in, the middle and the edges.
    fn hubs(layout: &Layout) -> [Vec2; 5] {
        let half = layout.size / 2.0;
        [
            layout.home(),
            -layout.home(),
            Vec2::ZERO,
            Vec2::new(half.x * 0.9, 0.0),
            Vec2::new(-half.x * 0.6, half.y * 0.6),
        ]
    }

    #[test]
    fn a_fan_never_overlaps_leaves_the_screen_or_touches_the_hub() {
        let theme = Theme::default();
        for width in [480.0, 800.0, 1280.0, 2000.0] {
            let layout = Layout::new(&screen(width), theme);
            let (low, high) = layout.span();
            let gap = layout.spacing();
            for center in hubs(&layout) {
                for count in [0, 1, 2, 3, 5, 8, 12] {
                    for letters in [0, 3, 8, 14] {
                        let fan = fan_of(&layout, center, count, letters);
                        let context = format!("{width} {center} {count} {letters}");
                        assert_eq!(fan.centers.len(), count, "{context}");
                        assert_eq!(fan.rings.len(), count, "{context}");
                        let feet = labelled(&layout, letters);
                        let areas: Vec<Area> =
                            fan.centers.iter().map(|at| feet.around(*at)).collect();
                        for (index, area) in areas.iter().enumerate() {
                            assert!(
                                area.0.cmpge(low).all() && area.1.cmple(high).all(),
                                "{context}: {index} is off the screen"
                            );
                            assert!(
                                center.clamp(area.0, area.1).distance(center)
                                    >= layout.hub_radius() + gap - 1e-3,
                                "{context}: {index} touches the hub"
                            );
                            for (other, behind) in areas.iter().enumerate().skip(index + 1) {
                                assert!(
                                    apart(*area, *behind, gap - 1e-3),
                                    "{context}: {index} and {other} overlap"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_fan_that_fits_keeps_the_usual_circle_and_one_ring() {
        let theme = Theme::default();
        let layout = Layout::new(&screen(1280.0), theme);
        let fan = fan_of(&layout, Vec2::ZERO, 4, 6);
        assert!(fan.rings.iter().all(|ring| *ring == 0));
        for at in &fan.centers {
            assert!((at.length() - RING_RADIUS * layout.unit()).abs() < 1e-3);
        }
    }

    #[test]
    fn a_fan_takes_more_arc_for_longer_labels() {
        let theme = Theme::default();
        let layout = Layout::new(&screen(1280.0), theme);
        let spread = |letters| {
            let fan = fan_of(&layout, Vec2::ZERO, 4, letters);
            fan.centers[0].distance(fan.centers[3])
        };
        assert!(spread(3) < spread(14));
    }

    #[test]
    fn the_circle_grows_by_the_least_that_makes_the_petals_fit() {
        let mut grown = 0;
        let theme = Theme::default();
        for width in [640.0, 1280.0] {
            let layout = Layout::new(&screen(width), theme);
            let step = layout.petal_radius() / 2.0;
            let feet = vec![labelled(&layout, 9); 5];
            for center in hubs(&layout) {
                let fan = layout.fan_footprints(center, &feet);
                if fan.rings.iter().any(|ring| *ring > 0) {
                    continue;
                }
                let radius = fan.centers[0].distance(center);
                let first = RING_RADIUS * layout.unit();
                assert!(radius >= first - 1e-3);
                if radius > first + 1e-3 {
                    grown += 1;
                    let toward = if center.length() > 1.0 {
                        (-center).to_angle()
                    } else {
                        FRAC_PI_2
                    };
                    assert!(
                        layout
                            .ring(center, radius - step, &feet, toward, &[])
                            .is_none(),
                        "{width} {center}: a smaller circle fits"
                    );
                }
            }
        }
        assert!(grown > 0, "some hub needed a bigger circle");
    }

    #[test]
    fn a_second_ring_opens_only_when_the_first_is_full_and_goes_on_the_same_side() {
        let theme = Theme::default();
        let layout = Layout::new(&screen(1280.0), theme);
        let center = layout.home();
        let fan = fan_of(&layout, center, 12, 14);
        let inner: Vec<Vec2> = (0..12)
            .filter(|index| fan.rings[*index] == 0)
            .map(|index| fan.centers[index])
            .collect();
        let outer: Vec<Vec2> = (0..12)
            .filter(|index| fan.rings[*index] > 0)
            .map(|index| fan.centers[index])
            .collect();
        assert!(!inner.is_empty() && !outer.is_empty(), "{:?}", fan.rings);
        assert!(
            fan.rings.windows(2).all(|pair| pair[0] <= pair[1]),
            "items fill the rings in order: {:?}",
            fan.rings
        );
        let reach = |points: &[Vec2]| {
            points
                .iter()
                .map(|at| at.distance(center))
                .fold(f32::MAX, f32::min)
        };
        assert!(reach(&outer) > reach(&inner));
        let side = |points: &[Vec2]| {
            points
                .iter()
                .map(|at| (*at - center).normalize())
                .sum::<Vec2>()
                .normalize()
        };
        assert!(side(&inner).dot(side(&outer)) > 0.9, "the same side");
        let no_room = fan_of(&layout, center, 1, 14);
        assert_eq!(no_room.rings, [0], "one petal needs no second ring");
    }

    #[test]
    fn a_petal_bigger_than_the_screen_is_moved_onto_it_and_the_rest_still_fit() {
        let theme = Theme::default();
        let layout = Layout::new(&screen(480.0), theme);
        let huge = Footprint {
            width: layout.size.x * 2.0,
            ..labelled(&layout, 3)
        };
        let fan = layout.fan_footprints(Vec2::ZERO, &[labelled(&layout, 3), huge]);
        assert_eq!(fan.centers.len(), 2);
        assert!(layout.keep_inside(fan.centers[1]).distance(fan.centers[1]) < 0.5);
    }

    #[test]
    fn a_fan_clears_the_hub_and_prefers_the_middle_of_the_screen() {
        let theme = Theme::default();
        let layout = Layout::new(&Screen::default(), theme);
        let home = layout.home();
        for petal in layout.fan(home, 3) {
            assert!(petal.distance(home) > layout.hub_radius() + layout.petal_radius());
            assert!(petal.length() < home.length(), "{petal}");
        }
        assert_eq!(layout.fan(home, 0).len(), 0);
    }

    #[test]
    fn a_chip_hovers_higher_in_a_bigger_window() {
        let theme = Theme::default();
        let wide = Layout::new(&screen(2000.0), theme).chip_lift();
        let narrow = Layout::new(&screen(700.0), theme).chip_lift();
        assert!(narrow < wide);
    }

    #[test]
    fn borders_are_lighter_than_their_fill_and_text_widths_grow_with_the_text() {
        let accent = Color::srgb(0.2, 0.3, 0.8);
        assert!(edge(accent).luminance() > accent.luminance());
        assert!(text_width("Select", 14.0) < text_width("Select the wall", 14.0));
        assert!((text_width("a\nabcd", 10.0) - 4.0 * 10.0 * GLYPH_ADVANCE).abs() < 1e-4);
        assert_eq!(text_width("", 10.0), 0.0);
    }

    #[test]
    fn hud_nodes_tween_with_the_overshoot_on_the_layer_of_the_theme() {
        for theme in [Theme::default(), Theme::new(7)] {
            let layout = Layout::new(&Screen::default(), theme);
            let [plate, border] =
                theme.panel::<Demo>("p", Vec2::new(100.0, 40.0), Vec2::ZERO, &layout);
            for node in [&plate, &border] {
                assert_eq!(node.layer, theme.layer);
                assert_eq!(node.overshoot, theme.overshoot);
            }
            assert_eq!(border.parent.as_deref(), Some("p"));
        }
        assert_eq!(Theme::default().layer, DEFAULT_LAYER);
    }

    #[test]
    fn the_theme_colors_the_nodes_and_scales_the_border_with_the_hud() {
        let theme = Theme {
            fill: Color::srgb(1.0, 0.0, 0.0),
            border_width: 4.0,
            ..Theme::default()
        };
        let layout = Layout::new(&screen(550.0), theme);
        assert_eq!(theme.border_width(&layout), 4.0 * theme.min_unit);
        let [plate, _] =
            theme.panel::<Demo>("p", Vec2::new(100.0, 40.0), Vec2::new(5.0, 6.0), &layout);
        assert_eq!(plate.color, theme.fill.into());
        assert_eq!(
            plate.transform.translation,
            Vec3::new(5.0, 6.0, PANEL_DEPTH)
        );
        let disc = theme.disc::<Demo>("d", 10.0, Color::WHITE, Vec3::ZERO);
        let ring = theme.disc_border::<Demo>("r", 10.0, 2.0, Color::WHITE, Vec3::ZERO);
        assert!(disc.drag.is_none() && ring.drag.is_none());
        assert_eq!(disc.layer, theme.layer);
    }

    #[test]
    fn the_eye_of_a_screen_is_on_the_line_the_camera_sees_a_point_along() {
        let world = Vec3::new(2.0, 0.5, 1.0);
        let perspective = Screen::default();
        let eye = perspective.eye_for(world).unwrap();
        let camera = Vec3::new(0.0, 0.0, 10.0);
        let along = (eye - world).normalize().dot((camera - world).normalize());
        assert!(along > 0.9999, "{along}");

        let position = Transform::from_xyz(10.0, 10.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y);
        let projection = OrthographicProjection::default_3d();
        let orthographic = Screen::new(
            DEFAULT_HEIGHT,
            16.0 / 9.0,
            projection.get_clip_from_view() * position.to_matrix().inverse(),
        );
        let eye = orthographic.eye_for(world).unwrap();
        let along = (eye - world).normalize().dot(Vec3::ONE.normalize()).abs();
        assert!(along > 0.9999, "{along}");
        assert_eq!(
            perspective.eye_for(Vec3::Z * 50.0),
            None,
            "behind the camera"
        );
    }

    #[test]
    fn a_chip_hovers_straight_up_unless_a_handle_is_there() {
        let theme = Theme::default();
        let layout = Layout::new(&Screen::default(), theme);
        let up = Vec2::Y * layout.chip_lift();
        assert!(layout.chip_spot(Vec2::ZERO, &[]).distance(up) < 1e-3);
        assert!(
            layout
                .chip_spot(Vec2::ZERO, &[Vec2::X * 400.0])
                .distance(up)
                < 1e-3
        );

        let blocked = layout.chip_spot(Vec2::ZERO, &[up]);
        assert!(blocked.distance(up) > layout.chip_lift());
        assert!((blocked.length() - layout.chip_lift()).abs() < 1e-3);

        let ring: Vec<Vec2> = (0..8)
            .map(|step| Vec2::from_angle(step as f32 * TAU / 8.0) * layout.chip_lift())
            .collect();
        let spot = layout.chip_spot(Vec2::ZERO, &ring);
        let clearance = |spot: Vec2| {
            ring.iter()
                .map(|point| point.distance(spot))
                .fold(f32::MAX, f32::min)
        };
        assert!(clearance(spot) >= clearance(up), "{spot} against {up}");
    }

    #[test]
    fn a_chip_stays_on_the_screen_beside_an_element_at_the_edge() {
        let theme = Theme::default();
        let layout = Layout::new(&Screen::default(), theme);
        let corner = layout.top_left();
        let spot = layout.chip_spot(corner, &[]);
        assert_eq!(spot, layout.keep_inside(spot));
    }
}
