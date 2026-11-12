//! The view of the editor: the handles of the track, the brush and the menu.
//!
//! The road is a node of the view, swept along the design that the view is of, so it bends and
//! widens while a handle is held, and the view draws the architect's grid and stilts. The view
//! holds what is edited with the pointer: a node named `track` that the handles of the curve
//! hang from, and the terrain, which takes the presses that no handle does. The tool decides
//! which handles are there.

use bevy::prelude::*;
use bevy_composable_dragging::handles::{CurveHandles, Lateral};
use bevy_composable_dragging::hud::{Layout, Theme};
use bevy_composable_dragging::ops::{Snap, custom, state};
use bevy_composable_dragging::radial::{RadialHost, push_menu};
use bevy_composable_dragging::{DragSpace, DragSpec, Frame, Pointer, View, ViewNode};

use crate::blueprint;
use crate::common::HUD_LAYER;
use crate::ground::BASE;
use crate::state::{Cursor, Editor, Tool, snap};

/// The id of the node that is the road.
pub const ROAD: &str = "road";
/// The id of the node that the handles of the curve hang from.
pub const TRACK: &str = "track";
/// The id of the node that is the terrain.
pub const TERRAIN: &str = "terrain";
/// The id of the node that shows the brush.
pub const CURSOR: &str = "cursor";
/// The height of flat ground, where the kart world draws it.
pub const GROUND_HEIGHT: f32 = BASE;
/// How thick the terrain is, as a fraction of its width.
const TERRAIN_THICKNESS: f32 = 0.01;
/// How thin the column of the brush is at least, as a fraction of its radius, so a brush that
/// has not moved the land yet can still be seen.
const CURSOR_THIN: f32 = 0.1;
const ROAD_COLOR: Color = Color::srgb(0.5, 0.5, 0.53);
const CURSOR_COLOR: Color = Color::srgba(0.45, 0.75, 1.0, 0.45);

/// The look of the HUD: the library theme, drawn on the layer of the HUD camera.
pub fn theme() -> Theme {
    Theme::new(HUD_LAYER)
}

/// A node that is there to be the parent of handles, and is not seen or hit.
fn anchor(id: &str, at: Vec3) -> ViewNode<Editor> {
    ViewNode::group(id.to_owned()).at(at)
}

/// Where a sculpting drag took hold: the point of the land that was pressed, and the vertical
/// line through it that the pointer is measured along.
struct Grip {
    line: DragSpace,
    center: Vec2,
    start: f32,
}

/// What a press on the land does with the Hills tool: it takes hold of the point of the land
/// under the pointer, and dragging up raises the land around it while dragging down lowers it,
/// in steps of a metre. The column of the brush shows how far, and the land takes the
/// change when the press is let go. A press that is not on the land does nothing.
fn sculpt(editor: &Editor) -> DragSpec<Editor> {
    let editor = editor.clone();
    custom(move |context| {
        let context = context.clone();
        let editor = editor.clone();
        let mut grip: Option<Option<Grip>> = None;
        Box::new(move |pointer: Pointer| {
            let grip = grip.get_or_insert_with(|| {
                let hit = editor.ground.hit(pointer.ray)?;
                let line = DragSpace::axis(Vec3::new(hit.x, 0.0, hit.z), Dir3::Y);
                let start = line.resolve(pointer.ray).map_or(hit.y, |point| point.y);
                Some(Grip {
                    line,
                    center: hit.xz(),
                    start,
                })
            });
            let Some(grip) = grip else {
                return Frame::new(context.render(&editor), editor.clone(), 0.0);
            };
            let raw = grip
                .line
                .resolve(pointer.ray)
                .map_or(0.0, |point| point.y - grip.start);
            let (shown, committed) = snap().apply(raw);
            let cursor = Cursor {
                center: grip.center,
                radius: editor.brush_radius(),
                rise: shown,
            };
            Frame::new(
                context.render(
                    &editor
                        .sculpted(grip.center, shown)
                        .with_cursor(Some(cursor)),
                ),
                editor.sculpted(grip.center, committed),
                0.0,
            )
        })
    })
}

