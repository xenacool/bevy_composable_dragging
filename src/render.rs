//! Keeps Bevy entities in sync with the view of the [`DragModel`].

use std::collections::{HashMap, HashSet};
use std::marker::PhantomData;

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;

use crate::geometry::Geometry;
use crate::model::DragModel;
use crate::rendering::Rendering;
use crate::shapes::Looks;
use crate::view::{NodeId, ViewNode};

/// Marks the entity that renders the view node with the given id.
#[derive(Component)]
pub(crate) struct DragNode<S: 'static> {
    pub(crate) id: NodeId,
    parent: Option<NodeId>,
    geometry: Geometry,
    color: LinearRgba,
    #[cfg(feature = "gradmesh")]
    style: Option<bevy_gradmesh::Style>,
    /// The opacity the node was drawn with, multiplied through its ancestors.
    opacity: f32,
    casts_shadow: bool,
    /// Whether the pointer can hit the entity.
    pickable: bool,
    /// The rendering mode the entity was drawn with.
    mode: Rendering,
    /// The render layer of the entity.
    layer: usize,
    /// Whether the entity is hidden because the preview removed its node during a drag.
    hidden: bool,
    state: PhantomData<fn() -> S>,
}

/// Maps node ids to the entities that render them.
#[derive(Resource)]
pub(crate) struct NodeEntities<S: 'static>(HashMap<NodeId, Entity>, PhantomData<fn() -> S>);

impl<S> Default for NodeEntities<S> {
    fn default() -> Self {
        Self(HashMap::new(), PhantomData)
    }
}

/// The picking behavior of a node: hit as usual, or ignored by the pointer altogether.
fn pickable(pickable: bool) -> Pickable {
    if pickable {
        Pickable::default()
    } else {
        Pickable::IGNORE
    }
}

/// Whether the gradient style of `node` differs from the one it was drawn with.
fn style_changed<S: 'static>(marker: &DragNode<S>, node: &ViewNode<S>) -> bool {
    #[cfg(feature = "gradmesh")]
    return marker.style != node.style;
    #[cfg(not(feature = "gradmesh"))]
    {
        let _ = (marker, node);
        false
    }
}

