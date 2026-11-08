//! The view of the editor: the handles of the track, the gizmo and the menu.
//!
//! The track itself is drawn by the world of the kart racer from the design, not by the view;
//! the view draws the architect's grid and stilts and, until the track can be driven, a ribbon.
//! The view holds what is edited with the pointer: a node named `track` that the handles of the
//! curve hang from, and an `anchor` at the selected control point that the gizmo hangs from. The
//! tool decides which handles are there. While the track is driven there are only the HUD nodes.

use std::sync::Arc;

use bevy::prelude::*;
use bevy_composable_dragging::gizmo::{Gizmo, GizmoReveal};
use bevy_composable_dragging::handles::{CurveHandles, Lateral, Twist};
use bevy_composable_dragging::hud::{Layout, Theme};
use bevy_composable_dragging::ops::{Snap, custom, state};
use bevy_composable_dragging::radial::{RadialHost, push_menu};
use bevy_composable_dragging::{DragContext, DragSpace, DragSpec, Frame, Pointer, View, ViewNode};

use crate::blueprint;
use crate::common::HUD_LAYER;
use crate::ground::BASE;
use crate::state::{Cursor, Editor, Tool};

/// The id of the node that the handles of the curve hang from.
pub const TRACK: &str = "track";
/// The id of the node that the gizmo hangs from.
pub const ANCHOR: &str = "anchor";
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
const CURSOR_COLOR: Color = Color::srgba(0.45, 0.75, 1.0, 0.45);

/// The look of the HUD: the library theme, drawn on the layer of the HUD camera.
pub fn theme() -> Theme {
    Theme::new(HUD_LAYER)
}

/// A node that is there to be the parent of handles, and is not seen or hit.
fn anchor(id: &str, at: Vec3) -> ViewNode<Editor> {
    ViewNode::group(id.to_owned()).at(at)
}