/// The column that shows the brush: from the land up to how far the drag has raised it, or down
/// to how far it has lowered it, as wide as the brush.
fn cursor(editor: &Editor) -> Option<ViewNode<Editor>> {
    let cursor = editor.cursor?;
    let surface = editor.ground.surface(cursor.center);
    let low = surface.min(surface + cursor.rise);
    let high = surface
        .max(surface + cursor.rise)
        .max(low + cursor.radius * CURSOR_THIN);
    Some(
        ViewNode::cylinder(CURSOR, 1.0, 1.0)
            .at(Vec3::new(
                cursor.center.x,
                f32::midpoint(low, high),
                cursor.center.y,
            ))
            .scaled(Vec3::new(cursor.radius, high - low, cursor.radius))
            .with_color(CURSOR_COLOR)
            .pickable(false)
            .casts_shadow(false),
    )
}

/// The terrain: a slab under the whole map that is not seen. A press that no handle takes lands
/// on it. It selects the terrain, which clears the selection and closes the menu, or with the
/// Hills tool it shapes the land.
fn terrain(editor: &Editor) -> ViewNode<Editor> {
    let (middle, half) = editor.terrain_square();
    let width = half * 2.0;
    let thickness = width * TERRAIN_THICKNESS;
    let cleared = editor.with_selected(None).with_menu(None);
    let tool = editor.tool;
    ViewNode::cuboid(TERRAIN, Vec3::new(width, thickness, width), 0.0)
        .at(Vec3::new(
            middle.x,
            GROUND_HEIGHT - thickness / 2.0,
            middle.y,
        ))
        .with_opacity(0.0)
        .casts_shadow(false)
        .draggable_by_in(
            DragSpace::plane(Vec3::Y * GROUND_HEIGHT, Dir3::Y),
            move |editor| match tool {
                Tool::Hills => sculpt(editor),
                Tool::Move | Tool::Width => state(cleared.clone()),
            },
        )
}

/// The handles the chosen tool shows for the control points of the track.
fn curve_handles(editor: &Editor) -> CurveHandles<Editor> {
    let tool = editor.tool;
    let points: Vec<Vec3> = (0..editor.design.control_points.len())
        .map(|index| editor.handle_position(index))
        .collect();
    let lateral: Vec<Lateral> = editor
        .frames()
        .iter()
        .map(|frame| Lateral {
            right: frame.right,
            half_width: frame.width / 2.0,
        })
        .collect();
    let mut handles = CurveHandles::new(TRACK, Transform::IDENTITY, &points)
        .closed(true)
        .selected(editor.selected)
        .with_radius(editor.handle_radius())
        .move_point(
            if tool == Tool::Move {
                snap()
            } else {
                Snap::NONE
            },
            move |editor: &Editor, index, offset| match tool {
                Tool::Move => editor.moved(index, offset),
                Tool::Width | Tool::Hills => editor.with_selected(Some(index)),
            },
        );
    if let Some(eye) = eye(editor) {
        handles = handles.seen_from(eye);
    }
    if tool == Tool::Width {
        handles = handles.width(&lateral, snap(), |editor: &Editor, index, change| {
            editor.widened(index, change)
        });
    }
    handles
}

/// Where the camera sees the selected control point from, or the middle of the track when
/// nothing is selected. The handles that are seen edge-on or end-on from there are left out.
fn eye(editor: &Editor) -> Option<Vec3> {
    let at = editor
        .selected
        .filter(|index| *index < editor.design.control_points.len())
        .map_or_else(|| editor.middle(), |index| editor.handle_position(index));
    editor.screen.eye_for(at)
}

/// The points of the world that the handles of the chosen tool occupy around control point
/// `index`: the gem and the width handle. The chip of the menu keeps clear of them.
pub fn footprint(editor: &Editor, index: usize) -> Vec<Vec3> {
    curve_handles(editor).footprint(index)
}

/// The road of the design: a strip swept along the curve through the control points.
fn road(editor: &Editor) -> ViewNode<Editor> {
    ViewNode::grad(ROAD, editor.design.road_shape())
        .with_color(ROAD_COLOR)
        .pickable(false)
        .casts_shadow(false)
}

/// The view of the editor: the terrain, the road, the drawing, the handles of the track and the menu.
pub fn view(editor: &Editor) -> View<Editor> {
    let mut view = View::new();
    view.push(terrain(editor));
    view.push(road(editor));
    if let Some(brush) = cursor(editor) {
        view.push(brush);
    }
    blueprint::push(&mut view, editor);
    view.push(anchor(TRACK, Vec3::ZERO));
    curve_handles(editor).add_to(&mut view);
    let theme = theme();
    let layout = Layout::new(&editor.screen, theme);
    push_menu(&mut view, editor, &layout, &theme);
    view
}

