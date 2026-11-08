//! The architect's drawing under the editor: what shows the structure and the elevation of the
//! level from any side, whether or not the track can be driven yet.
//!
//! - A grid on the ground, over the whole space of the map, with a heavier line every fifth.
//!   Its spacing is a round number picked from the size of the space, so the squares are a
//!   length that can be read off the map at any size of track.
//! - A stilt from the ground to every control point and to the middle of every segment, and a
//!   disc on the ground under each control point, which show how high the road is and where it
//!   stands.
//! - While the track cannot be driven, a translucent ribbon along the curve through the points,
//!   as wide as the road is there, so the shape that is being laid down can be seen before the
//!   world builds a road. The ribbon follows the heights and the widths; the banking is for
//!   the roll ring and the road.
//!
//! None of it takes a press, so the handles and the terrain keep every press that is theirs.

use bevy::prelude::*;
use bevy_composable_dragging::{View, ViewNode};
use bevy_gradmesh::{CatmullRomSpline, Shape, ShapeDetail, Style, SweepOptions};

use crate::state::Editor;
use crate::view::GROUND_HEIGHT;

/// How many lines of the grid cross the space at most along each axis.
const GRID_LINES: f32 = 40.0;
/// How often a line of the grid is a heavy one.
const MAJOR_EVERY: i32 = 5;
/// How thick a line of the grid is, as a fraction of the spacing of the lines.
const LINE_THICKNESS: f32 = 0.02;
/// How much thicker a heavy line is than a light one.
const MAJOR_FACTOR: f32 = 2.5;
/// How wide the stilt under a control point is, as a multiple of the radius of its gem.
const STILT_WIDTH: f32 = 0.35;
/// How wide the stilt under the middle of a segment is, as a multiple of the radius of a gem.
const MIDDLE_STILT_WIDTH: f32 = 0.2;
/// How wide the disc under a control point is, as a multiple of the radius of its gem.
const FOOT_RADIUS: f32 = 1.4;
/// How thick the disc under a control point is, as a fraction of its radius.
const FOOT_THICKNESS: f32 = 0.15;
/// How many points of the curve are drawn for every segment between control points.
const SAMPLES: usize = 16;
/// How thick the ribbon is, as a fraction of the width of the road.
const RIBBON_THICKNESS: f32 = 0.05;
/// How far apart two points of the ribbon have to be to have a direction.
const RIBBON_EPSILON: f32 = 1e-3;

const MINOR_LINE: Color = Color::srgb(0.28, 0.4, 0.3);
const MAJOR_LINE: Color = Color::srgb(0.55, 0.7, 0.6);
const STILT: Color = Color::srgb(0.7, 0.78, 0.9);
const FOOT: Color = Color::srgb(0.45, 0.6, 0.85);
const RIBBON: Color = Color::srgba(0.35, 0.55, 0.9, 0.55);

/// Adds the drawing of the editor's track to `view`.
pub fn push(view: &mut View<Editor>, editor: &Editor) {
    push_grid(view, editor);
    push_stilts(view, editor);
    if editor.error().is_some() {
        push_ribbon(view, editor);
    }
}