/// Spawns, updates and despawns entities so they show the current view.
pub(crate) fn sync_nodes<S: Clone + Send + Sync + 'static>(
    mut commands: Commands,
    model: Res<DragModel<S>>,
    mut entities: ResMut<NodeEntities<S>>,
    mut looks: Looks,
    mut nodes: Query<(&mut DragNode<S>, &mut Transform)>,
) {
    if !model.is_changed() {
        return;
    }
    let view = model.view();
    let opacities = view.effective_opacities();
    let ids: HashSet<&str> = view.nodes().iter().map(|node| node.id.as_ref()).collect();
    let mut reparent: Vec<(Entity, Option<NodeId>)> = Vec::new();

    for node in view.nodes() {
        let opacity = opacities[node.id.as_ref()];
        let parent = node
            .parent
            .clone()
            .filter(|parent| ids.contains(parent.as_ref()));
        let mode = looks.mode_of(node.rendering);
        if let Some(entity) = entities.0.get(&node.id).copied() {
            let Ok((mut marker, mut transform)) = nodes.get_mut(entity) else {
                continue;
            };
            if *transform != node.transform {
                *transform = node.transform;
            }
            if marker.hidden {
                marker.hidden = false;
                commands.entity(entity).insert(Visibility::Inherited);
            }
            if marker.geometry != node.geometry
                || marker.color != node.color
                || style_changed(&marker, node)
                || (marker.opacity - opacity).abs() > f32::EPSILON
                || marker.casts_shadow != node.casts_shadow
                || marker.pickable != node.pickable
                || marker.mode != mode
                || marker.layer != node.layer
            {
                let mut entity_commands = commands.entity(entity);
                if marker.geometry != node.geometry || marker.mode != mode {
                    looks.apply_mesh(&mut entity_commands, &node.geometry, mode);
                }
                if marker.layer != node.layer {
                    entity_commands.insert(RenderLayers::layer(node.layer));
                }
                looks.apply_material(&mut entity_commands, node, opacity, mode);
                marker.geometry.clone_from(&node.geometry);
                marker.color = node.color;
                marker.opacity = opacity;
                marker.casts_shadow = node.casts_shadow;
                if marker.pickable != node.pickable {
                    entity_commands.insert(pickable(node.pickable));
                    marker.pickable = node.pickable;
                }
                marker.mode = mode;
                marker.layer = node.layer;
                #[cfg(feature = "gradmesh")]
                marker.style.clone_from(&node.style);
            }
            if marker.parent != parent {
                marker.parent.clone_from(&parent);
                reparent.push((entity, parent));
            }
        } else {
            let mut spawned = commands.spawn((
                Name::new(node.id.to_string()),
                node.transform,
                DragNode::<S> {
                    id: node.id.clone(),
                    parent: parent.clone(),
                    geometry: node.geometry.clone(),
                    color: node.color,
                    #[cfg(feature = "gradmesh")]
                    style: node.style.clone(),
                    opacity,
                    casts_shadow: node.casts_shadow,
                    pickable: node.pickable,
                    mode,
                    layer: node.layer,
                    hidden: false,
                    state: PhantomData,
                },
                RenderLayers::layer(node.layer),
                pickable(node.pickable),
            ));
            looks.apply_mesh(&mut spawned, &node.geometry, mode);
            looks.apply_material(&mut spawned, node, opacity, mode);
            let entity = spawned.id();
            entities.0.insert(node.id.clone(), entity);
            if parent.is_some() {
                reparent.push((entity, parent));
            }
        }
    }

    // Re-parenting happens before despawning so children of removed nodes are not despawned too.
    for (entity, parent) in reparent {
        match parent.and_then(|parent| entities.0.get(&parent).copied()) {
            Some(parent) => commands.entity(entity).insert(ChildOf(parent)),
            None => commands.entity(entity).remove::<ChildOf>(),
        };
    }
    // The entity being dragged keeps receiving pointer events, so it must outlive a preview that
    // removes its node (for example while hovering a trash bin). It is hidden instead.
    let dragged = model.dragged_node();
    entities.0.retain(|id, entity| {
        if ids.contains(id.as_ref()) {
            return true;
        }
        if dragged == Some(id.as_ref()) {
            if let Ok((mut marker, _)) = nodes.get_mut(*entity)
                && !marker.hidden
            {
                marker.hidden = true;
                commands.entity(*entity).insert(Visibility::Hidden);
            }
            return true;
        }
        // A node whose ancestor is removed too goes down with it, so its entity may already be gone.
        commands.entity(*entity).try_despawn();
        false
    });
}

#[cfg(all(test, feature = "gradmesh"))]
mod tests {
    use bevy::asset::AssetPlugin;
    use bevy::sprite_render::{ColorMaterial, MeshMaterial2d};
    use bevy_gradmesh::{
        GradMeshMaterial, GradMeshMaterial2d, GradMeshMaterialPool, SdfMaterial2d,
    };

    use super::*;
    use crate::geometry::MeshGeometry;
    use crate::sdf::SdfShape;
    use crate::shapes::ShapeMeshes;
    use crate::view::View;

    fn app(rendering: Rendering) -> App {
        app_with(rendering, || {
            let mut view = View::new();
            view.push(ViewNode::rectangle("flat", Vec2::ONE));
            view.push(ViewNode::sphere("round", 0.5));
            view
        })
    }

    fn app_with(rendering: Rendering, build: fn() -> View<()>) -> App {
        let mut app = App::new();
        app.add_plugins(AssetPlugin::default())
            .init_asset::<Mesh>()
            .init_asset::<StandardMaterial>()
            .init_asset::<GradMeshMaterial>()
            .init_asset::<ColorMaterial>()
            .init_asset::<GradMeshMaterial2d>()
            .init_asset::<SdfMaterial2d>()
            .init_resource::<GradMeshMaterialPool>()
            .init_resource::<ShapeMeshes>()
            .init_resource::<NodeEntities<()>>()
            .insert_resource(rendering)
            .insert_resource(DragModel::new((), move |()| build()))
            .add_systems(Update, sync_nodes::<()>);
        app.update();
        app
    }

