//! Turns picking events into drags.

use bevy::picking::events::{PointerDrag, PointerDragEnd, PointerDragStart, PointerPress};
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;

use crate::model::DragModel;
use crate::render::DragNode;

/// The plane that the pointer is projected onto while dragging.
///
/// Drag specs work with positions in the XY plane at depth `z`. For 2D-style scenes the default
/// of `z = 0` is what you want; scenes with a perspective camera can move the plane to the depth
/// of the objects being dragged.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct DragPlane {
    /// The depth of the plane.
    pub z: f32,
}

/// Projects a pointer position (in window coordinates) through `camera` onto the drag plane.
fn pointer_on_plane(
    cameras: &Query<(&Camera, &GlobalTransform)>,
    camera: Entity,
    position: Vec2,
    plane: DragPlane,
) -> Option<Vec2> {
    let (camera, transform) = cameras.get(camera).ok()?;
    let ray = camera.viewport_to_world(transform, position).ok()?;
    let distance =
        ray.intersect_plane(Vec3::new(0.0, 0.0, plane.z), InfinitePlane3d::new(Vec3::Z))?;
    Some(ray.get_point(distance).truncate())
}

/// Remembers a press on a draggable node.
pub(crate) fn on_press<S: Clone + Send + Sync + 'static>(
    mut press: On<PointerPress>,
    nodes: Query<&DragNode<S>>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    plane: Res<DragPlane>,
    mut model: ResMut<DragModel<S>>,
) {
    if press.button != PointerButton::Primary {
        return;
    }
    let Ok(node) = nodes.get(press.entity) else {
        return;
    };
    let camera = press.hit.camera;
    let Some(pointer) = pointer_on_plane(&cameras, camera, press.pointer.position, *plane) else {
        return;
    };
    if model.press(&node.id, camera, pointer) {
        press.propagate(false);
    }
}

/// Starts the drag once the pointer moves after a press.
pub(crate) fn on_drag_start<S: Clone + Send + Sync + 'static>(
    mut start: On<PointerDragStart>,
    nodes: Query<&DragNode<S>>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    plane: Res<DragPlane>,
    mut model: ResMut<DragModel<S>>,
) {
    let Ok(node) = nodes.get(start.entity) else {
        return;
    };
    if model.pending_node() != Some(node.id.as_ref()) {
        return;
    }
    let Some(camera) = model.start_drag() else {
        return;
    };
    start.propagate(false);
    if let Some(pointer) = pointer_on_plane(&cameras, camera, start.pointer.position, *plane) {
        model.drive(pointer);
    }
}

/// Updates the preview while the pointer moves.
pub(crate) fn on_drag<S: Clone + Send + Sync + 'static>(
    mut drag: On<PointerDrag>,
    nodes: Query<&DragNode<S>>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    plane: Res<DragPlane>,
    mut model: ResMut<DragModel<S>>,
) {
    let Ok(node) = nodes.get(drag.entity) else {
        return;
    };
    if model.dragged_node() != Some(node.id.as_ref()) {
        return;
    }
    drag.propagate(false);
    let Some(camera) = model.drag_camera() else {
        return;
    };
    if let Some(pointer) = pointer_on_plane(&cameras, camera, drag.pointer.position, *plane) {
        model.drive(pointer);
    }
}

/// Drops the dragged node on release.
pub(crate) fn on_drag_end<S: Clone + Send + Sync + 'static>(
    mut end: On<PointerDragEnd>,
    nodes: Query<&DragNode<S>>,
    mut model: ResMut<DragModel<S>>,
) {
    let Ok(node) = nodes.get(end.entity) else {
        return;
    };
    if model.dragged_node() != Some(node.id.as_ref()) {
        return;
    }
    end.propagate(false);
    model.drop_drag();
}
