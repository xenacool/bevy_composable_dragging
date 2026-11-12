//! Text labels that follow `ViewNode`s, so a draggable shape can have a readable name.
//!
//! The engine blends views by id, but `Text2d` entities are not part of the view -- they are
//! ordinary ECS entities that a sync system keeps in step with the view. This module provides
//! [`LabelPlugin`], which runs that sync system for any view.
//!
//! The user inserts [`LabelsResource`] with a function that returns the labels for the current
//! state, then adds [`LabelPlugin`] to run the sync system. Each label has a stable key, the
//! text to show, and the parent node id it follows. The plugin spawns one `Text2d` entity per
//! label, removes it when the label is gone, and updates its text when the state changes.
//!
//! Labels are drawn in screen space. Every frame the plugin projects the parent node through
//! the scene camera and places the text at that pixel, so the text always has the size in
//! pixels that the label asks for, whatever the window size or the scaling mode of the camera.
//! Because the projection comes from the camera itself, flat, isometric and perspective scenes
//! all work. A label whose node is behind the camera or outside its depth range is hidden.
//!
//! The scene camera is the active camera with the highest order that renders layer 0, the
//! layer nodes are drawn on by default. The labels are drawn by a camera of their own on the
//! layer given to [`LabelsResource`], which no scene camera may render, or the text would be
//! drawn twice.
//!
//! ```no_run
//! use bevy::prelude::*;
//! use bevy_composable_dragging::label::{Label, LabelPlugin, LabelsResource};
//! use bevy_composable_dragging::{DraggablePlugin, View, ViewNode};
//!
//! #[derive(Clone)]
//! struct Editor {
//!     nodes: Vec<(u32, String)>,
//! }
//!
//! fn view(state: &Editor) -> View<Editor> {
//!     let mut view = View::new();
//!     for (id, _name) in &state.nodes {
//!         view.push(
//!             ViewNode::rectangle(format!("node-{id}"), Vec2::new(80.0, 60.0))
//!                 .at_xy(Vec2::new(*id as f32 * 100.0, 0.0)),
//!         );
//!     }
//!     view
//! }
//!
//! fn labels(state: &Editor, _view: &View<Editor>) -> Vec<Label> {
//!     state
//!         .nodes
//!         .iter()
//!         .map(|(id, name)| Label::new(format!("label-{id}"), name.clone(), format!("node-{id}")))
//!         .collect()
//! }
//!
//! App::new()
//!     .add_plugins(DefaultPlugins)
//!     .add_plugins(DraggablePlugin::new(Editor { nodes: vec![(0, "Start".into())] }, view))
//!     .insert_resource(LabelsResource::<Editor>::new(labels, 3))
//!     .add_plugins(LabelPlugin::<Editor>::default())
//!     .run();
//! ```

use std::collections::HashSet;
use std::marker::PhantomData;

use bevy::camera::visibility::RenderLayers;
use bevy::ecs::entity::Entity;
use bevy::ecs::system::Commands;
use bevy::math::Vec3;
use bevy::prelude::*;
use bevy::sprite::Anchor;
use bevy::window::PrimaryWindow;

use crate::DragModel;
use crate::view::View;

/// The default font size of a label, in pixels.
const DEFAULT_LABEL_SIZE: f32 = 14.0;

/// The default depth of labels in the label camera.
const DEFAULT_LABEL_DEPTH: f32 = 1.0;

/// The default order of the label camera, high so that the text is drawn over every other camera.
const DEFAULT_LABEL_CAMERA_ORDER: isize = 1000;

