//! The state of the editor: the track design, what is selected, the tool and the edits.
//!
//! The state is a plain value. Every edit takes the state and returns the next one, through the
//! editing operations of the `TrackDesign`, so the limits of the track apply to every edit. A
//! design that cannot be driven is kept and shown, with the reason in the status, so it can be
//! fixed with the handles; only a design that can be driven is built into a road.

use std::f32::consts::PI;
use std::sync::Arc;

use bevy::prelude::*;
use bevy_composable_dragging::gizmo::GizmoReveal;
use bevy_composable_dragging::hud::{HudHost, Layout, Screen};
use bevy_composable_dragging::ops::Snap;
use bevy_composable_dragging::radial::RadialHost;

use crate::ground::{Brush, CHUNKS, Ground};
use crate::menu::Ring;
use crate::track::{BakedTrack, ControlPoint, RoadFrame, TrackDesign, TrackError};

/// The radius of the gem of a control point on the screen, in HUD units.
pub const HANDLE_HUD_RADIUS: f32 = 9.0;
/// The size of the gizmo of a control point on the screen, in HUD units.
pub const GIZMO_HUD_SIZE: f32 = 70.0;
/// How much a petal of the road ring banks the road, in radians.
const BANK_STEP: f32 = PI / 36.0;
/// How much a petal of the road ring widens or narrows the road, as a factor.
const WIDTH_STEP: f32 = 1.1;
/// How many turn radii of the smallest drivable loop the map is framed to reach from the middle
/// of the track at least, which leaves room for a track of several such loops.
const ROOM_LOOPS: f32 = 4.0;
/// How many times the half-width of the space of the map the land reaches, so a track can be
/// pulled out to the edge of the map and still have land around it.
const LAND_REACH: f32 = 2.0;
/// The part of the radius of the brush, from the edge inwards, over which it fades out.
const BRUSH_FALLOFF: f32 = 0.9;
/// How many times the reach of what is framed the space of the map reaches from its middle:
/// the ground that the map can be slid over and zoomed out to, drawn with a grid.
const SPACE_REACH: f32 = 4.0;

/// The space of a map framed around `points`: the middle of their bounds and the half-width of
/// the square of ground, around that middle, that the map covers. It reaches a few times as far
/// as the points do, so the track sits in the open with room to be seen from every side.
pub fn space_of(points: &[Vec3]) -> (Vec3, f32) {
    let (low, high) = points.iter().fold(
        (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
        |(low, high), point| (low.min(*point), high.max(*point)),
    );
    if points.is_empty() {
        return (Vec3::ZERO, 0.0);
    }
    let middle = (low + high) / 2.0;
    let half = points
        .iter()
        .map(|point| (point.x - middle.x).abs().max((point.z - middle.z).abs()))
        .fold(0.0, f32::max);
    (middle, half * SPACE_REACH)
}

/// What the handles and the pointer do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    /// Pressing a gem selects its control point.
    Select,
    /// The gems and the gizmo move the selected control point.
    Move,
    /// The cones and the gizmo raise and lower control points.
    Raise,
    /// The handle at the edge of the road widens it.
    Width,
    /// The ring around the road banks it.
    Roll,
    /// The small gems between the control points pull out new control points.
    Insert,
    /// Pressing a gem removes its control point.
    Delete,
    /// Dragging the land up or down raises or lowers it, in a round patch.
    Hills,
    /// Pressing the land cuts a hole in it, and pressing a hole fills it in again.
    Hole,
    /// Starts and ends a drive on the track.
    Drive,
}

impl Tool {
    /// Every tool, in the order of the ring.
    pub const ALL: [Self; 10] = [
        Self::Select,
        Self::Move,
        Self::Raise,
        Self::Width,
        Self::Roll,
        Self::Insert,
        Self::Delete,
        Self::Hills,
        Self::Hole,
        Self::Drive,
    ];