    fn entity_of(app: &mut App, id: &str) -> Entity {
        let name = Name::new(id.to_string());
        app.world_mut()
            .query::<(Entity, &Name)>()
            .iter(app.world())
            .find_map(|(entity, candidate)| (*candidate == name).then_some(entity))
            .expect("the node is rendered")
    }

    #[test]
    fn spatial_rendering_draws_3d_meshes() {
        let mut app = app(Rendering::Spatial);
        for id in ["flat", "round"] {
            let entity = entity_of(&mut app, id);
            let entity = app.world().entity(entity);
            assert!(entity.contains::<Mesh3d>());
            assert!(!entity.contains::<Mesh2d>());
        }
        let round = entity_of(&mut app, "round");
        assert!(
            app.world()
                .entity(round)
                .contains::<MeshMaterial3d<GradMeshMaterial>>()
        );
    }

    #[test]
    fn flat_rendering_draws_2d_meshes() {
        let mut app = app(Rendering::Flat);
        for id in ["flat", "round"] {
            let entity = entity_of(&mut app, id);
            let entity = app.world().entity(entity);
            assert!(entity.contains::<Mesh2d>());
            assert!(!entity.contains::<Mesh3d>());
        }
        let flat = entity_of(&mut app, "flat");
        let round = entity_of(&mut app, "round");
        assert!(
            app.world()
                .entity(flat)
                .contains::<MeshMaterial2d<GradMeshMaterial2d>>()
                || app
                    .world()
                    .entity(flat)
                    .contains::<MeshMaterial2d<ColorMaterial>>()
        );
        assert!(
            app.world()
                .entity(round)
                .contains::<MeshMaterial2d<GradMeshMaterial2d>>()
        );
    }

    fn panel_view() -> View<()> {
        View::new().with(ViewNode::sdf(
            "panel",
            SdfShape::rect(Vec2::new(40.0, 20.0)).with_corner_radius(4.0),
        ))
    }

    #[test]
    fn an_sdf_node_is_drawn_flat_on_a_quad_with_room_for_its_fringe() {
        let mut app = app_with(Rendering::Flat, panel_view);
        let entity = entity_of(&mut app, "panel");
        let entity = app.world().entity(entity);
        assert!(entity.contains::<MeshMaterial2d<SdfMaterial2d>>());
        assert!(!entity.contains::<MeshMaterial2d<ColorMaterial>>());
        let handle = entity.get::<Mesh2d>().unwrap().0.id();
        let mesh = app.world().resource::<Assets<Mesh>>().get(handle).unwrap();
        let positions = mesh
            .attribute(Mesh::ATTRIBUTE_POSITION)
            .and_then(|positions| positions.as_float3())
            .unwrap();
        let width = positions.iter().map(|p| p[0]).fold(f32::MIN, f32::max)
            - positions.iter().map(|p| p[0]).fold(f32::MAX, f32::min);
        assert!((width - 44.0).abs() < 1e-4, "{width}");
    }

    #[test]
    fn a_long_animation_does_not_pile_up_gradient_materials() {
        use std::sync::atomic::{AtomicU32, Ordering};

        static STEP: AtomicU32 = AtomicU32::new(0);
        fn view() -> View<()> {
            let hue = STEP.load(Ordering::Relaxed) as f32;
            View::new().with(ViewNode::sphere("orb", 0.5).with_color(Color::hsl(
                hue * 3.0 % 360.0,
                0.8,
                0.5,
            )))
        }
        let mut app = app_with(Rendering::Spatial, view);
        for step in 1..=120 {
            STEP.store(step, Ordering::Relaxed);
            app.world_mut()
                .resource_mut::<DragModel<()>>()
                .set_state(());
            app.update();
        }
        app.update();
        let count = app.world().resource::<Assets<GradMeshMaterial>>().len();
        assert!(count <= 4, "{count} materials are alive");
        let orb = entity_of(&mut app, "orb");
        assert!(
            app.world()
                .entity(orb)
                .contains::<MeshMaterial3d<GradMeshMaterial>>()
        );
    }