/// The first round number of the form 1, 2 or 5 times a power of ten that is at least `raw`.
fn round_step(raw: f32) -> f32 {
    let magnitude = 10f32.powf(raw.log10().floor());
    [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .map(|multiple| multiple * magnitude)
        .find(|step| *step >= raw)
        .unwrap_or(10.0 * magnitude)
}

/// The spacing of the lines of the grid over a space `half` wide on each side of its middle.
pub fn grid_step(half: f32) -> f32 {
    round_step(half * 2.0 / GRID_LINES)
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "the line numbers of a grid of at most a few dozen lines"
)]
fn push_grid(view: &mut View<Editor>, editor: &Editor) {
    let (middle, half) = editor.space();
    if half <= 0.0 || !half.is_finite() {
        return;
    }
    let step = grid_step(half);
    let thickness = step * LINE_THICKNESS;
    let lines = |around: f32| {
        let first = ((around - half) / step).ceil() as i32;
        let last = ((around + half) / step).floor() as i32;
        first..=last
    };
    let line = |id: String, k: i32, size: Vec3, at: Vec3| {
        let major = k % MAJOR_EVERY == 0;
        let grow = if major { MAJOR_FACTOR } else { 1.0 };
        let size = Vec3::new(
            size.x.max(thickness * grow),
            thickness * grow,
            size.z.max(thickness * grow),
        );
        ViewNode::cuboid(id, size, 0.0)
            .at(at + Vec3::Y * (GROUND_HEIGHT + size.y / 2.0))
            .with_color(if major { MAJOR_LINE } else { MINOR_LINE })
            .pickable(false)
            .casts_shadow(false)
    };
    for k in lines(middle.x) {
        let at = Vec3::new(k as f32 * step, 0.0, middle.z);
        view.push(line(
            format!("plan/x/{k}"),
            k,
            Vec3::new(0.0, 0.0, half * 2.0),
            at,
        ));
    }
    for k in lines(middle.z) {
        let at = Vec3::new(middle.x, 0.0, k as f32 * step);
        view.push(line(
            format!("plan/z/{k}"),
            k,
            Vec3::new(half * 2.0, 0.0, 0.0),
            at,
        ));
    }
}

/// The points of the curve through `points`: a closed loop of [`SAMPLES`] points for each
/// segment when there are three or more, and the straight segment between two. The point of
/// control point `i` is number `i * SAMPLES`.
fn curve(points: &[Vec3]) -> Vec<Vec3> {
    let count = points.len();
    if count < 3 {
        return points.to_vec();
    }
    let spline = CatmullRomSpline::closed(points.to_vec());
    let total = count * SAMPLES;
    (0..total)
        .map(|i| spline.point(i as f32 / total as f32))
        .collect()
}

/// A column from the ground to `top`, `width` across.
fn stilt(id: String, top: Vec3, width: f32, color: Color) -> ViewNode<Editor> {
    let low = GROUND_HEIGHT.min(top.y);
    let height = (top.y - GROUND_HEIGHT).abs().max(width * FOOT_THICKNESS);
    ViewNode::cuboid(id, Vec3::new(width, height, width), 0.0)
        .at(Vec3::new(top.x, low + height / 2.0, top.z))
        .with_color(color)
        .pickable(false)
        .casts_shadow(false)
}

fn push_stilts(view: &mut View<Editor>, editor: &Editor) {
    let points: Vec<Vec3> = editor
        .design
        .control_points
        .iter()
        .map(|point| point.position)
        .collect();
    if points.is_empty() {
        return;
    }
    let radius = editor.handle_radius();
    let path = curve(&points);
    let middles: Vec<Vec3> = if points.len() < 3 {
        points
            .windows(2)
            .map(|pair| (pair[0] + pair[1]) / 2.0)
            .collect()
    } else {
        (0..points.len())
            .map(|segment| path[segment * SAMPLES + SAMPLES / 2])
            .collect()
    };
    for (index, point) in points.iter().enumerate() {
        view.push(stilt(
            format!("plan/stilt-{index}"),
            *point,
            radius * STILT_WIDTH,
            STILT,
        ));
        view.push(
            ViewNode::cylinder(
                format!("plan/foot-{index}"),
                radius * FOOT_RADIUS,
                radius * FOOT_RADIUS * FOOT_THICKNESS,
            )
            .at(Vec3::new(
                point.x,
                GROUND_HEIGHT + radius * FOOT_RADIUS * FOOT_THICKNESS / 2.0,
                point.z,
            ))
            .with_color(FOOT)
            .pickable(false)
            .casts_shadow(false),
        );
    }
    for (index, middle) in middles.iter().enumerate() {
        view.push(stilt(
            format!("plan/middle-{index}"),
            *middle,
            radius * MIDDLE_STILT_WIDTH,
            STILT,
        ));
    }
}

