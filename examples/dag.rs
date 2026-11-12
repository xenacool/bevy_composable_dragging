//! A directed acyclic graph: the `substate` zipper focuses on one part of the graph at a time.
//!
//! Dragging a node focuses on that node's position; dragging an arrowhead focuses on the one
//! end of its edge. A ring marks the focus while everything else stays in view and draggable.
//! An arrowhead snaps to the nodes it can point at without closing a cycle and floats freely
//! when far from them; dropped loose, the edge is deleted.

use bevy::prelude::*;
use bevy_composable_dragging::label::{Label, LabelPlugin, LabelsResource};
use bevy_composable_dragging::ops::{Lens, closest_states, substate, vary};
use bevy_composable_dragging::{DragSpec, DraggablePlugin, Frame, Param, View, ViewNode, param};

mod common;
use common::screenshot::AutoScreenshotPlugin;
use common::script::ScriptedDragPlugin;

const NODE_RADIUS: f32 = 0.5;
const ARROW_LENGTH: f32 = 0.5;
/// How far the pointer may be from a node's snap position before the arrowhead floats free.
const LOOSE_GAP: f32 = 1.0;

/// Where the head of an edge points.
#[derive(Clone, Copy, Debug, PartialEq)]
enum End {
    Node(usize),
    Free(Vec2),
}

/// An edge from a node to an end.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Edge {
    from: usize,
    to: End,
}

/// Node positions and directed edges.
#[derive(Clone, Debug, PartialEq)]
struct Dag {
    nodes: Vec<Vec2>,
    edges: Vec<Edge>,
}

impl Dag {
    /// Whether pointing `edge` at `target` would let the graph reach itself.
    fn closes_a_cycle(&self, edge: usize, target: usize) -> bool {
        let source = self.edges[edge].from;
        let mut pending = vec![target];
        let mut seen = vec![false; self.nodes.len()];
        while let Some(node) = pending.pop() {
            if node == source {
                return true;
            }
            if !std::mem::replace(&mut seen[node], true) {
                for (index, other) in self.edges.iter().enumerate() {
                    if let (true, End::Node(next)) = (index != edge && other.from == node, other.to)
                    {
                        pending.push(next);
                    }
                }
            }
        }
        false
    }

    /// The point the head of `edge` touches and the direction it points in.
    fn head(&self, edge: usize) -> (Vec2, Vec2) {
        let Edge { from, to } = self.edges[edge];
        let from = self.nodes[from];
        let tip = match to {
            End::Free(point) => point,
            End::Node(node) => {
                self.nodes[node] - (self.nodes[node] - from).normalize_or_zero() * NODE_RADIUS
            }
        };
        (tip, (tip - from).try_normalize().unwrap_or(Vec2::X))
    }

    /// Where a finger grabs the arrowhead of `edge`.
    fn grip(&self, edge: usize) -> Vec2 {
        let (tip, direction) = self.head(edge);
        tip - direction * ARROW_LENGTH * 0.4
    }
}

/// A ring that marks the node `id` as the focus of the drag.
fn focus_ring(id: String, radius: f32) -> impl Fn(&Frame<Dag>) -> Vec<ViewNode<Dag>> + Clone {
    move |frame| {
        frame
            .view
            .world_position(&id)
            .map(|at| {
                vec![
                    ViewNode::ring("focus", radius, radius * 0.88)
                        .at_xy(at.truncate())
                        .with_color(Color::srgb(1.0, 0.85, 0.2)),
                ]
            })
            .unwrap_or_default()
    }
}

/// Moves one node: the spec only sees the node's position.
fn node_spec(dag: &Dag, index: usize) -> DragSpec<Dag> {
    let position = Lens::new(
        move |dag: &Dag| dag.nodes[index],
        move |dag: &Dag, position| {
            let mut dag = dag.clone();
            dag.nodes[index] = position;
            dag
        },
    );
    let free = vary(dag.nodes[index], [param!(Vec2, x), param!(Vec2, y)]);
    substate(position, free).with_overlay(focus_ring(format!("node-{index}"), 0.8))
}

/// Retargets one edge: the spec only sees where the head points.
fn head_spec(dag: &Dag, index: usize) -> DragSpec<Dag> {
    let head = Lens::new(
        move |dag: &Dag| dag.edges[index].to,
        move |dag: &Dag, to| {
            let mut dag = dag.clone();
            dag.edges[index].to = to;
            dag
        },
    );
    let targets = (0..dag.nodes.len())
        .filter(|&node| node != dag.edges[index].from && !dag.closes_a_cycle(index, node))
        .map(End::Node);
    let loose = vary(
        End::Free(dag.head(index).0),
        [
            Param::new(
                |end: &End| if let End::Free(at) = end { at.x } else { 0.0 },
                |end, x| {
                    if let End::Free(at) = end {
                        at.x = x;
                    }
                },
            ),
            Param::new(
                |end: &End| if let End::Free(at) = end { at.y } else { 0.0 },
                |end, y| {
                    if let End::Free(at) = end {
                        at.y = y;
                    }
                },
            ),
        ],
    );
    substate(head, closest_states(targets).when_far(loose, LOOSE_GAP))
        .with_overlay(focus_ring(format!("arrow-{index}"), 0.5))
        .on_drop_with(|dag| Dag {
            edges: dag
                .edges
                .iter()
                .filter(|edge| matches!(edge.to, End::Node(_)))
                .copied()
                .collect(),
            ..dag.clone()
        })
}