    /// The name of the tool.
    pub fn label(self) -> &'static str {
        match self {
            Self::Select => "Select",
            Self::Move => "Move",
            Self::Raise => "Raise",
            Self::Width => "Width",
            Self::Roll => "Roll",
            Self::Insert => "Insert",
            Self::Delete => "Delete",
            Self::Hills => "Hills",
            Self::Hole => "Holes",
            Self::Drive => "Test drive",
        }
    }

    /// Whether the tool shapes the land rather than the road.
    pub fn shapes_land(self) -> bool {
        matches!(self, Self::Hills | Self::Hole)
    }

    /// What to do with the tool, in a line.
    pub fn hint(self) -> &'static str {
        match self {
            Self::Hills => "Drag the land up or down",
            Self::Hole => "Press the land to cut a hole, a hole to fill it",
            _ => "",
        }
    }

    /// Whether the tool shows the gizmo of the selected control point. Raising has a cone on
    /// every point, which the vertical arrow of the gizmo would sit on top of.
    pub fn reveals_gizmo(self) -> bool {
        matches!(self, Self::Move)
    }

    /// Whether the tool edits the selected control point where it is, so the menu keeps clear
    /// of it.
    pub fn edits_in_place(self) -> bool {
        matches!(
            self,
            Self::Move | Self::Raise | Self::Width | Self::Roll | Self::Insert
        )
    }
}

/// How finely the handles step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Precision {
    /// No steps.
    Free,
    /// Steps of a metre and of five degrees.
    Fine,
    /// Steps of five metres and of fifteen degrees.
    Coarse,
}

impl Precision {
    /// Every precision, in the order of the ring.
    pub const ALL: [Self; 3] = [Self::Free, Self::Fine, Self::Coarse];

    /// The steps of lengths, in metres.
    pub fn snap(self) -> Snap {
        match self {
            Self::Free => Snap::NONE,
            Self::Fine => Snap::step(1.0),
            Self::Coarse => Snap::step(5.0),
        }
    }

    /// How far the brush that shapes the land reaches, in cells of the land.
    pub fn brush(self) -> f32 {
        match self {
            Self::Free => 4.0,
            Self::Fine => 3.0,
            Self::Coarse => 6.0,
        }
    }

    /// The steps of angles, in radians.
    pub fn turn(self) -> Snap {
        match self {
            Self::Free => Snap::NONE,
            Self::Fine => Snap::step(PI / 36.0),
            Self::Coarse => Snap::step(PI / 12.0),
        }
    }
}

/// What a design is: whether it can be driven, how long it is and what the road is like at its
/// control points. It is worked out once for a design, as the work is the same for everything
/// that asks.
#[derive(Clone, Debug)]
struct Report {
    /// Why the design cannot be driven, or `None` when it can.
    error: Option<TrackError>,
    /// The length of the track in metres, or zero when the design cannot be driven.
    length: f32,
    /// The road at every control point.
    frames: Vec<RoadFrame>,
    /// The baked road, when the design can be driven.
    track: Option<BakedTrack>,
}

impl Report {
    fn of(design: &TrackDesign) -> Self {
        let error = design.validate().err();
        let (track, frames) = if error.is_none() {
            (Some(design.bake()), design.control_frames())
        } else {
            (None, plain_frames(design))
        };
        Self {
            error,
            length: track.as_ref().map_or(0.0, BakedTrack::length),
            frames,
            track,
        }
    }

    /// `ground` with whatever covers the road cut away, so the road is never buried. A design
    /// that cannot be driven has no road yet and leaves the land as it is.
    fn cut(&self, ground: &Ground) -> Ground {
        match &self.track {
            Some(track) => ground.cut_along(track),
            None => ground.clone(),
        }
    }
}

/// The road at every control point of a design that cannot be driven, found from the control
/// points alone, since the loop through them may be too tangled to follow.
fn plain_frames(design: &TrackDesign) -> Vec<RoadFrame> {
    let points = &design.control_points;
    let count = points.len();
    (0..count)
        .map(|index| {
            let point = points[index];
            let before = points[(index + count - 1) % count].position;
            let after = points[(index + 1) % count].position;
            let tangent = (after - before).normalize_or(Vec3::Z);
            let level_right = tangent.cross(Vec3::Y).try_normalize().unwrap_or(Vec3::X);
            let level_up = level_right.cross(tangent).normalize_or(Vec3::Y);
            let (sin, cos) = point.roll.sin_cos();
            RoadFrame {
                position: point.position,
                tangent,
                up: level_up * cos + level_right * sin,
                right: level_right * cos - level_up * sin,
                width: design.widths.straight * point.width,
                roll: point.roll,
                curvature: 0.0,
            }
        })
        .collect()
}

