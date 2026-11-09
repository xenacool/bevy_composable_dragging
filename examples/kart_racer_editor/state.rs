//! The state of the editor: the track design, the land, what is selected, the tool and the edits.
//!
//! The state is a plain value. Every edit takes the state and returns the next one, through the
//! editing operations of the `TrackDesign`, so the limits of the track apply to every edit.

use bevy::prelude::*;
use bevy_composable_dragging::hud::{HudHost, Layout, Screen};
use bevy_composable_dragging::ops::Snap;
use bevy_composable_dragging::radial::RadialHost;

use crate::ground::{Brush, CHUNKS, Ground};
use crate::menu::Ring;
use crate::track::{BakedTrack, ControlPoint, RoadFrame, TrackDesign};

/// The radius of the gem of a control point on the screen, in HUD units.
pub const HANDLE_HUD_RADIUS: f32 = 9.0;
/// How many times the half-width of the map the land reaches, so there is land around the map.
const LAND_REACH: f32 = 2.0;
/// How much room the map has around the track it starts with, as a factor of the track's reach.
const MAP_ROOM: f32 = 1.12;
/// The part of the radius of the brush, from the edge inwards, over which it fades out.
const BRUSH_FALLOFF: f32 = 0.9;
/// How far the brush that shapes the land reaches, in cells of the land.
const BRUSH_CELLS: f32 = 8.0;
/// The steps of lengths that the handles and the brush take, in metres.
const STEP: f32 = 1.0;

/// The steps of lengths that the handles and the brush take.
pub fn snap() -> Snap {
    Snap::step(STEP)
}

/// What the handles and the pointer do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    /// The gems move their control points, which pulls the curve through them.
    Move,
    /// The handle at the edge of the road widens it.
    Width,
    /// Dragging the land up or down raises or lowers it, in a round patch.
    Hills,
}

impl Tool {
    /// Every tool, in the order of the ring.
    pub const ALL: [Self; 3] = [Self::Move, Self::Width, Self::Hills];

    /// The name of the tool.
    pub fn label(self) -> &'static str {
        match self {
            Self::Move => "Move",
            Self::Width => "Width",
            Self::Hills => "Hills",
        }
    }

    /// Whether the tool edits the selected control point where it is, so the menu keeps clear
    /// of it.
    pub fn edits_in_place(self) -> bool {
        matches!(self, Self::Move | Self::Width)
    }

    /// What to do with the tool, in a line.
    pub fn hint(self) -> &'static str {
        match self {
            Self::Move => "Drag a gem to pull the curve",
            Self::Width => "Drag the handle at the edge of the road",
            Self::Hills => "Drag the land up or down",
        }
    }
}

/// What a design is like: the road at every control point and the baked road.
#[derive(Clone, Debug)]
struct Report {
    /// The road at every control point.
    frames: Vec<RoadFrame>,
    /// The baked road.
    track: BakedTrack,
}

impl Report {
    fn of(design: &TrackDesign) -> Self {
        Self {
            frames: design.control_frames(),
            track: design.bake(),
        }
    }
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
    /// has lowered it.
    pub rise: f32,
}

/// The editor: the track being edited and everything the interface remembers.
#[derive(Clone)]
pub struct Editor {
    /// The track being edited.
    pub design: TrackDesign,
    /// The land the track lies on, which is not the road: it can be raised and lowered, and the
    /// road is never covered by it.
    pub ground: Ground,
    /// The brush, while the land is being shaped.
    pub cursor: Option<Cursor>,
    /// The control point that is selected.
    pub selected: Option<usize>,
    /// What the handles do.
    pub tool: Tool,
    /// The ring of the radial menu that is open.
    pub menu: Option<Ring>,
    /// The size of the HUD and the projection of the camera, which the HUD lays itself out from.
    pub screen: Screen,
    report: Box<Report>,
}

impl Editor {
    /// An editor of `design` with nothing selected and the move tool. The map is the square
    /// around the design that reaches [`MAP_ROOM`] times as far as the design does.
    pub fn new(design: TrackDesign) -> Self {
        let report = Report::of(&design);
        let points: Vec<Vec2> = design
            .control_points
            .iter()
            .map(|point| point.position.xz())
            .collect();
        let low = points
            .iter()
            .fold(Vec2::splat(f32::MAX), |low, p| low.min(*p));
        let high = points
            .iter()
            .fold(Vec2::splat(f32::MIN), |high, p| high.max(*p));
        let middle = (low + high) / 2.0;
        let reach = (high - low).max_element() / 2.0;
        let land = Ground::new(
            middle,
            reach * MAP_ROOM * LAND_REACH,
            CHUNKS,
            crate::ground::BASE,
        );
        Self {
            ground: land.cut_along(&report.track),
            report: Box::new(report),
            cursor: None,
            design,
            selected: None,
            tool: Tool::Move,
            menu: None,
            screen: Screen::default(),
        }
    }