    #[test]
    fn a_node_the_pointer_cannot_hit_ignores_it_and_can_change_its_mind() {
        use std::sync::atomic::{AtomicBool, Ordering};

        static HITTABLE: AtomicBool = AtomicBool::new(false);
        fn view() -> View<()> {
            View::new()
                .with(ViewNode::sphere("label", 0.5).pickable(HITTABLE.load(Ordering::Relaxed)))
        }
        let mut app = app_with(Rendering::Spatial, view);
        let label = entity_of(&mut app, "label");
        assert_eq!(app.world().get::<Pickable>(label), Some(&Pickable::IGNORE));
        HITTABLE.store(true, Ordering::Relaxed);
        app.world_mut()
            .resource_mut::<DragModel<()>>()
            .set_state(());
        app.update();
        assert_eq!(
            app.world().get::<Pickable>(label),
            Some(&Pickable::default())
        );
    }

    #[test]
    fn a_parent_and_its_child_can_leave_the_view_together() {
        use std::sync::atomic::{AtomicBool, Ordering};

        static GONE: AtomicBool = AtomicBool::new(false);
        fn view() -> View<()> {
            let mut view = View::new();
            if !GONE.load(Ordering::Relaxed) {
                for pair in 0..24 {
                    view.push(ViewNode::sphere(format!("parent{pair}"), 0.5));
                    view.push(
                        ViewNode::sphere(format!("child{pair}"), 0.25)
                            .with_parent(format!("parent{pair}")),
                    );
                }
            }
            view
        }
        let mut app = app_with(Rendering::Spatial, view);
        assert_eq!(
            app.world_mut()
                .query::<&DragNode<()>>()
                .iter(app.world())
                .count(),
            48
        );
        GONE.store(true, Ordering::Relaxed);
        app.world_mut()
            .resource_mut::<DragModel<()>>()
            .set_state(());
        app.update();
        app.update();
        assert_eq!(
            app.world_mut()
                .query::<&DragNode<()>>()
                .iter(app.world())
                .count(),
            0
        );
    }

    fn terrain_view() -> View<()> {
        let mut view = View::new();
        view.push(ViewNode::mesh(
            "terrain",
            MeshGeometry::new(1, Mesh::from(Rectangle::new(4.0, 4.0))),
        ));
        view
    }

    #[test]
    fn a_mesh_of_your_own_is_drawn_in_both_modes_with_the_gradient_material() {
        let mut spatial = app_with(Rendering::Spatial, terrain_view);
        let entity = entity_of(&mut spatial, "terrain");
        let entity = spatial.world().entity(entity);
        assert!(entity.contains::<Mesh3d>() && !entity.contains::<Mesh2d>());
        assert!(entity.contains::<MeshMaterial3d<GradMeshMaterial>>());

        let mut flat = app_with(Rendering::Flat, terrain_view);
        let entity = entity_of(&mut flat, "terrain");
        let entity = flat.world().entity(entity);
        assert!(entity.contains::<Mesh2d>() && !entity.contains::<Mesh3d>());
        assert!(entity.contains::<MeshMaterial2d<GradMeshMaterial2d>>());
    }