/// Where the brush is while the land is being shaped, and what it is doing: a patch of the land
/// and how far the drag has raised it, which the view draws so the size and the height can be
/// seen before the press is let go.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cursor {
    /// Where the brush is on the ground plane.
    pub center: Vec2,
    /// How far the brush reaches.
    pub radius: f32,
    /// How far the drag has raised the land under the middle of the brush; negative when it
    /// has lowered it, and zero for the tool that cuts holes.
    pub rise: f32,
}

/// The points the map of `design` is framed around: its control points and the corners of a
/// square around their middle that holds a few of the smallest loops that can be driven, whose
/// turn radius is the design's own. A track of one point, of two, or of none still has a map
/// with a size, and a track that spans more than the square is framed as it is.
fn framing_points(design: &TrackDesign) -> Vec<Vec3> {
    let mut points: Vec<Vec3> = design
        .control_points
        .iter()
        .map(|point| point.position)
        .collect();
    let (low, high) = points.iter().fold(
        (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
        |(low, high), point| (low.min(*point), high.max(*point)),
    );
    let middle = if points.is_empty() {
        Vec3::ZERO
    } else {
        (low + high) / 2.0
    };
    let reach = design.min_turn_radius * ROOM_LOOPS;
    points.extend(
        [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
            .map(|(x, z)| Vec3::new(middle.x + x * reach, middle.y, middle.z + z * reach)),
    );
    points
}

/// The editor: the track being edited and everything the interface remembers.
#[derive(Clone)]
pub struct Editor {
    /// The track being edited.
    pub design: TrackDesign,
    /// The land the track lies on, which is not the road: it can be raised, lowered and cut
    /// through, and a kart that is not on the road drives on it or falls through it.
    pub ground: Ground,
    /// The brush, while the land is being shaped.
    pub cursor: Option<Cursor>,
    /// The control point that is selected.
    pub selected: Option<usize>,
    /// What the handles do.
    pub tool: Tool,
    /// How finely the handles step.
    pub precision: Precision,
    /// The ring of the radial menu that is open.
    pub menu: Option<Ring>,
    /// Whether the track is being driven.
    pub testing: bool,
    /// The size of the HUD and the projection of the camera, which the HUD lays itself out from.
    pub screen: Screen,
    report: Arc<Report>,
}

impl Editor {
    /// An editor of `design` with nothing selected and the select tool.
    pub fn new(design: TrackDesign) -> Self {
        let report = Report::of(&design);
        let (middle, half) = space_of(&framing_points(&design));
        let land = Ground::new(middle.xz(), half * LAND_REACH, CHUNKS, crate::ground::BASE);
        Self {
            ground: report.cut(&land),
            report: Arc::new(report),
            cursor: None,
            design,
            selected: None,
            tool: Tool::Select,
            precision: Precision::Fine,
            menu: None,
            testing: false,
            screen: Screen::default(),
        }
    }

    /// An editor of an empty track, which the first presses on the ground start to lay down.
    /// When `DSD_SCREENSHOT` is set, loads the upstream track instead so the screenshot shows
    /// a drivable track.
    pub fn initial() -> Self {
        let design = if std::env::var("DSD_SCREENSHOT").is_ok() {
            TrackDesign::upstream()
        } else {
            TrackDesign::empty()
        };
        Self::new(design)
    }

    /// An editor of the upstream track, for the tests that need a track to edit.
    #[cfg(test)]
    pub fn upstream() -> Self {
        Self::new(TrackDesign::upstream())
    }

    /// Why the track cannot be driven, or `None` when it can.
    pub fn error(&self) -> Option<TrackError> {
        self.report.error
    }

    /// The length of the track in metres; zero while it cannot be driven.
    pub fn length(&self) -> f32 {
        self.report.length
    }

    /// The road at every control point.
    pub fn frames(&self) -> &[RoadFrame] {
        &self.report.frames
    }

    /// The control point that is selected.
    pub fn point(&self) -> Option<(usize, &ControlPoint)> {
        let index = self.selected?;
        Some((index, self.design.control_points.get(index)?))
    }

    /// How long `hud` HUD units on the screen are in the world at `at`. Behind the camera
    /// nothing is seen, and a unit is as good as any other.
    fn world_size(&self, hud: f32, at: Vec3) -> f32 {
        hud * self.screen.world_per_hud(at).unwrap_or(1.0)
    }

    /// The middle of the control points, where the sizes of the handles are measured.
    pub(crate) fn middle(&self) -> Vec3 {
        let points = &self.design.control_points;
        if points.is_empty() {
            return Vec3::ZERO;
        }
        points.iter().map(|point| point.position).sum::<Vec3>() / points.len() as f32
    }

    /// The points the map is framed around, see [`framing_points`].
    pub fn framing_points(&self) -> Vec<Vec3> {
        framing_points(&self.design)
    }

    /// The space of the map: the middle of the ground it covers and the half-width of that square.
    pub fn space(&self) -> (Vec3, f32) {
        space_of(&self.framing_points())
    }

    /// The radius of the gems of the control points, which is the same on the screen wherever
    /// the camera looks and however far it is zoomed.
    pub fn handle_radius(&self) -> f32 {
        self.world_size(HANDLE_HUD_RADIUS, self.middle())
    }

    /// The size of the gizmo of the control points, which is the same on the screen too.
    pub fn gizmo_size(&self) -> f32 {
        self.world_size(GIZMO_HUD_SIZE, self.middle())
    }

    /// Where the handles of a control point float: a gem's radius above the road, so that they
    /// rest on it.
    pub fn handle_position(&self, index: usize) -> Vec3 {
        self.design.control_points[index].position + Vec3::Y * self.handle_radius()
    }

    /// This editor with `design` as the track. A selection that is no longer there is dropped,
    /// and the land grows to reach [`LAND_REACH`] times as far as the map of the track does, so a
    /// track that is pulled out over the grid always has land under it.
    #[must_use]
    pub fn with_design(&self, design: TrackDesign) -> Self {
        if design == self.design {
            return self.clone();
        }
        let count = design.control_points.len();
        let report = Report::of(&design);
        let (middle, half) = space_of(&framing_points(&design));
        let land = self.ground.grown_to(middle.xz(), half * LAND_REACH);
        Self {
            selected: self.selected.filter(|index| *index < count),
            ground: report.cut(&land),
            report: Arc::new(report),
            design,
            ..self.clone()
        }
    }

    /// This editor with `tool` chosen.
    #[must_use]
    pub fn with_tool(&self, tool: Tool) -> Self {
        Self {
            tool,
            ..self.clone()
        }
    }

    /// This editor with `precision` chosen.
    #[must_use]
    pub fn with_precision(&self, precision: Precision) -> Self {
        Self {
            precision,
            ..self.clone()
        }
    }

    /// This editor driving the track, or editing it again.
    #[must_use]
    pub fn with_testing(&self, testing: bool) -> Self {
        Self {
            testing,
            ..self.clone()
        }
    }

    /// This editor with control point `selected` selected.
    #[must_use]
    pub fn with_selected(&self, selected: Option<usize>) -> Self {
        Self {
            selected: selected.filter(|index| *index < self.design.control_points.len()),
            ..self.clone()
        }
    }

    /// This editor with control point `index` moved by the world `offset`, and selected.
    #[must_use]
    pub fn moved(&self, index: usize, offset: Vec3) -> Self {
        let Some(point) = self.design.control_points.get(index) else {
            return self.clone();
        };
        let at = self.within(point.position + offset);
        self.with_design(self.design.with_point_moved(index, at))
            .with_selected(Some(index))
    }

    /// `at` with its place on the ground plane brought onto the land, so a control point can
    /// never be dragged off the map, where the road would hang over nothing.
    pub fn within(&self, at: Vec3) -> Vec3 {
        let (middle, half) = (self.ground.middle(), self.ground.half());
        Vec3::new(
            at.x.clamp(middle.x - half, middle.x + half),
            at.y,
            at.z.clamp(middle.y - half, middle.y + half),
        )
    }

    /// The square that presses on the ground land on: the land, or the space of the map when a
    /// track that has grown reaches further. Its middle and half-width.
    pub fn terrain_square(&self) -> (Vec2, f32) {
        let (middle, half) = self.space();
        if half > self.ground.half() {
            (middle.xz(), half)
        } else {
            (self.ground.middle(), self.ground.half())
        }
    }

    /// How far the brush that shapes the land reaches: a few cells of the land, as many as the
    /// precision says, so the brush is as coarse as the land is.
    pub fn brush_radius(&self) -> f32 {
        self.ground.pitch() * self.precision.brush()
    }

    /// This editor with `ground` as the land.
    #[must_use]
    pub fn with_ground(&self, ground: Ground) -> Self {
        Self {
            ground,
            ..self.clone()
        }
    }

    /// This editor with the brush at `cursor`, or without one.
    #[must_use]
    pub fn with_cursor(&self, cursor: Option<Cursor>) -> Self {
        Self {
            cursor,
            ..self.clone()
        }
    }

    /// This editor with the land under the brush at `center` raised by `amount`, or lowered when
    /// that is negative. The brush is as wide as the tool and the precision make it, and the land
    /// never covers the road: where a hill would, it is cut down along the road.
    #[must_use]
    pub fn sculpted(&self, center: Vec2, amount: f32) -> Self {
        let brush = Brush {
            center,
            radius: self.brush_radius(),
            amount,
            falloff: BRUSH_FALLOFF,
        };
        self.with_ground(self.report.cut(&self.ground.raised(&brush)))
    }

    /// This editor with the land around `center` cut through, or filled in again when `hole` is
    /// false.
    #[must_use]
    pub fn holed(&self, center: Vec2, hole: bool) -> Self {
        self.with_ground(self.ground.with_holes(center, self.brush_radius(), hole))
    }

    /// This editor with control point `index` raised by `dy`, and selected.
    #[must_use]
    pub fn raised(&self, index: usize, dy: f32) -> Self {
        self.with_design(self.design.with_point_raised(index, dy))
            .with_selected(Some(index))
    }

    /// This editor with the road at control point `index` wider by `change` on each side, and
    /// the point selected. The width stays within the limits of the track.
    #[must_use]
    pub fn widened(&self, index: usize, change: f32) -> Self {
        let (Some(point), Some(frame)) = (
            self.design.control_points.get(index),
            self.frames().get(index),
        ) else {
            return self.clone();
        };
        let factor = (frame.width + 2.0 * change) / frame.width;
        self.with_design(self.design.with_point_width(index, point.width * factor))
            .with_selected(Some(index))
    }

    /// This editor with the road at control point `index` banked further by `change` radians,
    /// and the point selected. The banking stays within the limits of the track.
    #[must_use]
    pub fn rolled(&self, index: usize, change: f32) -> Self {
        let Some(point) = self.design.control_points.get(index) else {
            return self.clone();
        };
        self.with_design(self.design.with_point_roll(index, point.roll + change))
            .with_selected(Some(index))
    }

    /// This editor with a new control point between points `segment` and `segment + 1`, put
    /// `offset` away from the middle of the straight line between them, and selected.
    #[must_use]
    pub fn inserted(&self, segment: usize, offset: Vec3) -> Self {
        let points = &self.design.control_points;
        if segment >= points.len() {
            return self.clone();
        }
        let middle =
            (points[segment].position + points[(segment + 1) % points.len()].position) / 2.0;
        let inserted = self.design.with_point_inserted(segment);
        if inserted.control_points.len() == points.len() {
            return self.clone();
        }
        let at = self.within(middle + offset);
        self.with_design(inserted.with_point_moved(segment + 1, at))
            .with_selected(Some(segment + 1))
    }

    /// This editor with a new control point laid on the ground at `at`, after the last one, and
    /// selected.
    #[must_use]
    pub fn placed(&self, at: Vec3) -> Self {
        let at = self.within(at);
        let design = self.design.with_point_appended(Vec3::new(at.x, 0.0, at.z));
        let last = design.control_points.len().checked_sub(1);
        self.with_design(design).with_selected(last)
    }

    /// This editor without control point `index`, unless the loop would be left with too few
    /// points. Nothing is selected afterwards.
    #[must_use]
    pub fn removed(&self, index: usize) -> Self {
        Self {
            selected: None,
            ..self.with_design(self.design.with_point_removed(index))
        }
    }

    /// This editor with the banking of control point `index` taken away.
    #[must_use]
    pub fn leveled(&self, index: usize) -> Self {
        self.with_design(self.design.with_point_roll(index, 0.0))
    }

    /// This editor with control point `index` put on the ground.
    #[must_use]
    pub fn grounded(&self, index: usize) -> Self {
        let Some(point) = self.design.control_points.get(index) else {
            return self.clone();
        };
        self.with_design(
            self.design
                .with_point_moved(index, Vec3::new(point.position.x, 0.0, point.position.z)),
        )
    }

    /// This editor with the road at control point `index` banked by `steps` times five degrees.
    #[must_use]
    pub fn banked(&self, index: usize, steps: f32) -> Self {
        self.rolled(index, steps * BANK_STEP)
    }

    /// This editor with the road at control point `index` wider by `steps` tenths, or narrower
    /// when `steps` is negative.
    #[must_use]
    pub fn broadened(&self, index: usize, steps: i32) -> Self {
        let Some(point) = self.design.control_points.get(index) else {
            return self.clone();
        };
        self.with_design(
            self.design
                .with_point_width(index, point.width * WIDTH_STEP.powi(steps)),
        )
    }

    /// The point the menu pops up over: the selected control point.
    fn anchor(&self) -> Option<Vec3> {
        let (index, _) = self.point()?;
        Some(self.handle_position(index))
    }

    /// The first lines of the status: what is selected, and how the road is there.
    fn selection_lines(&self) -> String {
        match self.point() {
            None if self.design.control_points.is_empty() => format!(
                "Empty track - {}\nChoose Insert, then press the ground to lay points",
                self.tool.label()
            ),
            None => format!(
                "Track of {} points - {}",
                self.design.control_points.len(),
                self.tool.label()
            ),
            Some((index, point)) => {
                let width = self.frames().get(index).map_or(0.0, |frame| frame.width);
                format!(
                    "Point {} of {} - {}\nHeight {:.1} m  Roll {:.0} deg\nWidth x{:.2} ({width:.1} m)",
                    index + 1,
                    self.design.control_points.len(),
                    self.tool.label(),
                    point.position.y,
                    point.roll.to_degrees(),
                    point.width,
                )
            }
        }
    }
}

impl HudHost for Editor {
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

impl RadialHost for Editor {
    type Ring = Ring;

    fn root(&self) -> Ring {
        Ring::Root
    }

    fn menu(&self) -> Option<Ring> {
        self.menu
    }

    fn with_menu(&self, menu: Option<Ring>) -> Self {
        Self {
            menu,
            ..self.clone()
        }
    }

    fn pop_center(&self, layout: &Layout) -> Vec2 {
        let Some(at) = self.anchor().and_then(|anchor| self.screen.project(anchor)) else {
            return layout.home();
        };
        match self.point() {
            Some((index, _)) if self.is_chip() => {
                let avoid: Vec<Vec2> = crate::view::footprint(self, index)
                    .into_iter()
                    .filter_map(|spot| self.screen.project(spot))
                    .collect();
                layout.chip_spot(at, &avoid)
            }
            _ => layout.keep_inside(at),
        }
    }

    fn is_popped(&self) -> bool {
        self.anchor().is_some() && !self.testing
    }

    fn is_chip(&self) -> bool {
        self.is_popped() && self.menu.is_none() && self.tool.edits_in_place()
    }

    fn status(&self) -> String {
        if self.testing {
            return "Test drive\nW/S accelerate, A/D steer\nR restarts, Esc edits".into();
        }
        let last = match self.error() {
            Some(error) => format!("Cannot be driven: {error}"),
            None => format!("{:.0} m - {:?}", self.length(), self.precision),
        };
        if self.tool.shapes_land() {
            return format!("{}\n{}\n{last}", self.selection_lines(), self.tool.hint());
        }
        format!("{}\n{last}", self.selection_lines())
    }
}

impl GizmoReveal for Editor {
    fn reveals_gizmo(&self) -> bool {
        self.tool.reveals_gizmo() && !self.testing
    }
}

#[cfg(test)]
mod tests {
    use bevy_composable_dragging::radial::RadialHost;

    use super::*;
    use crate::ground::BASE;

    /// The upstream editor with the road lifted to `height` everywhere, so the land is clear of it.
    fn raised_road(height: f32) -> Editor {
        let mut design = TrackDesign::upstream();
        for point in &mut design.control_points {
            point.position.y = height;
        }
        Editor::new(design)
    }

    #[test]
    fn the_land_covers_twice_the_space_of_the_map_around_its_middle() {
        let editor = Editor::upstream();
        let (middle, half) = editor.space();
        assert!((editor.ground.half() - half * LAND_REACH).abs() < 1e-3);
        assert!(editor.ground.middle().distance(middle.xz()) < 1e-3);
        for point in &editor.design.control_points {
            assert!(editor.ground.contains(point.position.xz()));
        }
        assert_eq!(editor.cursor, None);
        let empty = Editor::initial();
        assert!(empty.ground.half() > 0.0);
        assert!(empty.ground.contains(Vec2::ZERO));
    }

    #[test]
    fn the_land_grows_with_a_track_that_is_pulled_out_and_keeps_its_hills() {
        let editor = raised_road(30.0).with_tool(Tool::Hills);
        let hill = editor.ground.middle() + Vec2::new(300.0, -200.0);
        let hilly = editor.sculpted(hill, 6.0);
        let reach = hilly.ground.half();
        let out = Vec3::new(reach * 0.9, 30.0, 0.0);
        let pulled = hilly.moved(0, out);
        assert!(pulled.ground.half() > reach, "the land grew");
        let (middle, half) = pulled.space();
        assert!(pulled.ground.half() >= half * LAND_REACH - 1e-3);
        for point in &pulled.design.control_points {
            assert!(pulled.ground.contains(point.position.xz()));
        }
        assert!(middle.xz().distance(pulled.ground.middle()) < pulled.ground.half());
        assert!(
            pulled.ground.surface(hill) > BASE + 3.0,
            "the hill is still there: {}",
            pulled.ground.surface(hill)
        );
        let (square, side) = pulled.terrain_square();
        assert!(side >= pulled.ground.half() - 1e-3 || square != pulled.ground.middle());
        let nudged = pulled.moved(0, Vec3::X);
        assert_eq!(
            nudged.ground.half(),
            pulled.ground.half(),
            "a small step does not grow the land again"
        );
    }

    #[test]
    fn the_brush_is_as_many_cells_wide_as_the_precision_says() {
        let editor = Editor::upstream();
        for precision in Precision::ALL {
            let chosen = editor.with_precision(precision);
            let cells = chosen.brush_radius() / chosen.ground.pitch();
            assert!((cells - precision.brush()).abs() < 1e-4, "{precision:?}");
        }
        assert!(Precision::Coarse.brush() > Precision::Fine.brush());
        let small = Editor::initial();
        assert!(
            small.brush_radius() < editor.brush_radius(),
            "a smaller land has a finer brush"
        );
    }

    #[test]
    fn sculpting_raises_and_lowers_the_land_and_leaves_the_road_and_the_rest_alone() {
        let editor = raised_road(30.0).with_tool(Tool::Hills);
        let at = editor.ground.middle() + Vec2::new(300.0, -200.0);
        let up = editor.sculpted(at, 6.0);
        assert!(up.ground.surface(at) > BASE + 5.0);
        assert_eq!(up.design, editor.design);
        assert_eq!(up.selected, editor.selected);
        let down = up.sculpted(at, -12.0);
        assert!(down.ground.surface(at) < BASE - 5.0);
        let far = editor.ground.middle() + Vec2::splat(editor.ground.half() * 0.9);
        assert_eq!(up.ground.surface(far), editor.ground.surface(far));
    }

    #[test]
    fn holes_are_cut_and_filled_without_touching_the_heights() {
        let editor = Editor::upstream().with_tool(Tool::Hole);
        let at = editor.ground.middle() + Vec2::new(300.0, -200.0);
        let holed = editor.holed(at, true);
        assert!(holed.ground.is_hole(at));
        assert_eq!(holed.ground.surface(at), editor.ground.surface(at));
        assert_eq!(holed.holed(at, false).ground, editor.ground);
        assert_eq!(holed.design, editor.design);
    }

    #[test]
    fn a_hill_raised_over_the_road_is_cut_down_along_it() {
        let editor = raised_road(20.0).with_tool(Tool::Hills);
        let frame = editor.design.bake().sample(0.0);
        let covered = editor.sculpted(frame.position.xz(), 400.0);
        assert!(covered.ground.surface(frame.position.xz()) < frame.position.y);
        for side in [-1.0, 1.0] {
            let edge = frame.edge(side);
            assert!(covered.ground.surface(edge.xz()) < edge.y, "{side}");
        }
        let aside = frame
            .surface_point(frame.width / 2.0 + covered.ground.pitch() * 3.0)
            .xz();
        assert!(
            covered.ground.surface(aside) > BASE + 1.0,
            "the hill stands beside the road"
        );
    }

    #[test]
    fn a_road_moved_into_a_hill_cuts_the_hill() {
        let editor = raised_road(20.0).with_tool(Tool::Hills);
        let at = editor.design.control_points[0].position.xz() + Vec2::new(200.0, 0.0);
        let hill = editor.sculpted(at, 400.0);
        assert!(hill.ground.surface(at) > BASE + 10.0);
        let moved = hill.moved(0, Vec3::new(200.0, 0.0, 0.0));
        let road = moved.design.control_points[0].position;
        assert!(
            moved.ground.surface(road.xz()) < road.y,
            "{}",
            moved.ground.surface(road.xz())
        );
        let untouched = hill.ground.surface(at + Vec2::new(0.0, 600.0));
        assert_eq!(moved.ground.surface(at + Vec2::new(0.0, 600.0)), untouched);
    }

    #[test]
    fn a_control_point_cannot_be_moved_inserted_or_placed_off_the_land() {
        let editor = Editor::upstream();
        let limit = editor.ground.middle() + Vec2::splat(editor.ground.half());
        let moved = editor.moved(0, Vec3::new(1.0e6, 0.0, -1.0e6));
        let position = moved.design.control_points[0].position;
        assert!((position.x - limit.x).abs() < 1e-2 && editor.ground.contains(position.xz()));
        assert!(position.z > editor.ground.middle().y - editor.ground.half() - 1e-2);
        let inserted = editor.inserted(0, Vec3::new(-1.0e6, 0.0, 1.0e6));
        assert!(
            editor
                .ground
                .contains(inserted.design.control_points[1].position.xz())
        );
        let placed = Editor::initial().placed(Vec3::new(1.0e6, 0.0, 1.0e6));
        assert!(
            placed
                .ground
                .contains(placed.design.control_points[0].position.xz())
        );
        assert_eq!(
            editor.within(Vec3::new(1.0, 7.0, 2.0)),
            Vec3::new(1.0, 7.0, 2.0)
        );
    }

    #[test]
    fn the_land_tools_say_what_they_do_in_the_status() {
        for tool in Tool::ALL {
            let status = Editor::upstream().with_tool(tool).status();
            assert_eq!(
                status.contains(tool.hint()),
                tool.shapes_land() || tool.hint().is_empty()
            );
            assert_eq!(tool.shapes_land(), !tool.hint().is_empty(), "{tool:?}");
        }
        assert!(Tool::Hills.shapes_land() && Tool::Hole.shapes_land());
        assert!(!Tool::Hills.edits_in_place() && !Tool::Hills.reveals_gizmo());
        assert!(!Tool::Hole.edits_in_place() && !Tool::Hole.reveals_gizmo());
    }
}