/// Describes one text label.
///
/// The key identifies the label across frames so the plugin keeps the same entity. The text is
/// the string shown. The parent is the `ViewNode` id the label follows; when `None`, the label
/// sits at a fixed world position.
#[derive(Clone, Debug, PartialEq)]
pub struct Label {
    /// Names the label, so its entity is kept from one frame to the next.
    pub key: String,
    /// The text to display.
    pub text: String,
    /// The `ViewNode` id this label follows. When `None`, the label stays at a fixed position.
    pub parent: Option<String>,
    /// The world position of the label, used only when `parent` is `None`.
    pub at: Option<Vec3>,
    /// Where the text sits relative to the projected position, in pixels with y pointing up.
    /// The default centers the text on the node.
    pub offset: Vec2,
    /// The color of the text.
    pub color: Color,
    /// The font size in pixels.
    pub font_size: f32,
    /// The render layer of the label.
    pub layer: usize,
}

impl Label {
    /// Creates a label centered on the node with id `parent`.
    #[must_use]
    pub fn new(key: impl Into<String>, text: impl Into<String>, parent: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            text: text.into(),
            parent: Some(parent.into()),
            at: None,
            offset: Vec2::ZERO,
            color: Color::WHITE,
            font_size: DEFAULT_LABEL_SIZE,
            layer: 0,
        }
    }

    /// Creates a label at a fixed world position.
    #[must_use]
    pub fn at(key: impl Into<String>, text: impl Into<String>, at: Vec3) -> Self {
        Self {
            key: key.into(),
            text: text.into(),
            parent: None,
            at: Some(at),
            offset: Vec2::ZERO,
            color: Color::WHITE,
            font_size: DEFAULT_LABEL_SIZE,
            layer: 0,
        }
    }

    /// Moves the text by `offset` pixels from the projected position, with y pointing up.
    #[must_use]
    pub fn with_offset(mut self, offset: Vec2) -> Self {
        self.offset = offset;
        self
    }

    /// Sets the text color.
    #[must_use]
    pub fn with_color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// Sets the font size in pixels.
    #[must_use]
    pub fn with_font_size(mut self, size: f32) -> Self {
        self.font_size = size.max(1.0);
        self
    }

    /// Sets the render layer.
    #[must_use]
    pub fn on_layer(mut self, layer: usize) -> Self {
        self.layer = layer;
        self
    }
}

/// Marks a Text2d entity managed by the label plugin.
#[derive(Component, Debug)]
pub struct LabelMarker(pub String);

/// Marks the camera spawned by [`LabelPlugin`] so it can be queried or excluded.
#[derive(Component)]
pub struct LabelCamera;

/// The query type for existing label entities.
type LabelQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static LabelMarker,
        &'static mut Text2d,
        &'static mut Transform,
        &'static mut Visibility,
        &'static mut TextColor,
        &'static mut TextFont,
        &'static mut RenderLayers,
    ),
>;

/// The query type for the cameras that may draw the scene.
type SceneCameras<'w, 's> = Query<
    'w,
    's,
    (
        &'static Camera,
        &'static Projection,
        &'static GlobalTransform,
        Option<&'static RenderLayers>,
    ),
    (Without<LabelCamera>, Without<LabelMarker>),
>;

/// The labels function stored as a resource by [`LabelPlugin`].
///
/// Insert this resource with your labels function, then add [`LabelPlugin`] to run the sync
/// system. The plugin reads this resource each frame to determine which labels to show.
#[derive(Resource)]
pub struct LabelsResource<S> {
    /// The function that returns the labels for the current state and view.
    #[allow(clippy::type_complexity)]
    pub labels_fn: Box<dyn Fn(&S, &View<S>) -> Vec<Label> + Send + Sync>,
    /// The render layer of the labels and of the camera that draws them. No scene camera may
    /// render it, or the text would be drawn a second time at the wrong size.
    pub layer: usize,
    /// The depth of labels along the camera view direction. Positive values place labels in front
    /// of the scene; negative values behind. Default is 1.0.
    pub depth: f32,
    /// The render order of the label camera. Higher values render later (on top). Default is 1000.
    pub camera_order: isize,
    _marker: PhantomData<fn() -> S>,
}