/// The ribbon along the curve through the control points, or `None` while there are not two
/// that are apart.
pub fn ribbon(editor: &Editor) -> Option<Shape> {
    let design = &editor.design;
    let points: Vec<Vec3> = design.control_points.iter().map(|p| p.position).collect();
    if points.len() < 2 {
        return None;
    }
    let closed = points.len() >= 3;
    let path = curve(&points);
    let length = |closing: bool| {
        path.windows(2)
            .map(|pair| pair[0].distance(pair[1]))
            .chain(closing.then(|| path[path.len() - 1].distance(path[0])))
            .collect::<Vec<f32>>()
    };
    let steps = length(closed);
    let total: f32 = steps.iter().sum();
    if total <= RIBBON_EPSILON * path.len() as f32 {
        return None;
    }
    let widths: Vec<f32> = design.control_points.iter().map(|p| p.width).collect();
    let ratio_of = |sample: usize| steps[..sample].iter().sum::<f32>() / total;
    let mut keyframes: Vec<(f32, Vec2)> = if closed {
        (0..points.len())
            .map(|index| (ratio_of(index * SAMPLES), Vec2::new(widths[index], 1.0)))
            .collect()
    } else {
        vec![(0.0, Vec2::new(widths[0], 1.0))]
    };
    keyframes.push((1.0, Vec2::new(widths[usize::from(!closed)], 1.0)));
    keyframes.dedup_by(|later, earlier| later.0 <= earlier.0);
    let width = design.road_width(0.0);
    let thickness = width * RIBBON_THICKNESS;
    let profile = vec![
        Vec2::new(-width / 2.0, -thickness),
        Vec2::new(width / 2.0, -thickness),
        Vec2::new(width / 2.0, 0.0),
        Vec2::new(-width / 2.0, 0.0),
    ];
    Some(Shape::sweep(
        profile,
        path,
        SweepOptions::default()
            .with_closed(closed)
            .with_caps(!closed)
            .with_scale_along(keyframes)
            .with_detail(ShapeDetail::Low),
    ))
}