/// What a press on the ground does with the Insert tool: it lays a control point where it lands,
/// and the point follows the pointer until the press is let go. A drag hands its operator the
/// place of the node it holds, which is the pointer less the grab offset, so the offset is put
/// back to find the pointer itself.
fn place() -> DragSpec<Editor> {
    DragSpec::new(|context: &DragContext<Editor>| {
        let start = context.start_state().clone();
        let grab = context.grab_offset();
        let context = context.clone();
        Box::new(move |pointer| {
            let next = start.placed(pointer.point + grab);
            Frame::new(context.render(&next), next, 0.0)
        })
    })
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
/// in the steps of the precision. The column of the brush shows how far, and the land takes the
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
            let (shown, committed) = editor.precision.snap().apply(raw);
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

/// What a press on the land does with the Holes tool: it cuts a hole under the pointer, or fills
/// one in when the press is on a hole, and dragging carries on the same way over the land it
/// crosses, so a trench can be cut or a field filled in one stroke.
fn carve(editor: &Editor) -> DragSpec<Editor> {
    let editor = editor.clone();
    custom(move |context| {
        let context = context.clone();
        let start = editor.clone();
        let mut carved = editor.clone();
        let mut cutting: Option<bool> = None;
        Box::new(move |pointer: Pointer| {
            let Some(hit) = start.ground.hit(pointer.ray) else {
                return Frame::new(context.render(&carved), carved.clone(), 0.0);
            };
            let cut = *cutting.get_or_insert_with(|| !start.ground.is_hole(hit.xz()));
            carved = carved.holed(hit.xz(), cut);
            let cursor = Cursor {
                center: hit.xz(),
                radius: carved.brush_radius(),
                rise: 0.0,
            };
            Frame::new(
                context.render(&carved.with_cursor(Some(cursor))),
                carved.clone(),
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
/// Insert tool it places a control point, or with a tool of the land it shapes the land.
fn terrain(editor: &Editor) -> ViewNode<Editor> {
    let (middle, half) = editor.terrain_square();
    let width = half * 2.0;
    let thickness = width * TERRAIN_THICKNESS;
    let cleared = editor.with_selected(None).with_menu(None);
    let tool = editor.tool;
    let snapshot = editor.clone();
    ViewNode::cuboid(TERRAIN, Vec3::new(width, thickness, width), 0.0)
        .at(Vec3::new(
            middle.x,
            GROUND_HEIGHT - thickness / 2.0,
            middle.y,
        ))
        .with_opacity(0.0)
        .casts_shadow(false)
        .draggable_in(
            DragSpace::plane(Vec3::Y * GROUND_HEIGHT, Dir3::Y),
            move || match tool {
                Tool::Insert => place(),
                Tool::Hills => sculpt(&snapshot),
                Tool::Hole => carve(&snapshot),
                _ => state(cleared.clone()),
            },
        )
}

/// The handles the chosen tool shows for the control points of the track.
fn curve_handles(editor: &Editor) -> CurveHandles<Editor> {
    let tool = editor.tool;
    let snap = editor.precision.snap();
    let turn = editor.precision.turn();
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
    let twist: Vec<Twist> = editor
        .frames()
        .iter()
        .map(|frame| Twist {
            axis: frame.tangent,
            angle: frame.roll,
        })
        .collect();
    let mut handles = CurveHandles::new(TRACK, Transform::IDENTITY, &points)
        .closed(true)
        .selected(editor.selected)
        .with_radius(editor.handle_radius())
        .move_point(
            if tool == Tool::Move { snap } else { Snap::NONE },
            move |editor: &Editor, index, offset| match tool {
                Tool::Move => editor.moved(index, offset),
                Tool::Delete => editor.removed(index),
                _ => editor.with_selected(Some(index)),
            },
        );
    if let Some(eye) = eye(editor) {
        handles = handles.seen_from(eye);
    }
    match tool {
        Tool::Raise => {
            handles = handles.raise_point(snap, |editor: &Editor, index, rise| {
                editor.raised(index, rise)
            });
        }
        Tool::Width => {
            handles = handles.width(&lateral, snap, |editor: &Editor, index, change| {
                editor.widened(index, change)
            });
        }
        Tool::Roll => {
            handles = handles.roll(&twist, turn, |editor: &Editor, index, change| {
                editor.rolled(index, change)
            });
        }
        Tool::Insert => {
            handles = handles.insert_point(snap, |editor: &Editor, segment, offset| {
                editor.inserted(segment, offset)
            });
        }
        Tool::Select | Tool::Move | Tool::Delete | Tool::Hills | Tool::Hole | Tool::Drive => {}
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

/// The gizmo of the selected control point, which moves it along each axis and in each plane.
fn point_gizmo(editor: &Editor, index: usize) -> Gizmo<Editor> {
    let at = editor.handle_position(index);
    let gizmo = Gizmo::new(ANCHOR, Transform::from_translation(at)).with_size(editor.gizmo_size());
    let gizmo = match eye(editor) {
        Some(eye) => gizmo.seen_from(eye),
        None => gizmo,
    };
    gizmo.translate(editor.precision.snap(), move |editor: &Editor, offset| {
        editor.moved(index, offset)
    })
}

/// The points of the world that the handles of the chosen tool occupy around control point
/// `index`: the gem, the cones, the width handle, the roll ring and the gizmo. The chip of the
/// menu keeps clear of them.
pub fn footprint(editor: &Editor, index: usize) -> Vec<Vec3> {
    let mut spots = curve_handles(editor).footprint(index);
    if editor.reveals_gizmo() && editor.point().is_some_and(|(chosen, _)| chosen == index) {
        spots.extend(point_gizmo(editor, index).footprint());
    }
    spots
}

/// The view of the editor: the handles of the track unless it is being driven, and the menu.
pub fn view(editor: &Editor) -> View<Editor> {
    let mut view = View::new();
    if !editor.testing {
        view.push(terrain(editor));
        if let Some(brush) = cursor(editor) {
            view.push(brush);
        }
        blueprint::push(&mut view, editor);
        view.push(anchor(TRACK, Vec3::ZERO));
        curve_handles(editor).add_to(&mut view);
        if let Some((index, _)) = editor.point() {
            let at = editor.handle_position(index);
            view.push(anchor(ANCHOR, at));
            point_gizmo(editor, index).add_if_revealed(&mut view, editor);
        }
    }
    let layout = Layout::new(&editor.screen);
    push_menu(&mut view, &Arc::new(editor.clone()), &layout, &theme());
    view
}

#[cfg(test)]
mod tests {
    use bevy_composable_dragging::{DragContext, Pointer};

    use super::*;

    fn with_tool(tool: Tool, selected: Option<usize>) -> Editor {
        let mut editor = Editor::upstream().with_selected(selected);
        editor.tool = tool;
        editor
    }

    fn empty(tool: Tool) -> Editor {
        Editor::initial().with_tool(tool)
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
        let spec = (node_ref.drag.as_ref().unwrap())();
        let anchor = view.world_position(node).unwrap();
        let context =
            DragContext::new(Arc::new(self::view), node.to_owned().into(), editor.clone())
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
    fn the_land_tools_have_the_gems_and_the_terrain_and_no_other_handles() {
        for tool in [Tool::Hills, Tool::Hole] {
            let ids = ids(&with_tool(tool, None));
            assert!(has(&ids, TERRAIN), "{tool:?}");
            assert!(has(&ids, "track/point-"), "{tool:?}");
            for kind in [
                "track/lift-",
                "track/mid-",
                "track/width-",
                "track/roll-",
                "anchor",
            ] {
                assert!(!has(&ids, kind), "{tool:?}: {kind}");
            }
            assert!(
                !has(&ids, CURSOR),
                "{tool:?}: there is no brush until the press"
            );
        }
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
    fn the_select_tool_has_gems_and_nothing_else() {
        let ids = ids(&with_tool(Tool::Select, None));
        let points = ids
            .iter()
            .filter(|id| id.starts_with("track/point-"))
            .count();
        assert_eq!(points, Editor::upstream().design.control_points.len());
        for kind in [
            "track/lift-",
            "track/mid-",
            "track/width-",
            "track/roll-",
            "anchor",
        ] {
            assert!(!has(&ids, kind), "{kind}");
        }
    }

    #[test]
    fn each_tool_shows_its_own_handles() {
        let selected = Some(4);
        assert!(has(&ids(&with_tool(Tool::Raise, selected)), "track/lift-"));
        assert!(has(&ids(&with_tool(Tool::Insert, selected)), "track/mid-"));
        let width = ids(&with_tool(Tool::Width, selected));
        assert!(has(&width, "track/width-4"));
        assert!(!has(&width, "track/roll-"));
        let roll = ids(&with_tool(Tool::Roll, selected));
        assert!(has(&roll, "track/roll-4"));
        assert!(!has(&roll, "track/width-"));
        assert!(!has(&ids(&with_tool(Tool::Width, None)), "track/width-"));
    }

    #[test]
    fn the_gizmo_follows_the_tool_and_the_drive() {
        for tool in Tool::ALL {
            let ids = ids(&with_tool(tool, Some(2)));
            assert_eq!(has(&ids, "anchor/"), tool.reveals_gizmo(), "{tool:?}");
        }
        assert!(!has(&ids(&with_tool(Tool::Move, None)), "anchor"));
        let mut driving = with_tool(Tool::Move, Some(2));
        driving.testing = true;
        assert!(
            ids(&driving).is_empty(),
            "only the menu is left while driving"
        );
    }

    #[test]
    fn the_gems_float_above_the_control_points() {
        let editor = with_tool(Tool::Select, None);
        let view = view(&editor);
        for (index, point) in editor.design.control_points.iter().enumerate() {
            let gem = view
                .world_position(&format!("track/point-{index}"))
                .unwrap();
            assert!(gem.distance(point.position + Vec3::Y * editor.handle_radius()) < 1e-3);
        }
    }

    #[test]
    fn dragging_a_gem_selects_and_the_move_tool_moves_it() {
        let selected = drag(&with_tool(Tool::Select, None), "track/point-5", |at| {
            at + Vec3::new(9.0, 0.0, 9.0)
        });
        assert_eq!(selected.selected, Some(5));
        assert_eq!(selected.design, Editor::upstream().design);

        let before = Editor::upstream().design.control_points[5].position;
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
    fn dragging_a_cone_raises_the_road_in_steps() {
        let editor = with_tool(Tool::Raise, Some(3));
        let before = editor.design.control_points[3].position.y;
        let raised = drag(&editor, "track/lift-3", |at| at + Vec3::new(4.0, 2.4, 0.0));
        let after = raised.design.control_points[3].position.y;
        assert!(
            (after - before - 2.0).abs() < 1e-3,
            "one metre steps: {after}"
        );
    }

    #[test]
    fn dragging_the_width_handle_outwards_widens_the_road_and_inwards_narrows_it() {
        let editor = with_tool(Tool::Width, Some(3));
        let before = editor.design.control_points[3].width;
        let wider = drag(&editor, "track/width-3", |at| {
            at + editor.frames()[3].right * 2.0
        });
        assert!(wider.design.control_points[3].width > before);
        let narrower = drag(&editor, "track/width-3", |at| {
            at - editor.frames()[3].right * 2.0
        });
        assert!(narrower.design.control_points[3].width < before);
        let width_after = |editor: &Editor| editor.frames()[3].width;
        let grown = width_after(&wider) - width_after(&editor);
        assert!((grown - 4.0).abs() < 0.5, "two metres each side: {grown}");
    }

    #[test]
    fn the_width_stays_within_the_limits_of_the_track() {
        let editor = with_tool(Tool::Width, Some(3));
        let huge = drag(&editor, "track/width-3", |at| {
            at + editor.frames()[3].right * 500.0
        });
        assert_eq!(
            huge.design.control_points[3].width,
            crate::track::MAX_POINT_WIDTH
        );
        let tiny = drag(&editor, "track/width-3", |at| {
            at - editor.frames()[3].right * 500.0
        });
        assert_eq!(
            tiny.design.control_points[3].width,
            crate::track::MIN_POINT_WIDTH
        );
    }

    #[test]
    fn dragging_the_roll_ring_banks_the_road_in_steps_and_within_the_limit() {
        let editor = with_tool(Tool::Roll, Some(3)).with_precision(crate::state::Precision::Coarse);
        let frame = editor.frames()[3];
        let node = "track/roll-3";
        let view = view(&editor);
        let node_ref = view.get(node).unwrap();
        let spec = (node_ref.drag.as_ref().unwrap())();
        let center = editor.handle_position(3);
        let grab = frame.up * 5.0;
        let context =
            DragContext::new(Arc::new(self::view), node.to_owned().into(), editor.clone())
                .with_space(node_ref.drag_space.unwrap())
                .with_grab_offset(grab);
        let mut behavior = spec.compile(&context);
        let mut turned = |degrees: f32| {
            let pointer =
                center + Quat::from_axis_angle(frame.tangent, degrees.to_radians()) * grab;
            behavior(Pointer::at(pointer - grab))
                .drop_state
                .design
                .control_points[3]
                .roll
        };
        let banked = turned(20.0);
        assert!(
            (banked - 15_f32.to_radians()).abs() < 1e-3,
            "fifteen degree steps: {banked}"
        );
        let other_way = turned(-20.0);
        assert!(
            (other_way + 15_f32.to_radians()).abs() < 1e-3,
            "{other_way}"
        );
        let far = turned(100.0);
        assert!(
            (far - crate::track::MAX_ROLL).abs() < 1e-5,
            "the limit holds: {far}"
        );
    }

    #[test]
    fn dragging_a_middle_gem_inserts_a_control_point_there() {
        let editor = with_tool(Tool::Insert, Some(0));
        let count = editor.design.control_points.len();
        let inserted = drag(&editor, "track/mid-2", |at| at + Vec3::new(0.0, 0.0, 5.0));
        assert_eq!(inserted.design.control_points.len(), count + 1);
        assert_eq!(inserted.selected, Some(3));
    }

    #[test]
    fn pressing_a_gem_with_the_delete_tool_removes_the_point_unless_too_few_are_left() {
        let editor = with_tool(Tool::Delete, None);
        let count = editor.design.control_points.len();
        let removed = drag(&editor, "track/point-4", |at| at);
        assert_eq!(removed.design.control_points.len(), count - 1);
        assert_eq!(removed.selected, None);

        let mut small = Editor::upstream();
        small.tool = Tool::Delete;
        let mut design = small.design.clone();
        while design.control_points.len() > crate::track::MIN_POINTS {
            design = design.with_point_removed(0);
        }
        let small = small.with_design(design);
        let refused = drag(&small, "track/point-0", |at| at);
        assert_eq!(
            refused.design.control_points.len(),
            crate::track::MIN_POINTS
        );
    }

    #[test]
    fn the_gizmo_moves_the_selected_point_along_an_axis() {
        let editor = with_tool(Tool::Move, Some(6));
        let view = view(&editor);
        let handle = view
            .nodes()
            .iter()
            .map(|node| node.id.to_string())
            .find(|id| id.starts_with("anchor/") && id.contains("translate-y"))
            .expect("the gizmo has a vertical translate handle");
        let before = editor.design.control_points[6].position;
        let moved = drag(&editor, &handle, |at| at + Vec3::Y * 4.0);
        let after = moved.design.control_points[6].position;
        assert!(
            (after.y - before.y - 4.0).abs() < 1e-2,
            "{after} from {before}"
        );
        assert!((after.x - before.x).abs() < 1e-3 && (after.z - before.z).abs() < 1e-3);
    }

    #[test]
    fn an_empty_editor_has_the_ground_and_no_handles_and_nothing_that_is_not_a_number() {
        let editor = Editor::initial();
        let ids = ids(&editor);
        assert!(!has(&ids, "track/point-") && !has(&ids, "track/mid-"));
        let view = view(&editor);
        let at = view.world_position(TERRAIN).unwrap();
        assert!(at.is_finite(), "{at}");
    }

    #[test]
    fn pressing_the_ground_places_a_point_with_insert_and_otherwise_selects_the_terrain() {
        let ground = |_| Vec3::new(10.0, GROUND_HEIGHT, 20.0);
        let placed = drag(&empty(Tool::Insert), TERRAIN, ground);
        assert_eq!(placed.design.control_points.len(), 1);
        assert_eq!(
            placed.design.control_points[0].position,
            Vec3::new(10.0, 0.0, 20.0)
        );
        assert_eq!(placed.selected, Some(0));

        let more = drag(&placed.with_tool(Tool::Insert), TERRAIN, |_| {
            Vec3::new(-5.0, GROUND_HEIGHT, 3.0)
        });
        assert_eq!(more.design.control_points.len(), 2);
        assert_eq!(more.selected, Some(1), "the newest point is selected");

        let moving = drag(&empty(Tool::Move), TERRAIN, ground);
        assert!(
            moving.design.control_points.is_empty(),
            "other tools place nothing"
        );
    }
}
