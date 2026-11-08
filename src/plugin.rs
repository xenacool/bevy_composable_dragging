//! The Bevy plugin that wires everything together.

use std::sync::Arc;

use bevy::picking::PickingPlugin;
use bevy::picking::input::PointerInputPlugin;
use bevy::picking::mesh_picking::MeshPickingPlugin;
use bevy::prelude::*;

use crate::behavior::ViewFn;
use crate::input::{DragPlane, on_click, on_drag, on_drag_end, on_drag_start, on_press};
use crate::model::DragModel;
use crate::render::{NodeEntities, sync_nodes};
use crate::rendering::Rendering;
use crate::shapes::ShapeMeshes;
#[cfg(feature = "gradmesh")]
use crate::shapes::release_unused_materials;
use crate::transition::advance_transitions;
use crate::view::View;

/// Makes the interface described by a view function over the state `S` draggable.
///
/// The plugin inserts a [`DragModel<S>`] resource, renders its view as mesh entities, and turns
/// pointer events on the nodes that carry a drag spec into drags. The scene needs a camera; the
/// drag plane defaults to the XY plane at `z = 0` (see [`DragPlane`]). Nodes are drawn as `Mesh3d`
/// entities for a `Camera3d`, or as flat `Mesh2d` entities for a `Camera2d` with
/// [`with_rendering`](Self::with_rendering).
pub struct DraggablePlugin<S> {
    initial: S,
    view_fn: ViewFn<S>,
    transition_duration: Option<f32>,
    rendering: Rendering,
}

impl<S> DraggablePlugin<S> {
    /// Creates the plugin for an interface that starts in `initial` and is drawn by `view_fn`.
    pub fn new(initial: S, view_fn: impl Fn(&S) -> View<S> + Send + Sync + 'static) -> Self {
        Self {
            initial,
            view_fn: Arc::new(view_fn),
            transition_duration: None,
            rendering: Rendering::default(),
        }
    }

    /// Chooses between `Mesh3d` entities for a `Camera3d` (the default) and flat `Mesh2d`
    /// entities for a `Camera2d`. Single nodes override it with `ViewNode::flat` and
    /// `ViewNode::spatial`. See [`Rendering`].
    #[must_use]
    pub fn with_rendering(mut self, rendering: Rendering) -> Self {
        self.rendering = rendering;
        self
    }

    /// Sets how long the animation after a drop takes, in seconds. Zero snaps instantly.
    #[must_use]
    pub fn with_transition_duration(mut self, seconds: f32) -> Self {
        self.transition_duration = Some(seconds);
        self
    }
}

/// Registers what the nodes need from `bevy_gradmesh` in an app without a renderer, such as a
/// test app without a GPU: the material assets and their pool, but none of the GPU pipelines.
#[cfg(feature = "gradmesh")]
fn add_headless_assets(app: &mut App) {
    use bevy::sprite_render::ColorMaterial;
    use bevy_gradmesh::{
        GradMeshMaterial, GradMeshMaterial2d, GradMeshMaterialPool, SdfMaterial2d,
    };

    app.init_asset::<Mesh>()
        .init_asset::<StandardMaterial>()
        .init_asset::<ColorMaterial>()
        .init_asset::<GradMeshMaterial>()
        .init_asset::<GradMeshMaterial2d>()
        .init_asset::<SdfMaterial2d>()
        .init_resource::<GradMeshMaterialPool>();
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
        #[cfg(feature = "gradmesh")]
        if !app.is_plugin_added::<bevy_gradmesh::GradMeshPlugin>() {
            if app.get_sub_app(bevy::render::RenderApp).is_some() {
                app.add_plugins(bevy_gradmesh::GradMeshPlugin);
            } else {
                add_headless_assets(app);
            }
        }
        let mut model = DragModel::from_view_fn(self.initial.clone(), Arc::clone(&self.view_fn));
        if let Some(seconds) = self.transition_duration {
            model.set_transition_duration(seconds);
        }
        app.insert_resource(model)
            .insert_resource(self.rendering)
            .init_resource::<DragPlane>()
            .init_resource::<ShapeMeshes>()
            .init_resource::<NodeEntities<S>>()
            .add_observer(on_press::<S>)
            .add_observer(on_click::<S>)
            .add_observer(on_drag_start::<S>)
            .add_observer(on_drag::<S>)
            .add_observer(on_drag_end::<S>)
            .add_systems(Update, (advance_transitions::<S>, sync_nodes::<S>).chain());
        #[cfg(feature = "gradmesh")]
        app.add_systems(Update, release_unused_materials.after(sync_nodes::<S>));
    }
}