fn arrowhead() -> bevy_gradmesh::Shape {
    let half = ARROW_LENGTH * 0.5;
    bevy_gradmesh::Shape::arbitrary_polygon(
        vec![
            Vec2::new(0.0, -half),
            Vec2::new(0.0, half),
            Vec2::new(ARROW_LENGTH, 0.0),
        ],
        bevy_gradmesh::ShapeDetail::Low,
    )
}

fn view(dag: &Dag) -> View<Dag> {
    let mut view = View::new();
    let arrow = arrowhead();
    let steel = Color::srgb(0.4, 0.45, 0.55);

    for (index, edge) in dag.edges.iter().enumerate() {
        let (tip, direction) = dag.head(index);
        let from = dag.nodes[edge.from];
        let base = tip - direction * ARROW_LENGTH;
        view.push(
            ViewNode::rectangle(
                format!("edge-{index}"),
                Vec2::new(from.distance(base).max(0.01), 0.06),
            )
            .at_xy((from + base) / 2.0)
            .with_rotation_xy((base - from).to_angle())
            .with_color(steel),
        );
        let snapshot = dag.clone();
        view.push(
            ViewNode::grad(format!("arrow-{index}"), arrow.clone())
                .at_xy(base)
                .with_z(0.05)
                .with_rotation_xy(direction.to_angle())
                .with_color(steel)
                .draggable(move || head_spec(&snapshot, index)),
        );
    }

    for (index, &position) in dag.nodes.iter().enumerate() {
        let snapshot = dag.clone();
        view.push(
            ViewNode::circle(format!("node-{index}"), NODE_RADIUS * 2.0)
                .at_xy(position)
                .with_z(0.1)
                .with_color(Color::srgb(0.15, 0.15, 0.2))
                .draggable(move || node_spec(&snapshot, index)),
        );
    }
    view
}

fn initial() -> Dag {
    let to = |from, node| Edge {
        from,
        to: End::Node(node),
    };
    Dag {
        nodes: vec![
            Vec2::new(-4.0, 2.0),
            Vec2::new(-1.0, 3.0),
            Vec2::new(-1.0, 0.5),
            Vec2::new(2.0, 2.0),
            Vec2::new(4.5, 1.0),
        ],
        edges: vec![to(0, 1), to(0, 2), to(1, 3), to(2, 3), to(3, 4)],
    }
}

/// The render layer of the labels, which only the label camera draws.
const LABEL_LAYER: usize = 3;

/// Label names for the DAG nodes, in order.
const NODE_NAMES: [&str; 5] = ["A", "B", "C", "D", "E"];

/// Returns a text label for each node in the DAG.
fn dag_labels(dag: &Dag, _view: &View<Dag>) -> Vec<Label> {
    dag.nodes
        .iter()
        .enumerate()
        .map(|(index, _position)| {
            Label::new(
                format!("label-{index}"),
                NODE_NAMES[index],
                format!("node-{index}"),
            )
        })
        .collect()
}

/// Run this example. Called by the WASM runner.
pub fn run(app: &mut App) {
    let dag = initial();
    // Carry the head of the edge from node 1 over to node 4.
    let mut retargeted = dag.clone();
    retargeted.edges[2].to = End::Node(4);
    app.add_plugins(common::default_plugins("dag"))
        .add_plugins(DraggablePlugin::new(dag.clone(), view))
        .insert_resource(LabelsResource::<Dag>::new(dag_labels, LABEL_LAYER))
        .add_plugins(LabelPlugin::<Dag>::default())
        .add_plugins(ScriptedDragPlugin::straight(
            dag.grip(2),
            retargeted.grip(2),
        ))
        .add_plugins(
            AutoScreenshotPlugin::new("screenshot_dag.gif")
                .with_title("dag")
                .with_description("substate: an arrowhead focuses on one end of one edge")
                .animation(600),
        )
        .add_systems(Startup, |mut commands: Commands| {
            common::spawn_cameras(&mut commands, 8.0);
        });
}

fn main() {
    let mut app = App::new();
    run(&mut app);
    app.run();
}

#[cfg(test)]
mod tests {
    use bevy_composable_dragging::{DragContext, NodeId, Pointer};

    use super::*;

    /// The frame of dragging `id` of the initial graph with the pointer at `pointer`.
    fn drag(id: impl Into<NodeId>, spec: DragSpec<Dag>, pointer: Vec2) -> Frame<Dag> {
        spec.compile(&DragContext::new(view, id.into(), initial()))(Pointer::from(pointer))
    }

