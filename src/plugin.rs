//! The Bevy plugin that wires everything together.

use std::sync::Arc;

use bevy::picking::PickingPlugin;
use bevy::picking::input::PointerInputPlugin;
use bevy::picking::mesh_picking::MeshPickingPlugin;
use bevy::prelude::*;

use crate::behavior::ViewFn;
use crate::input::{DragPlane, on_drag, on_drag_end, on_drag_start, on_press};
use crate::model::DragModel;
use crate::render::{NodeEntities, sync_nodes};
use crate::shapes::ShapeMeshes;
use crate::transition::advance_transitions;
use crate::view::View;

/// Makes the interface described by a view function over the state `S` draggable.
///
/// The plugin inserts a [`DragModel<S>`] resource, renders its view as mesh entities, and turns
/// pointer events on the nodes that carry a drag spec into drags. The scene needs a camera; the
/// drag plane defaults to the XY plane at `z = 0` (see [`DragPlane`]).
pub struct DraggablePlugin<S> {
    initial: S,
    view_fn: ViewFn<S>,
    transition_duration: Option<f32>,
}

impl<S> DraggablePlugin<S> {
    /// Creates the plugin for an interface that starts in `initial` and is drawn by `view_fn`.
    pub fn new(initial: S, view_fn: impl Fn(&S) -> View<S> + Send + Sync + 'static) -> Self {
        Self {
            initial,
            view_fn: Arc::new(view_fn),
            transition_duration: None,
        }
    }

    /// Sets how long the animation after a drop takes, in seconds. Zero snaps instantly.
    #[must_use]
    pub fn with_transition_duration(mut self, seconds: f32) -> Self {
        self.transition_duration = Some(seconds);
        self
    }
}

impl<S: Clone + Send + Sync + 'static> Plugin for DraggablePlugin<S> {
    fn build(&self, app: &mut App) {
        // `DefaultPlugins` already contains the pointer input and picking plugins, but not the
        // mesh backend this crate relies on, so each plugin is checked on its own.
        if !app.is_plugin_added::<PointerInputPlugin>() {
            app.add_plugins(PointerInputPlugin);
        }
        if !app.is_plugin_added::<PickingPlugin>() {
            app.add_plugins(PickingPlugin);
        }
        if !app.is_plugin_added::<MeshPickingPlugin>() {
            app.add_plugins(MeshPickingPlugin);
        }
        #[cfg(any(feature = "shapes-2d", feature = "shapes-3d"))]
        if !app.is_plugin_added::<bevy_gradmesh::GradMeshPlugin>() {
            app.add_plugins(bevy_gradmesh::GradMeshPlugin);
        }
        let mut model = DragModel::from_view_fn(self.initial.clone(), Arc::clone(&self.view_fn));
        if let Some(seconds) = self.transition_duration {
            model.set_transition_duration(seconds);
        }
        app.insert_resource(model)
            .init_resource::<DragPlane>()
            .init_resource::<ShapeMeshes>()
            .init_resource::<NodeEntities<S>>()
            .add_observer(on_press::<S>)
            .add_observer(on_drag_start::<S>)
            .add_observer(on_drag::<S>)
            .add_observer(on_drag_end::<S>)
            .add_systems(Update, (advance_transitions::<S>, sync_nodes::<S>).chain());
    }
}
