//! Keeps Bevy entities in sync with the view of the [`DragModel`].

use std::collections::{HashMap, HashSet};
use std::marker::PhantomData;

use bevy::prelude::*;

use crate::model::DragModel;
use crate::shapes::Looks;
use crate::view::{NodeId, ShapeKey};

/// Marks the entity that renders the view node with the given id.
#[derive(Component)]
pub(crate) struct DragNode<S: 'static> {
    pub(crate) id: NodeId,
    parent: Option<NodeId>,
    shape: ShapeKey,
    color: LinearRgba,
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
    let ids: HashSet<&str> = view.nodes().iter().map(|node| node.id.as_ref()).collect();
    let mut reparent: Vec<(Entity, Option<NodeId>)> = Vec::new();

    for node in view.nodes() {
        let parent = node
            .parent
            .clone()
            .filter(|parent| ids.contains(parent.as_ref()));
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
            if marker.shape != node.shape
                || marker.color != node.color
                || Looks::has_own_style(node)
            {
                let mut entity_commands = commands.entity(entity);
                if marker.shape != node.shape {
                    entity_commands.insert(looks.mesh(node.shape));
                }
                looks.apply_material(&mut entity_commands, node);
                marker.shape = node.shape;
                marker.color = node.color;
            }
            if marker.parent != parent {
                marker.parent.clone_from(&parent);
                reparent.push((entity, parent));
            }
        } else {
            let mut spawned = commands.spawn((
                Name::new(node.id.to_string()),
                looks.mesh(node.shape),
                node.transform,
                DragNode::<S> {
                    id: node.id.clone(),
                    parent: parent.clone(),
                    shape: node.shape,
                    color: node.color,
                    hidden: false,
                    state: PhantomData,
                },
            ));
            looks.apply_material(&mut spawned, node);
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
        commands.entity(*entity).despawn();
        false
    });
}