fn push_ribbon(view: &mut View<Editor>, editor: &Editor) {
    if let Some(shape) = ribbon(editor) {
        view.push(
            ViewNode::grad("plan/ribbon", shape)
                .with_style(Style::transparent(RIBBON))
                .pickable(false)
                .casts_shadow(false),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(editor: &Editor) -> Vec<String> {
        let mut view = View::new();
        push(&mut view, editor);
        view.nodes()
            .iter()
            .map(|node| node.id.to_string())
            .collect()
    }

    fn count(ids: &[String], prefix: &str) -> usize {
        ids.iter().filter(|id| id.starts_with(prefix)).count()
    }

    #[test]
    fn the_grid_steps_are_round_lengths_that_keep_the_lines_to_a_readable_few() {
        for half in [3.0, 17.0, 80.0, 420.0, 1000.0, 12_345.0] {
            let step = grid_step(half);
            let mantissa = step / 10f32.powf(step.log10().floor());
            assert!(
                [1.0, 2.0, 5.0, 10.0]
                    .iter()
                    .any(|m| (mantissa - m).abs() < 1e-3),
                "{half}: {step}"
            );
            let lines = half * 2.0 / step;
            assert!(lines <= GRID_LINES + 1e-3, "{half}: {lines} lines");
            assert!(
                lines > GRID_LINES / 2.5 - 1e-3,
                "{half}: only {lines} lines"
            );
        }
    }

    #[test]
    fn the_grid_covers_the_space_even_for_an_empty_track() {
        for editor in [Editor::initial(), Editor::upstream()] {
            let ids = names(&editor);
            let (_, half) = editor.space();
            let step = grid_step(half);
            let expected = (half * 2.0 / step).floor() as usize;
            assert!(
                count(&ids, "plan/x/") >= expected,
                "{}",
                count(&ids, "plan/x/")
            );
            assert!(count(&ids, "plan/z/") >= expected);
            assert!(count(&ids, "plan/x/") <= GRID_LINES as usize + 1);
        }
    }

    #[test]
    fn every_control_point_stands_on_a_stilt_and_a_foot_and_every_segment_has_a_middle() {
        let editor = Editor::upstream();
        let points = editor.design.control_points.len();
        let ids = names(&editor);
        assert_eq!(count(&ids, "plan/stilt-"), points);
        assert_eq!(count(&ids, "plan/foot-"), points);
        assert_eq!(count(&ids, "plan/middle-"), points);
        assert_eq!(count(&names(&Editor::initial()), "plan/stilt-"), 0);
    }

    #[test]
    fn a_stilt_reaches_from_the_ground_to_its_point_whatever_the_height() {
        for y in [-0.5, 0.0, 3.0, 40.0] {
            let top = Vec3::new(2.0, y, -5.0);
            let node = stilt("s".into(), top, 0.5, STILT);
            let shape = match &node.geometry {
                bevy_composable_dragging::Geometry::Grad(shape) => shape.clone(),
                other => panic!("{other:?}"),
            };
            let bounds = shape.bounds();
            let (low, high) = (
                node.transform.translation.y + bounds.min.y * node.transform.scale.y,
                node.transform.translation.y + bounds.max.y * node.transform.scale.y,
            );
            assert!(low <= GROUND_HEIGHT.min(y) + 1e-3, "{y}: {low}");
            assert!(high >= y.max(GROUND_HEIGHT) - 1e-3, "{y}: {high}");
        }
    }

    #[test]
    fn the_ribbon_is_only_drawn_while_the_track_cannot_be_driven() {
        let drivable = Editor::upstream();
        assert!(drivable.error().is_none());
        assert_eq!(count(&names(&drivable), "plan/ribbon"), 0);

        let one = Editor::initial().placed(Vec3::new(10.0, 0.0, 10.0));
        let two = one.placed(Vec3::new(60.0, 0.0, 10.0));
        assert_eq!(
            count(&names(&one), "plan/ribbon"),
            0,
            "one point has no direction"
        );
        assert_eq!(
            count(&names(&two), "plan/ribbon"),
            1,
            "two points make a segment"
        );
        assert!(two.error().is_some());
        let three = two.placed(Vec3::new(30.0, 0.0, 12.0));
        assert!(
            three.error().is_some(),
            "three points in a line are no loop"
        );
        assert_eq!(count(&names(&three), "plan/ribbon"), 1);
    }

    #[test]
    fn the_ribbon_follows_the_heights_of_the_points() {
        let editor = Editor::initial()
            .placed(Vec3::new(0.0, 0.0, 0.0))
            .placed(Vec3::new(80.0, 0.0, 0.0))
            .placed(Vec3::new(40.0, 0.0, 60.0));
        let low = ribbon(&editor).expect("a ribbon").bounds();
        let high = ribbon(&editor.raised(2, 12.0)).expect("a ribbon").bounds();
        assert!(
            high.max.y > low.max.y + 5.0,
            "{} against {}",
            high.max.y,
            low.max.y
        );
    }

    #[test]
    fn two_points_on_the_same_spot_have_no_ribbon_and_no_nan() {
        let editor = Editor::initial()
            .placed(Vec3::new(5.0, 0.0, 5.0))
            .placed(Vec3::new(5.0, 0.0, 5.0));
        assert!(ribbon(&editor).is_none());
        for node in {
            let mut view = View::new();
            push(&mut view, &editor);
            view.nodes().to_vec()
        } {
            assert!(node.transform.translation.is_finite(), "{}", node.id);
        }
    }
}