#[cfg(test)]
mod tests {
    use bevy_composable_dragging::{DragContext, Pointer};

    use super::*;

    /// A control point whose width handle does not face the default eye of the tests end-on, which
    /// would leave it out.
    const WIDTH_POINT: usize = 1;

    fn with_tool(tool: Tool, selected: Option<usize>) -> Editor {
        Editor::initial().with_tool(tool).with_selected(selected)
    }

    fn ids(editor: &Editor) -> Vec<String> {
        view(editor)
            .nodes()
            .iter()
            .map(|node| node.id.to_string())
            .filter(|id| !id.starts_with("menu/"))
            .collect()
    }

    fn has(ids: &[String], prefix: &str) -> bool {
        ids.iter().any(|id| id.starts_with(prefix))
    }

    /// Drags `node` with its spec from where it is to `to(there)`, and returns the editor the
    /// drag drops.
    fn drag(editor: &Editor, node: &str, to: impl Fn(Vec3) -> Vec3) -> Editor {
        let view = view(editor);
        let node_ref = view.get(node).unwrap();
        let spec = node_ref.drag_spec(editor).unwrap();
        let anchor = view.world_position(node).unwrap();
        let context = DragContext::new(self::view, node.to_owned().into(), editor.clone())
            .with_space(node_ref.drag_space.unwrap());
        spec.compile(&context)(Pointer::at(to(anchor))).drop_state
    }

    #[test]
    fn nothing_the_pointer_edits_is_hidden_by_a_parent_that_is_not_seen() {
        for tool in Tool::ALL {
            let editor = with_tool(tool, Some(3));
            let view = view(&editor);
            for node in view.nodes() {
                if node.id.as_ref() == TERRAIN || node.id.starts_with("menu/") {
                    continue;
                }
                let mut chain = vec![node];
                while let Some(parent) = chain.last().and_then(|node| node.parent.as_ref()) {
                    chain.push(view.get(parent).unwrap());
                }
                for part in chain {
                    assert!(
                        part.opacity > 0.0,
                        "{tool:?}: {} is hidden by {}",
                        node.id,
                        part.id
                    );
                }
            }
        }
    }

    #[test]
    fn the_road_is_swept_along_the_design_it_is_the_view_of_and_takes_no_presses() {
        let editor = with_tool(Tool::Move, None);
        let height = |editor: &Editor| match &view(editor).get(ROAD).unwrap().geometry {
            bevy_composable_dragging::Geometry::Grad(shape) => shape.bounds(),
            other => panic!("{other:?}"),
        };
        assert!(!view(&editor).get(ROAD).unwrap().pickable);
        let wider = editor.widened(2, 6.0);
        let (before, after) = (height(&editor), height(&wider));
        assert_ne!(before.min, after.min, "a wider road reaches further out");
        let pulled = editor.moved(2, Vec3::new(40.0, 0.0, 40.0));
        assert_ne!(
            height(&pulled).max,
            before.max,
            "a pulled curve reaches further out"
        );
    }

    #[test]
    fn the_terrain_covers_the_land_and_takes_the_presses_of_every_tool() {
        for tool in Tool::ALL {
            let editor = with_tool(tool, None);
            let view = view(&editor);
            let terrain = view.get(TERRAIN).unwrap();
            assert!(terrain.drag.is_some(), "{tool:?}");
            let (middle, half) = editor.terrain_square();
            let at = view.world_position(TERRAIN).unwrap();
            assert!((at.x - middle.x).abs() < 1e-3 && (at.z - middle.y).abs() < 1e-3);
            assert!(half >= editor.ground.half() - 1e-3);
        }
    }

    #[test]
    fn the_brush_is_a_column_from_the_land_to_how_far_the_drag_has_raised_it() {
        let editor = with_tool(Tool::Hills, None);
        let center = editor.ground.middle() + Vec2::new(100.0, 50.0);
        let surface = editor.ground.surface(center);
        let radius = editor.brush_radius();
        for rise in [8.0, -8.0, 0.0] {
            let shown = editor.with_cursor(Some(Cursor {
                center,
                radius,
                rise,
            }));
            let view = view(&shown);
            let node = view.get(CURSOR).unwrap();
            assert!(!node.pickable, "the brush takes no presses");
            let at = view.world_position(CURSOR).unwrap();
            assert!((at.x - center.x).abs() < 1e-3 && (at.z - center.y).abs() < 1e-3);
            let height = node.transform.scale.y;
            assert!((node.transform.scale.x - radius).abs() < 1e-3);
            let (low, high) = (at.y - height / 2.0, at.y + height / 2.0);
            assert!(height > 0.0, "{rise}: it can always be seen");
            if rise > 0.0 {
                assert!((low - surface).abs() < 1e-3 && (high - surface - rise).abs() < 1e-3);
            } else if rise < 0.0 {
                assert!((high - surface).abs() < 1e-3 && (low - surface - rise).abs() < 1e-3);
            } else {
                assert!((low - surface).abs() < 1e-3);
            }
        }
        assert!(view(&editor).get(CURSOR).is_none());
    }