    fn drag_head(edge: usize, pointer: Vec2) -> Frame<Dag> {
        drag(
            format!("arrow-{edge}"),
            head_spec(&initial(), edge),
            pointer,
        )
    }

    #[test]
    fn dragging_a_node_changes_only_that_node() {
        let dag = initial();
        let frame = drag(
            "node-3",
            node_spec(&dag, 3),
            dag.nodes[3] + Vec2::new(0.0, -1.5),
        );
        assert!(frame.drop_state.nodes[3].distance(dag.nodes[3] + Vec2::new(0.0, -1.5)) < 0.05);
        for other in [0, 1, 2, 4] {
            assert_eq!(frame.drop_state.nodes[other], dag.nodes[other]);
        }
        assert_eq!(frame.drop_state.edges, dag.edges);
    }

    #[test]
    fn the_edges_follow_a_dragged_node() {
        let dag = initial();
        let moved = dag.nodes[3] + Vec2::new(0.0, -1.5);
        let frame = drag("node-3", node_spec(&dag, 3), moved);
        let (tip, _) = frame.drop_state.head(2);
        assert!(tip.distance(moved) < NODE_RADIUS + 0.05);
        let arrow = frame.view.world_position("arrow-2").unwrap().truncate();
        assert!(arrow.distance(moved) < NODE_RADIUS + ARROW_LENGTH + 0.05);
    }

    #[test]
    fn the_focus_is_marked_and_nothing_else_stops_being_draggable() {
        let dag = initial();
        let frame = drag("node-3", node_spec(&dag, 3), dag.nodes[3]);
        let ring = frame.view.world_position("overlay-focus").unwrap();
        assert!(ring.truncate().distance(dag.nodes[3]) < 0.05);
        for id in view(&dag).nodes().iter().map(|node| node.id.clone()) {
            let draggable = id.starts_with("node-") || id.starts_with("arrow-");
            assert_eq!(
                frame.view.get(&id).unwrap().drag.is_some(),
                draggable,
                "{id}"
            );
        }
    }

    #[test]
    fn an_arrowhead_snaps_to_a_node_and_changes_only_its_edge() {
        let dag = initial();
        let mut retargeted = dag.clone();
        retargeted.edges[2].to = End::Node(4);
        let frame = drag_head(2, retargeted.grip(2));
        assert_eq!(frame.drop_state.edges[2].to, End::Node(4));
        assert_eq!(frame.drop_state.nodes, dag.nodes);
        for other in [0, 1, 3, 4] {
            assert_eq!(frame.drop_state.edges[other], dag.edges[other]);
        }
        assert!(frame.view.get("overlay-focus").is_some());
        assert!(frame.view.get("node-0").unwrap().drag.is_some());
    }

    #[test]
    fn an_arrowhead_far_from_every_node_floats_under_the_pointer() {
        let pointer = Vec2::new(0.5, -2.5);
        let frame = drag_head(2, pointer);
        let arrow = frame.view.world_position("arrow-2").unwrap().truncate();
        assert!(arrow.distance(pointer) < 0.1);
        assert!(frame.view.get("edge-2").is_some());
        assert_eq!(frame.drop_state.nodes, initial().nodes);
    }

    #[test]
    fn dropping_a_loose_arrowhead_deletes_its_edge() {
        let frame = drag_head(2, Vec2::new(0.5, -2.5));
        assert_eq!(frame.drop_state.edges.len(), 4);
        assert!(
            frame
                .drop_state
                .edges
                .iter()
                .all(|edge| matches!(edge.to, End::Node(_)))
        );
    }

    #[test]
    fn a_head_never_snaps_to_a_node_that_closes_a_cycle() {
        let dag = initial();
        // Pointing the edge from node 3 to node 4 back at node 0 would close 0 -> 1 -> 3 -> 0.
        assert!(dag.closes_a_cycle(4, 0));
        assert!(!dag.closes_a_cycle(4, 4));
        let mut tempting = dag.clone();
        tempting.edges[4].to = End::Node(0);
        let frame = drag_head(4, tempting.grip(4));
        // The head floated loose, so dropping it deleted the edge rather than closing the cycle.
        assert_eq!(frame.drop_state.edges.len(), 4);
        assert!(
            !frame
                .drop_state
                .edges
                .iter()
                .any(|edge| edge.to == End::Node(0))
        );
    }

    #[test]
    fn the_scripted_drag_presses_the_arrowhead_and_ends_on_another_node() {
        let dag = initial();
        let press = Ray3d::new(dag.grip(2).extend(10.0), Dir3::NEG_Z);
        assert_eq!(view(&dag).hit_test(press).unwrap().id, "arrow-2");
        let mut retargeted = dag.clone();
        retargeted.edges[2].to = End::Node(4);
        let frame = drag_head(2, retargeted.grip(2));
        assert_eq!(frame.drop_state.edges[2].to, End::Node(4));
    }

    #[test]
    fn an_edge_never_points_back_at_its_own_node() {
        let frame = drag_head(2, initial().nodes[1]);
        assert_ne!(frame.drop_state.edges[2].to, End::Node(1));
    }
}