impl<S> LabelsResource<S> {
    /// Creates a new labels resource with the given function and layer.
    #[must_use]
    pub fn new(
        labels_fn: impl Fn(&S, &View<S>) -> Vec<Label> + Send + Sync + 'static,
        layer: usize,
    ) -> Self {
        Self {
            labels_fn: Box::new(labels_fn),
            layer,
            depth: DEFAULT_LABEL_DEPTH,
            camera_order: DEFAULT_LABEL_CAMERA_ORDER,
            _marker: PhantomData,
        }
    }

    /// Sets the depth of labels along the camera view direction.
    #[must_use]
    pub fn with_depth(mut self, depth: f32) -> Self {
        self.depth = depth;
        self
    }

    /// Sets the render order of the label camera.
    #[must_use]
    pub fn with_camera_order(mut self, order: isize) -> Self {
        self.camera_order = order;
        self
    }
}

/// What it takes to turn a world position into a pixel of the scene camera.
struct SceneView {
    projection: Projection,
    transform: GlobalTransform,
    /// The size of the viewport in logical pixels.
    viewport: Vec2,
}

impl SceneView {
    /// The pixel of `world` relative to the center of the viewport, with y pointing up, or
    /// `None` when the point is behind the camera or outside its depth range.
    fn project(&self, world: Vec3) -> Option<Vec2> {
        let mut projection = self.projection.clone();
        projection.update(self.viewport.x, self.viewport.y);
        let in_view = self.transform.affine().inverse().transform_point3(world);
        let ndc = projection.get_clip_from_view().project_point3(in_view);
        (ndc.is_finite() && (0.0..=1.0).contains(&ndc.z))
            .then(|| ndc.truncate() * self.viewport / 2.0)
    }
}

/// Finds the camera that draws the scene: the active one with the highest order that renders
/// layer 0.
fn find_scene_view(
    cameras: &SceneCameras,
    windows: &Query<&Window, With<PrimaryWindow>>,
) -> Option<SceneView> {
    let (camera, projection, transform, _) = cameras
        .iter()
        .filter(|(camera, _, _, layers)| {
            camera.is_active && layers.is_none_or(|l| l.intersects(&RenderLayers::layer(0)))
        })
        .max_by_key(|(camera, _, _, _)| camera.order)?;
    let viewport = camera
        .logical_viewport_size()
        .or_else(|| windows.single().ok().map(Window::size))?;
    Some(SceneView {
        projection: projection.clone(),
        transform: *transform,
        viewport,
    })
}

/// The world position a label is attached to: its parent node when that exists in the view,
/// otherwise its fixed position.
fn label_world_position<S>(view: &View<S>, label: &Label) -> Option<Vec3> {
    label
        .parent
        .as_ref()
        .and_then(|parent| view.world_position(parent))
        .or(label.at)
}

/// The pixel a label is drawn at, or `None` when it cannot be seen.
fn label_pixel<S>(view: &View<S>, label: &Label, scene: Option<&SceneView>) -> Option<Vec2> {
    let world = label_world_position(view, label)?;
    Some(scene?.project(world)? + label.offset)
}

/// Keeps one `Text2d` entity for every label returned by the user's function, following the
/// parent nodes as they move, and removes labels that are gone.
fn sync_labels<S: Clone + Send + Sync + 'static>(
    layer: usize,
    depth: f32,
    wanted: &[Label],
    view: &View<S>,
    scene: Option<&SceneView>,
    mut commands: Commands,
    mut existing: LabelQuery,
) {
    let mut seen = HashSet::new();

    for (
        entity,
        marker,
        mut text,
        mut transform,
        mut visibility,
        mut color,
        mut font,
        mut render_layers,
    ) in &mut existing
    {
        let Some(label) = wanted.iter().find(|l| l.key == marker.0) else {
            commands.entity(entity).despawn();
            continue;
        };
        seen.insert(label.key.clone());

        if text.0 != label.text {
            text.0.clone_from(&label.text);
        }

        match label_pixel(view, label, scene) {
            Some(pixel) => {
                transform.translation = pixel.extend(depth);
                *visibility = Visibility::Inherited;
            }
            None => *visibility = Visibility::Hidden,
        }
        color.0 = label.color;
        font.font_size = FontSize::Px(label.font_size);
        *render_layers = RenderLayers::layer(label.layer.max(layer));
    }

    for label in wanted.iter().filter(|l| !seen.contains(&l.key)) {
        let (translation, visibility) = match label_pixel(view, label, scene) {
            Some(pixel) => (pixel.extend(depth), Visibility::Inherited),
            None => (Vec3::ZERO, Visibility::Hidden),
        };
        commands.spawn((
            LabelMarker(label.key.clone()),
            Text2d::new(label.text.clone()),
            TextFont {
                font_size: FontSize::Px(label.font_size),
                ..default()
            },
            TextColor(label.color),
            TextLayout::justify(Justify::Center),
            Anchor::CENTER,
            RenderLayers::layer(label.layer.max(layer)),
            Transform::from_translation(translation),
            visibility,
        ));
    }
}