    /// The editor of the track it starts with.
    pub fn initial() -> Self {
        Self::new(TrackDesign::initial())
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

    /// The middle of the control points, where the sizes of the handles are measured.
    pub(crate) fn middle(&self) -> Vec3 {
        let points = &self.design.control_points;
        points.iter().map(|point| point.position).sum::<Vec3>() / points.len().max(1) as f32
    }

    /// The map: the square of ground that the camera frames and the control points stay in,
    /// as its middle and half-width.
    pub fn space(&self) -> (Vec3, f32) {
        let middle = self.ground.middle();
        (
            Vec3::new(middle.x, 0.0, middle.y),
            self.ground.half() / LAND_REACH,
        )
    }

    /// The square that presses on the ground land on: the land. Its middle and half-width.
    pub fn terrain_square(&self) -> (Vec2, f32) {
        (self.ground.middle(), self.ground.half())
    }

    /// What the camera frames: the control points and the corners of the map.
    pub fn framing_points(&self) -> Vec<Vec3> {
        let (middle, half) = self.space();
        let mut points: Vec<Vec3> = self
            .design
            .control_points
            .iter()
            .map(|point| point.position)
            .collect();
        points.extend(
            [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
                .map(|(x, z)| middle + Vec3::new(x * half, 0.0, z * half)),
        );
        points
    }

    /// The radius of the gems of the control points, which is the same on the screen wherever
    /// the camera looks.
    pub fn handle_radius(&self) -> f32 {
        HANDLE_HUD_RADIUS * self.screen.world_per_hud(self.middle()).unwrap_or(1.0)
    }

    /// Where the handles of a control point float: a gem's radius above the road, so that they
    /// rest on it.
    pub fn handle_position(&self, index: usize) -> Vec3 {
        self.design.control_points[index].position + Vec3::Y * self.handle_radius()
    }

    /// This editor with `design` as the track. A selection that is no longer there is dropped,
    /// and the land is cut down along the new road, so the road is never buried.
    #[must_use]
    pub fn with_design(&self, design: TrackDesign) -> Self {
        if design == self.design {
            return self.clone();
        }
        let report = Report::of(&design);
        Self {
            selected: self
                .selected
                .filter(|index| *index < design.control_points.len()),
            ground: self.ground.cut_along(&report.track),
            report: Box::new(report),
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

    /// This editor with control point `selected` selected.
    #[must_use]
    pub fn with_selected(&self, selected: Option<usize>) -> Self {
        Self {
            selected: selected.filter(|index| *index < self.design.control_points.len()),
            ..self.clone()
        }
    }

    /// This editor with control point `index` moved by the world `offset`, and selected. The
    /// point stays on the map.
    #[must_use]
    pub fn moved(&self, index: usize, offset: Vec3) -> Self {
        let Some(point) = self.design.control_points.get(index) else {
            return self.clone();
        };
        let at = self.within(point.position + offset);
        self.with_design(self.design.with_point_moved(index, at))
            .with_selected(Some(index))
    }

    /// `at` with its place on the ground plane brought onto the map, so a control point can
    /// never be dragged off the screen.
    pub fn within(&self, at: Vec3) -> Vec3 {
        let (middle, half) = self.space();
        Vec3::new(
            at.x.clamp(middle.x - half, middle.x + half),
            at.y,
            at.z.clamp(middle.z - half, middle.z + half),
        )
    }

    /// How far the brush that shapes the land reaches: a few cells of the land, so the brush is
    /// as coarse as the land is.
    pub fn brush_radius(&self) -> f32 {
        self.ground.pitch() * BRUSH_CELLS
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
    /// that is negative. The land never covers the road: where a hill would, it is cut down
    /// along the road.
    #[must_use]
    pub fn sculpted(&self, center: Vec2, amount: f32) -> Self {
        let brush = Brush {
            center,
            radius: self.brush_radius(),
            amount,
            falloff: BRUSH_FALLOFF,
        };
        Self {
            ground: self.ground.raised(&brush).cut_along(&self.report.track),
            ..self.clone()
        }
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

    /// The point the menu pops up over: the selected control point.
    fn anchor(&self) -> Option<Vec3> {
        let (index, _) = self.point()?;
        Some(self.handle_position(index))
    }

    /// The first lines of the status: what is selected, and how the road is there.
    fn selection_lines(&self) -> String {
        match self.point() {
            None => format!(
                "Track of {} points - {}",
                self.design.control_points.len(),
                self.tool.label()
            ),
            Some((index, point)) => {
                let width = self.frames().get(index).map_or(0.0, |frame| frame.width);
                format!(
                    "Point {} of {} - {}\nHeight {:.1} m\nWidth x{:.2} ({width:.1} m)",
                    index + 1,
                    self.design.control_points.len(),
                    self.tool.label(),
                    point.position.y,
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
        self.anchor().is_some()
    }

    fn is_chip(&self) -> bool {
        self.is_popped() && self.menu.is_none() && self.tool.edits_in_place()
    }

    fn status(&self) -> String {
        format!("{}\n{}", self.selection_lines(), self.tool.hint())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ground::BASE;

    /// The initial editor with the road lifted to `height` everywhere, so the land is clear of it.
    fn raised_road(height: f32) -> Editor {
        let mut design = TrackDesign::initial();
        for point in &mut design.control_points {
            point.position.y = height;
        }
        Editor::new(design)
    }

    #[test]
    fn the_land_covers_twice_the_map_around_its_middle_and_the_track_is_on_the_map() {
        let editor = Editor::initial();
        let (middle, half) = editor.space();
        assert!((editor.ground.half() - half * LAND_REACH).abs() < 1e-3);
        for point in &editor.design.control_points {
            assert!((point.position.x - middle.x).abs() < half);
            assert!((point.position.z - middle.z).abs() < half);
        }
        assert_eq!(editor.cursor, None);
    }

    #[test]
    fn sculpting_raises_and_lowers_the_land_and_leaves_the_road_and_the_rest_alone() {
        let editor = raised_road(30.0).with_tool(Tool::Hills);
        let at = editor.ground.middle() + Vec2::new(250.0, -200.0);
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
    fn a_hill_raised_over_the_road_is_cut_down_along_it() {
        let editor = raised_road(20.0).with_tool(Tool::Hills);
        let frame = editor.design.bake().frames()[0];
        let covered = editor.sculpted(frame.position.xz(), 400.0);
        assert!(covered.ground.surface(frame.position.xz()) < frame.position.y);
        for side in [-1.0, 1.0] {
            let edge = frame.edge(side);
            assert!(covered.ground.surface(edge.xz()) < edge.y, "{side}");
        }
    }

    #[test]
    fn a_road_moved_into_a_hill_cuts_the_hill() {
        let editor = raised_road(20.0).with_tool(Tool::Hills);
        let at = editor.design.control_points[0].position.xz() + Vec2::new(-40.0, 0.0);
        let hill = editor.sculpted(at, 400.0);
        assert!(hill.ground.surface(at) > BASE + 10.0);
        let moved = hill.moved(0, Vec3::new(-40.0, 0.0, 0.0));
        let road = moved.design.control_points[0].position;
        assert!(moved.ground.surface(road.xz()) < road.y);
    }

    #[test]
    fn a_control_point_cannot_be_moved_off_the_map() {
        let editor = Editor::initial();
        let (middle, half) = editor.space();
        let moved = editor.moved(0, Vec3::new(1.0e6, 0.0, -1.0e6));
        let position = moved.design.control_points[0].position;
        assert!((position.x - (middle.x + half)).abs() < 1e-2);
        assert!((position.z - (middle.z - half)).abs() < 1e-2);
        assert_eq!(moved.selected, Some(0));
        assert_eq!(
            editor.within(Vec3::new(1.0, 7.0, 2.0)),
            Vec3::new(1.0, 7.0, 2.0)
        );
    }

    #[test]
    fn widening_a_point_widens_the_road_there_within_the_limits() {
        let editor = Editor::initial();
        let before = editor.frames()[3].width;
        let wider = editor.widened(3, 2.0);
        assert!((wider.frames()[3].width - before - 4.0).abs() < 0.5);
        assert_eq!(wider.selected, Some(3));
        let huge = editor.widened(3, 500.0);
        assert_eq!(
            huge.design.control_points[3].width,
            crate::track::MAX_POINT_WIDTH
        );
    }

    #[test]
    fn every_tool_says_what_it_does_in_the_status() {
        for tool in Tool::ALL {
            let status = Editor::initial().with_tool(tool).status();
            assert!(status.contains(tool.hint()), "{tool:?}");
            assert!(status.contains(tool.label()), "{tool:?}");
        }
    }
}
