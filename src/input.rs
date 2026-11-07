//! Turns picking events into drags.

use bevy::camera::visibility::RenderLayers;
use bevy::picking::events::{PointerDrag, PointerDragEnd, PointerDragStart, PointerPress};
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;

use crate::model::DragModel;
use crate::render::DragNode;
use crate::space::DragSpace;

/// The space that drags happen in when a node does not choose one with
/// [`ViewNode::draggable_in`](crate::ViewNode::draggable_in): the XY plane at depth `z`.
///
/// Drag specs work with positions in that plane. For 2D-style scenes the default of `z = 0` is
/// what you want; scenes with a perspective camera can move the plane to the depth of the
/// objects being dragged.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct DragPlane {
    /// The depth of the plane.
    pub z: f32,
}

impl DragPlane {
    /// The drag space of the plane.
    #[must_use]
    pub fn space(self) -> DragSpace {
        DragSpace::world_plane(self.z)
    }
}

/// The ray from `camera` through a pointer position in window coordinates.
fn pointer_ray(
    cameras: &Query<(&Camera, &GlobalTransform)>,
    camera: Entity,
    position: Vec2,
) -> Option<Ray3d> {
    let (camera, transform) = cameras.get(camera).ok()?;
    camera.viewport_to_world(transform, position).ok()
}

/// Remembers a press on a draggable node.
pub(crate) fn on_press<S: Clone + Send + Sync + 'static>(
    mut press: On<PointerPress>,
    nodes: Query<&DragNode<S>>,
    cameras: Query<(&Camera, &GlobalTransform)>,
    camera_layers: Query<&RenderLayers, With<Camera>>,
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
    let Some(ray) = pointer_ray(&cameras, camera, press.pointer.position) else {
        return;
    };
    let Ok((_, transform)) = cameras.get(camera) else {
        return;
    };
    // A camera without `RenderLayers` shows layer 0 only.
    let layers = camera_layers.get(camera).cloned().unwrap_or_default();
    let visible = |layer| layers.intersects(&RenderLayers::layer(layer));
    if model.press(
        &node.id,
        camera,
        ray,
        transform.forward(),
        plane.space(),
        visible,
    ) {
        press.propagate(false);
    }
}

/// Starts the drag once the pointer moves after a press.
pub(crate) fn on_drag_start<S: Clone + Send + Sync + 'static>(
    mut start: On<PointerDragStart>,
    nodes: Query<&DragNode<S>>,
    cameras: Query<(&Camera, &GlobalTransform)>,
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
    if let Some(ray) = pointer_ray(&cameras, camera, start.pointer.position) {
        model.drive(ray);
    }
}

/// Updates the preview while the pointer moves.
pub(crate) fn on_drag<S: Clone + Send + Sync + 'static>(
    mut drag: On<PointerDrag>,
    nodes: Query<&DragNode<S>>,
    cameras: Query<(&Camera, &GlobalTransform)>,
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
    if let Some(ray) = pointer_ray(&cameras, camera, drag.pointer.position) {
        model.drive(ray);
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