/// Draws text labels for a view of state `S` as `Text2d` entities.
///
/// The plugin spawns a `Camera2d` that renders only the labels' layer, with one world unit per
/// logical pixel and the origin at the center of the window. Each frame it projects every
/// label's node through the scene camera to a pixel and moves the text there, so the text has
/// the pixel size it asks for in flat, isometric and perspective scenes alike. Labels follow
/// their parent nodes, update their text when the state changes, and are removed when no
/// longer returned by the labels function.
///
/// First, insert the [`LabelsResource`] with your labels function, then add this plugin.
pub struct LabelPlugin<S> {
    marker: PhantomData<fn() -> S>,
}

impl<S: Clone + Send + Sync + 'static> Default for LabelPlugin<S> {
    fn default() -> Self {
        Self {
            marker: PhantomData,
        }
    }
}

impl<S: Clone + Send + Sync + 'static> Plugin for LabelPlugin<S> {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_label_camera::<S>)
            .add_systems(Update, sync_labels_system::<S>);
    }
}

/// Spawns the `Camera2d` that draws the labels in pixels over the scene.
fn spawn_label_camera<S: Send + Sync + 'static>(
    labels_res: Res<LabelsResource<S>>,
    mut commands: Commands,
) {
    commands.spawn((
        LabelCamera,
        Camera2d,
        RenderLayers::layer(labels_res.layer),
        Camera {
            order: labels_res.camera_order,
            clear_color: ClearColorConfig::None,
            ..default()
        },
    ));
}

fn sync_labels_system<S: Clone + Send + Sync + 'static>(
    labels_res: Res<LabelsResource<S>>,
    commands: Commands,
    model: Res<DragModel<S>>,
    existing: LabelQuery,
    cameras: SceneCameras,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    let wanted = (labels_res.labels_fn)(model.state(), model.view());
    let scene = find_scene_view(&cameras, &windows);
    sync_labels(
        labels_res.layer,
        labels_res.depth,
        &wanted,
        model.view(),
        scene.as_ref(),
        commands,
        existing,
    );
}