    #[test]
    fn every_tool_has_the_gems_and_only_the_width_tool_has_the_width_handle() {
        let points = Editor::initial().design.control_points.len();
        for tool in Tool::ALL {
            let ids = ids(&with_tool(tool, Some(WIDTH_POINT)));
            let gems = ids
                .iter()
                .filter(|id| id.starts_with("track/point-"))
                .count();
            assert_eq!(gems, points, "{tool:?}");
            assert_eq!(has(&ids, "track/width-1"), tool == Tool::Width, "{tool:?}");
        }
        assert!(!has(&ids(&with_tool(Tool::Width, None)), "track/width-"));
    }

    #[test]
    fn the_gems_float_above_the_control_points() {
        let editor = with_tool(Tool::Move, None);
        let view = view(&editor);
        for (index, point) in editor.design.control_points.iter().enumerate() {
            let gem = view
                .world_position(&format!("track/point-{index}"))
                .unwrap();
            assert!(gem.distance(point.position + Vec3::Y * editor.handle_radius()) < 1e-3);
        }
    }

    #[test]
    fn dragging_a_gem_selects_it_and_the_move_tool_moves_it() {
        let selected = drag(&with_tool(Tool::Width, None), "track/point-5", |at| {
            at + Vec3::new(9.0, 0.0, 9.0)
        });
        assert_eq!(selected.selected, Some(5));
        assert_eq!(selected.design, Editor::initial().design);

        let before = Editor::initial().design.control_points[5].position;
        let moved = drag(&with_tool(Tool::Move, None), "track/point-5", |at| {
            at + Vec3::new(10.0, 0.0, -6.0)
        });
        let after = moved.design.control_points[5].position;
        assert!(
            (after - before - Vec3::new(10.0, 0.0, -6.0)).length() < 1.0,
            "{after}"
        );
        assert_eq!(moved.selected, Some(5));
    }

    #[test]
    fn dragging_the_width_handle_outwards_widens_the_road_and_inwards_narrows_it() {
        let editor = with_tool(Tool::Width, Some(WIDTH_POINT));
        let before = editor.design.control_points[WIDTH_POINT].width;
        let wider = drag(&editor, "track/width-1", |at| {
            at + editor.frames()[WIDTH_POINT].right * 2.0
        });
        assert!(wider.design.control_points[WIDTH_POINT].width > before);
        let narrower = drag(&editor, "track/width-1", |at| {
            at - editor.frames()[WIDTH_POINT].right * 2.0
        });
        assert!(narrower.design.control_points[WIDTH_POINT].width < before);
        let grown = wider.frames()[WIDTH_POINT].width - editor.frames()[WIDTH_POINT].width;
        assert!((grown - 4.0).abs() < 0.5, "two metres each side: {grown}");
    }

    #[test]
    fn the_width_stays_within_the_limits_of_the_track() {
        let editor = with_tool(Tool::Width, Some(WIDTH_POINT));
        let huge = drag(&editor, "track/width-1", |at| {
            at + editor.frames()[WIDTH_POINT].right * 500.0
        });
        assert_eq!(
            huge.design.control_points[WIDTH_POINT].width,
            crate::track::MAX_POINT_WIDTH
        );
        let tiny = drag(&editor, "track/width-1", |at| {
            at - editor.frames()[WIDTH_POINT].right * 500.0
        });
        assert_eq!(
            tiny.design.control_points[WIDTH_POINT].width,
            crate::track::MIN_POINT_WIDTH
        );
    }

    #[test]
    fn pressing_the_terrain_clears_the_selection_and_closes_the_menu() {
        let editor = with_tool(Tool::Move, Some(2)).with_menu(Some(crate::menu::Ring::Root));
        let cleared = drag(&editor, TERRAIN, |at| at);
        assert_eq!((cleared.selected, cleared.menu), (None, None));
    }
}