    #[test]
    fn meshes_of_your_own_share_an_asset_by_key_and_a_new_key_frees_the_old_one() {
        let mesh =
            |key| Geometry::Mesh(MeshGeometry::new(key, Mesh::from(Rectangle::new(1.0, 1.0))));
        let mut shapes = ShapeMeshes::default();
        let mut meshes = Assets::<Mesh>::default();
        let first = shapes.get(&mesh(1), false, &mut meshes);
        let again = shapes.get(&mesh(1), false, &mut meshes);
        assert_eq!(first.id(), again.id(), "the same key shares the mesh");
        let flat = shapes.get(&mesh(1), true, &mut meshes);
        assert_ne!(
            first.id(),
            flat.id(),
            "flat and spatial copies are separate"
        );
        let other = shapes.get(&mesh(2), false, &mut meshes);
        assert_ne!(first.id(), other.id());
        let old = first.id();
        drop((first, again));
        meshes.remove(old);
        let rebuilt = shapes.get(&mesh(1), false, &mut meshes);
        assert_ne!(rebuilt.id(), old, "a freed mesh is built again when needed");
        assert!(meshes.contains(rebuilt.id()));
    }

    fn hud_over_scene() -> View<()> {
        let mut view = View::new();
        view.push(ViewNode::sphere("scene", 0.5));
        view.push(ViewNode::disc("hud", 0.5).flat().on_layer(2));
        view.push(ViewNode::rectangle("hud-child", Vec2::ONE).with_parent("hud"));
        view
    }

    #[test]
    fn a_node_overrides_the_rendering_mode_of_the_plugin() {
        let mut app = app_with(Rendering::Spatial, hud_over_scene);
        let scene = entity_of(&mut app, "scene");
        let hud = entity_of(&mut app, "hud");
        let child = entity_of(&mut app, "hud-child");
        let world = app.world();
        assert!(world.entity(scene).contains::<Mesh3d>());
        assert!(!world.entity(scene).contains::<Mesh2d>());
        assert!(world.entity(hud).contains::<Mesh2d>());
        assert!(!world.entity(hud).contains::<Mesh3d>());
        assert!(
            world
                .entity(hud)
                .contains::<MeshMaterial2d<GradMeshMaterial2d>>()
        );
        assert!(
            !world
                .entity(hud)
                .contains::<MeshMaterial3d<GradMeshMaterial>>()
        );
        // A node without an override follows the plugin, whatever its parent does.
        assert!(world.entity(child).contains::<Mesh3d>());
    }

    #[test]
    fn a_flat_plugin_default_can_be_overridden_by_a_spatial_node() {
        fn view() -> View<()> {
            let mut view = View::new();
            view.push(ViewNode::sphere("flat", 0.5));
            view.push(ViewNode::sphere("solid", 0.5).spatial());
            view
        }
        let mut app = app_with(Rendering::Flat, view);
        let flat = entity_of(&mut app, "flat");
        let solid = entity_of(&mut app, "solid");
        assert!(app.world().entity(flat).contains::<Mesh2d>());
        assert!(app.world().entity(solid).contains::<Mesh3d>());
        assert!(!app.world().entity(solid).contains::<Mesh2d>());
    }

    #[test]
    fn nodes_get_the_render_layer_they_ask_for() {
        let mut app = app_with(Rendering::Spatial, hud_over_scene);
        for (id, layer) in [("scene", 0), ("hud", 2)] {
            let entity = entity_of(&mut app, id);
            assert_eq!(
                app.world().entity(entity).get::<RenderLayers>(),
                Some(&RenderLayers::layer(layer)),
                "{id}"
            );
        }
    }

    #[test]
    fn flat_and_spatial_nodes_of_one_shape_get_their_own_meshes() {
        fn view() -> View<()> {
            let mut view = View::new();
            view.push(ViewNode::sphere("a", 0.5));
            view.push(ViewNode::sphere("b", 0.5).flat());
            view
        }
        let mut app = app_with(Rendering::Spatial, view);
        let a = entity_of(&mut app, "a");
        let b = entity_of(&mut app, "b");
        let meshes = app.world().resource::<Assets<Mesh>>();
        let spatial = &app.world().entity(a).get::<Mesh3d>().unwrap().0;
        let flat = &app.world().entity(b).get::<Mesh2d>().unwrap().0;
        assert_ne!(spatial.id(), flat.id());
        assert!(meshes.contains(spatial.id()) && meshes.contains(flat.id()));
    }
}