/// A convenience function that builds labels from a view's nodes, using a naming function.
///
/// This is the common pattern: every node in the view gets a label centered on it. The
/// `name_fn` receives the node id and returns the label text, or `None` to skip that node.
pub fn labels_from_view<S: Clone + Send + Sync + 'static>(
    view: &View<S>,
    name_fn: &dyn Fn(&str) -> Option<String>,
) -> Vec<Label> {
    view.nodes()
        .iter()
        .filter_map(|node| {
            name_fn(&node.id).map(|text| {
                Label::new(format!("label-{}", node.id), text, node.id.to_string())
                    .on_layer(node.layer)
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use bevy::camera::ScalingMode;
    use bevy::transform::TransformPlugin;

    use super::*;
    use crate::GIZMO_LAYER;
    use crate::view::ViewNode;

    #[derive(Clone)]
    struct Demo {
        items: Vec<(u32, String)>,
    }

    fn demo(id: u32) -> Demo {
        Demo {
            items: vec![(id, "A".into())],
        }
    }

    fn view_fn(state: &Demo) -> View<Demo> {
        let mut view = View::new();
        for (id, _name) in &state.items {
            let snapshot = state.clone();
            view.push(
                ViewNode::rectangle(format!("node-{id}"), Vec2::new(80.0, 60.0))
                    .at_xy(Vec2::new(*id as f32 * 100.0, 0.0))
                    .draggable(move || crate::ops::state(snapshot.clone())),
            );
        }
        view
    }

    fn labels_fn(state: &Demo, _view: &View<Demo>) -> Vec<Label> {
        state
            .items
            .iter()
            .map(|(id, name)| Label::new(format!("label-{id}"), name.clone(), format!("node-{id}")))
            .collect()
    }

    #[test]
    fn label_follows_parent_node_position() {
        let view = view_fn(&Demo {
            items: vec![(0, "A".into()), (1, "B".into())],
        });
        let label = Label::new("label-0", "A", "node-0");
        let pos = label_world_position(&view, &label).unwrap();
        assert!(pos.distance(view.world_position("node-0").unwrap()) < 1e-3);
    }

    #[test]
    fn label_at_fixed_position_ignores_parent() {
        let at = Vec3::new(10.0, 20.0, 30.0);
        let label = Label::at("fixed", "Fixed", at);
        let view: View<Demo> = View::new();
        let pos = label_world_position(&view, &label).unwrap();
        assert!(pos.distance(at) < 1e-3);
    }

    #[test]
    fn label_builder_sets_color_font_size_and_offset() {
        let label = Label::new("k", "text", "p")
            .with_color(Color::srgb(1.0, 0.0, 0.0))
            .with_font_size(20.0)
            .with_offset(Vec2::new(0.0, 30.0))
            .on_layer(5);
        assert_eq!(label.color, Color::srgb(1.0, 0.0, 0.0));
        assert_eq!(label.font_size, 20.0);
        assert_eq!(label.offset, Vec2::new(0.0, 30.0));
        assert_eq!(label.layer, 5);
    }

    #[test]
    fn labels_from_view_skips_nodes_without_names() {
        let view: View<Demo> = View::new()
            .with(ViewNode::rectangle("a", Vec2::ONE).at_xy(Vec2::new(0.0, 0.0)))
            .with(ViewNode::rectangle("b", Vec2::ONE).at_xy(Vec2::new(100.0, 0.0)));
        let labels = labels_from_view(&view, &|id| {
            if id == "a" {
                Some("Node A".into())
            } else {
                None
            }
        });
        assert_eq!(labels.len(), 1);
        assert_eq!(labels[0].key, "label-a");
        assert_eq!(labels[0].text, "Node A");
        assert_eq!(labels[0].parent.as_deref(), Some("a"));
    }

    #[test]
    fn plugin_spawns_and_removes_labels() {
        let mut app = App::new();
        app.insert_resource(DragModel::new(demo(0), view_fn))
            .insert_resource(LabelsResource::<Demo>::new(labels_fn, 0))
            .add_plugins(LabelPlugin::<Demo>::default());
        app.update();
        app.update();

        let mut query = app.world_mut().query::<(&LabelMarker, &Text2d)>();
        let labels: Vec<_> = query.iter(app.world()).collect();
        assert_eq!(labels.len(), 1);
        assert_eq!(labels[0].0.0, "label-0");
        assert_eq!(labels[0].1.0, "A");

        app.world_mut()
            .resource_mut::<DragModel<Demo>>()
            .set_state(Demo { items: vec![] });
        app.update();
        app.update();

        let mut query = app.world_mut().query::<(&LabelMarker, &Text2d)>();
        let labels: Vec<_> = query.iter(app.world()).collect();
        assert_eq!(labels.len(), 0, "label should be despawned");
    }

    #[test]
    fn plugin_updates_text_when_state_changes() {
        let mut app = App::new();
        app.insert_resource(DragModel::new(demo(0), view_fn))
            .insert_resource(LabelsResource::<Demo>::new(labels_fn, 0))
            .add_plugins(LabelPlugin::<Demo>::default());
        app.update();
        app.update();

        let mut query = app.world_mut().query::<(&LabelMarker, &Text2d)>();
        let labels: Vec<_> = query.iter(app.world()).collect();
        assert_eq!(labels[0].1.0, "A");

        app.world_mut()
            .resource_mut::<DragModel<Demo>>()
            .set_state(Demo {
                items: vec![(0, "Updated".into())],
            });
        app.update();
        app.update();

        let mut query = app.world_mut().query::<(&LabelMarker, &Text2d)>();
        let labels: Vec<_> = query.iter(app.world()).collect();
        assert_eq!(labels[0].1.0, "Updated");
    }

    #[test]
    fn label_position_falls_back_to_at_when_parent_missing() {
        let view: View<Demo> = View::new();
        let mut label = Label::at("orphan", "Orphan", Vec3::new(5.0, 10.0, 0.0));
        label.parent = Some("missing".into());
        let pos = label_world_position(&view, &label).unwrap();
        assert!(pos.distance(Vec3::new(5.0, 10.0, 0.0)) < 1e-3);
    }

    #[test]
    fn label_without_a_place_has_no_world_position() {
        let view: View<Demo> = View::new();
        let mut label = Label::new("orphan", "Orphan", "missing");
        label.at = None;
        assert_eq!(label_world_position(&view, &label), None);
    }

    /// A node at x = id - 4, y = 2 on the plane z = 0.
    fn view_at(state: &Demo) -> View<Demo> {
        let id = state.items[0].0;
        let mut view = View::new();
        view.push(
            ViewNode::rectangle("node-0", Vec2::new(1.0, 1.0))
                .at_xy(Vec2::new(id as f32 - 4.0, 2.0)),
        );
        view
    }

    /// A node at the origin, 20 units in front of the plane z = 0 where the cameras look from 10.
    fn view_behind_camera(_state: &Demo) -> View<Demo> {
        let mut view = View::new();
        view.push(
            ViewNode::rectangle("node-0", Vec2::new(1.0, 1.0))
                .at_xy(Vec2::ZERO)
                .with_z(20.0),
        );
        view
    }

    fn one_label(_: &Demo, _: &View<Demo>) -> Vec<Label> {
        vec![Label::new("label-0", "A", "node-0")]
    }

    fn offset_label(_: &Demo, _: &View<Demo>) -> Vec<Label> {
        vec![Label::new("label-0", "A", "node-0").with_offset(Vec2::new(0.0, 30.0))]
    }

    fn fit_8_units() -> Projection {
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::AutoMin {
                min_width: 8.0 * 16.0 / 9.0,
                min_height: 8.0,
            },
            ..OrthographicProjection::default_3d()
        })
    }

    /// The camera of `common::spawn_cameras`: a 3D camera looking down Z with an orthographic
    /// projection, drawing layer 0 and the gizmo layer.
    fn flat_camera(commands: &mut Commands) {
        commands.spawn((
            Camera3d::default(),
            RenderLayers::from_layers(&[0, GIZMO_LAYER]),
            Camera {
                clear_color: ClearColorConfig::None,
                ..default()
            },
            fit_8_units(),
            Transform::from_xyz(0.0, 0.0, 10.0),
        ));
    }

    /// The same projection seen from a corner of the cube, like an isometric view.
    fn isometric_camera(commands: &mut Commands) {
        commands.spawn((
            Camera3d::default(),
            Camera {
                clear_color: ClearColorConfig::None,
                ..default()
            },
            fit_8_units(),
            Transform::from_xyz(10.0, 10.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
        ));
    }

    fn perspective_camera(commands: &mut Commands) {
        commands.spawn((
            Camera3d::default(),
            Camera {
                clear_color: ClearColorConfig::None,
                ..default()
            },
            Transform::from_xyz(0.0, 0.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
        ));
    }

    /// An app with the scene camera of `spawn_scene` and the screenshot overlay camera of the
    /// examples (layer 1, lower order), a window of `window` pixels and the label plugin.
    fn scene_app(
        window: (u32, u32),
        spawn_scene: fn(&mut Commands),
        state: Demo,
        view_fn: fn(&Demo) -> View<Demo>,
        labels_fn: fn(&Demo, &View<Demo>) -> Vec<Label>,
    ) -> App {
        let mut app = App::new();
        app.add_plugins((
            WindowPlugin {
                primary_window: Some(Window {
                    resolution: window.into(),
                    ..default()
                }),
                ..default()
            },
            TransformPlugin,
        ));
        app.add_systems(Startup, move |mut commands: Commands| {
            commands.spawn((
                Camera2d,
                RenderLayers::layer(1),
                Camera {
                    order: -1,
                    ..default()
                },
                Transform::from_xyz(0.0, 0.0, 100.0),
            ));
            spawn_scene(&mut commands);
        });
        app.insert_resource(DragModel::new(state, view_fn))
            .insert_resource(LabelsResource::<Demo>::new(labels_fn, 3))
            .add_plugins(LabelPlugin::<Demo>::default());
        app.update();
        app.update();
        app
    }

    /// The translation and visibility of the only label.
    fn the_label(app: &mut App) -> (Vec3, Visibility) {
        let mut query = app
            .world_mut()
            .query::<(&LabelMarker, &Transform, &Visibility)>();
        let labels: Vec<_> = query.iter(app.world()).collect();
        assert_eq!(labels.len(), 1, "exactly one label should exist");
        (labels[0].1.translation, *labels[0].2)
    }

    fn assert_pixel(actual: Vec3, expected: Vec2) {
        assert!(
            actual.truncate().distance(expected) < 0.5,
            "label is at {:?} pixels but should be at {expected:?}",
            actual.truncate()
        );
    }

    fn set_item(app: &mut App, id: u32) {
        app.world_mut()
            .resource_mut::<DragModel<Demo>>()
            .set_state(demo(id));
        app.update();
        app.update();
    }

    #[test]
    fn label_sits_on_its_node_in_pixels_for_a_flat_camera() {
        // 8 world units over 720 pixels is 90 pixels a unit, so (-4, 2) is (-360, 180).
        let mut app = scene_app((1280, 720), flat_camera, demo(0), view_at, one_label);
        let (translation, visibility) = the_label(&mut app);
        assert_ne!(visibility, Visibility::Hidden);
        assert_pixel(translation, Vec2::new(-360.0, 180.0));
    }

    #[test]
    fn label_position_follows_the_window_size() {
        // A square window shows 14.22 units across 800 pixels, 56.25 pixels a unit.
        let mut app = scene_app((800, 800), flat_camera, demo(0), view_at, one_label);
        let (translation, _) = the_label(&mut app);
        assert_pixel(translation, Vec2::new(-225.0, 112.5));
    }

    #[test]
    fn label_moves_with_its_node() {
        let mut app = scene_app((1280, 720), flat_camera, demo(4), view_at, one_label);
        assert_pixel(the_label(&mut app).0, Vec2::new(0.0, 180.0));

        set_item(&mut app, 5);
        assert_pixel(the_label(&mut app).0, Vec2::new(90.0, 180.0));

        set_item(&mut app, 2);
        assert_pixel(the_label(&mut app).0, Vec2::new(-180.0, 180.0));
    }

    #[test]
    fn label_offset_is_in_pixels_and_does_not_scale_with_the_scene() {
        let mut app = scene_app((1280, 720), flat_camera, demo(0), view_at, offset_label);
        assert_pixel(the_label(&mut app).0, Vec2::new(-360.0, 210.0));
        let mut app = scene_app((800, 800), flat_camera, demo(0), view_at, offset_label);
        assert_pixel(the_label(&mut app).0, Vec2::new(-225.0, 142.5));
    }

    #[test]
    fn label_text_has_the_font_size_in_pixels() {
        let mut app = scene_app((1280, 720), flat_camera, demo(0), view_at, one_label);
        let mut query = app
            .world_mut()
            .query::<(&TextFont, &Transform, &RenderLayers)>();
        let labels: Vec<_> = query.iter(app.world()).collect();
        assert_eq!(labels.len(), 1);
        assert_eq!(labels[0].0.font_size, FontSize::Px(DEFAULT_LABEL_SIZE));
        assert_eq!(labels[0].1.scale, Vec3::ONE);
        assert!(labels[0].2.intersects(&RenderLayers::layer(3)));
        assert!(!labels[0].2.intersects(&RenderLayers::layer(0)));
    }

    #[test]
    fn label_camera_draws_pixels_on_the_label_layer_only() {
        let mut app = scene_app((1280, 720), flat_camera, demo(0), view_at, one_label);
        let mut query = app.world_mut().query::<(
            &LabelCamera,
            &Camera,
            &Projection,
            &Transform,
            &RenderLayers,
        )>();
        let cameras: Vec<_> = query.iter(app.world()).collect();
        assert_eq!(cameras.len(), 1, "one label camera should exist");
        let (_, camera, projection, transform, layers) = cameras[0];
        let Projection::Orthographic(ortho) = projection else {
            panic!("the label camera should be orthographic");
        };
        assert!(matches!(ortho.scaling_mode, ScalingMode::WindowSize));
        assert_eq!(ortho.scale, 1.0);
        assert_eq!(transform.translation, Vec3::ZERO);
        assert!(camera.order > 0, "labels should draw over the scene");
        assert!(layers.intersects(&RenderLayers::layer(3)));
        assert!(!layers.intersects(&RenderLayers::layer(0)));
        assert!(!layers.intersects(&RenderLayers::layer(1)));
        assert!(
            DEFAULT_LABEL_DEPTH.abs() < 1000.0,
            "labels must be inside the camera's depth range"
        );
    }

    #[test]
    fn label_sits_on_its_node_for_an_isometric_camera() {
        // The camera at (10, 10, 10) has right axis (1, 0, -1) / sqrt(2) and up axis
        // (-1, 2, -1) / sqrt(6). The node at (1, 2, 0) is 0.7071 right and 1.2247 up, at
        // 90 pixels a unit.
        let mut app = scene_app((1280, 720), isometric_camera, demo(5), view_at, one_label);
        let (translation, visibility) = the_label(&mut app);
        assert_ne!(visibility, Visibility::Hidden);
        assert_pixel(translation, Vec2::new(63.64, 110.23));
    }

    #[test]
    fn label_sits_on_its_node_for_a_perspective_camera() {
        let mut app = scene_app((1280, 720), perspective_camera, demo(4), view_at, one_label);
        let (translation, visibility) = the_label(&mut app);
        assert_ne!(visibility, Visibility::Hidden);
        // Node (0, 2, 0) is straight above the look target.
        assert!(translation.x.abs() < 0.5);
        assert!(translation.y > 10.0 && translation.y < 360.0);
    }

    #[test]
    fn label_is_hidden_when_its_node_is_behind_the_camera() {
        let mut app = scene_app(
            (1280, 720),
            perspective_camera,
            demo(0),
            view_behind_camera,
            one_label,
        );
        assert_eq!(the_label(&mut app).1, Visibility::Hidden);
    }

    #[test]
    fn label_is_hidden_when_there_is_no_scene_camera() {
        fn no_camera(_: &mut Commands) {}
        let mut app = scene_app((1280, 720), no_camera, demo(0), view_at, one_label);
        assert_eq!(the_label(&mut app).1, Visibility::Hidden);
    }

    #[test]
    fn label_is_hidden_when_its_node_is_behind_an_orthographic_camera() {
        let mut app = scene_app(
            (1280, 720),
            flat_camera,
            demo(0),
            view_behind_camera,
            one_label,
        );
        // The camera at z = 10 looks down -Z, so a node at z = 20 is behind it.
        assert_eq!(the_label(&mut app).1, Visibility::Hidden);
    }
}
