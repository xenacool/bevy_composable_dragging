//! Blending of views by node id.

use std::collections::{HashMap, HashSet};

use bevy::color::LinearRgba;
use bevy::math::Vec3;
use bevy::prelude::Transform;
#[cfg(feature = "gradmesh")]
use bevy_gradmesh::Style;

use crate::blend::{Blend, Presence};
use crate::geometry::Geometry;
#[cfg(feature = "gradmesh")]
use crate::shapes::gradient_style;
use crate::view::{View, ViewNode, to_local};

/// A node whose opacity is below this is invisible and dropped from a blended view.
const INVISIBLE: f32 = 0.01;

/// Linearly interpolates between two transforms (translation and scale linearly, rotation
/// spherically along the shortest arc).
#[must_use]
pub fn lerp_transform(a: &Transform, b: &Transform, t: f32) -> Transform {
    a.blend(b, t)
}

/// Blends the gradient styles of two matched nodes with `Style::lerp`.
///
/// A node without a style takes part with the gradient derived from its color, so a styled node
/// still blends with an unstyled one. Without any style the result is `None` and the blended
/// color alone decides the gradient.
#[cfg(feature = "gradmesh")]
fn blend_styles<S>(a: &ViewNode<S>, b: &ViewNode<S>, t: f32) -> Option<Style> {
    match (&a.style, &b.style) {
        (None, None) => None,
        (Some(style_a), Some(style_b)) => Some(style_a.blend(style_b, t)),
        (Some(style_a), None) => Some(style_a.blend(&gradient_style(b.color), t)),
        (None, Some(style_b)) => Some(gradient_style(a.color).blend(style_b, t)),
    }
}

/// The blended look of a node that is about to be added to a blended view.
struct Blended<'a, S> {
    /// The node whose discrete properties (parent, drag, presence) the result takes.
    discrete: &'a ViewNode<S>,
    world: Transform,
    color: LinearRgba,
    opacity: f32,
    #[cfg(feature = "gradmesh")]
    style: Option<Style>,
    geometry: Geometry,
}

impl<'a, S> Blended<'a, S> {
    /// A node taken over as it is, with its world transform.
    fn unchanged(node: &'a ViewNode<S>, world: Transform) -> Self {
        Self {
            discrete: node,
            world,
            color: node.color,
            opacity: node.opacity,
            #[cfg(feature = "gradmesh")]
            style: node.style.clone(),
            geometry: node.geometry.clone(),
        }
    }
}

/// Which view a node that exists in only one of two blended views belongs to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    From,
    To,
}

/// Blends two views, matching nodes by id.
///
/// - Matched nodes blend each property through [`Blend`]: world transform (so nodes that change
///   parent move in a straight line), color (in Oklab), opacity, gradient style and geometry.
///   Their parent, drag spec, presence, rendering mode and render layer come from the nearer
///   view.
/// - A node that only exists in `a` or only in `b` follows its [`Presence`]: it fades (the
///   default), grows from another node, or pops at the midpoint.
#[must_use]
pub fn lerp_views<S>(a: &View<S>, b: &View<S>, t: f32) -> View<S> {
    blend_pair(a, b, t, None)
}

/// Blends two views. The discrete properties of a node come from the node of `discrete` with
/// the same id when there is one, and from the nearer view otherwise.
fn blend_pair<S>(a: &View<S>, b: &View<S>, t: f32, discrete: Option<&View<S>>) -> View<S> {
    let t = t.clamp(0.0, 1.0);
    let nearer_is_a = t < 0.5;
    let worlds_a = a.world_transforms();
    let worlds_b = b.world_transforms();

    let mut entries: Vec<Blended<S>> = Vec::with_capacity(a.len().max(b.len()));
    for node_a in a.nodes() {
        let id = node_a.id.as_ref();
        if let Some(node_b) = b.get(id) {
            let nearer = if nearer_is_a { node_a } else { node_b };
            entries.push(Blended {
                discrete: nearer,
                world: worlds_a[id].blend(&worlds_b[id], t),
                color: node_a.color.blend(&node_b.color, t),
                opacity: node_a.opacity.blend(&node_b.opacity, t),
                #[cfg(feature = "gradmesh")]
                style: blend_styles(node_a, node_b, t),
                geometry: node_a.geometry.blend(&node_b.geometry, t),
            });
        } else if let Some(entry) = one_sided(node_a, Side::From, t, (a, &worlds_a), (b, &worlds_b))
        {
            entries.push(entry);
        }
    }
    for node_b in b.nodes() {
        if a.get(&node_b.id).is_none()
            && let Some(entry) = one_sided(node_b, Side::To, t, (b, &worlds_b), (a, &worlds_a))
        {
            entries.push(entry);
        }
    }
    let entries: Vec<Blended<S>> = entries
        .into_iter()
        .map(|mut entry| {
            if let Some(node) = discrete.and_then(|view| view.get(&entry.discrete.id)) {
                entry.discrete = node;
            }
            entry
        })
        .collect();

    let worlds: HashMap<&str, Transform> = entries
        .iter()
        .map(|entry| (entry.discrete.id.as_ref(), entry.world))
        .collect();
    let present: HashSet<&str> = worlds.keys().copied().collect();

    let mut blended = View::new();
    for entry in &entries {
        let node = entry.discrete;
        let parent = node
            .parent
            .clone()
            .filter(|parent| present.contains(parent.as_ref()));
        let parent_world = parent
            .as_deref()
            .and_then(|parent| worlds.get(parent).copied());
        blended.push(ViewNode {
            id: node.id.clone(),
            parent,
            transform: to_local(entry.world, parent_world),
            color: entry.color,
            #[cfg(feature = "gradmesh")]
            style: entry.style.clone(),
            geometry: entry.geometry.clone(),
            opacity: entry.opacity,
            presence: node.presence.clone(),
            casts_shadow: node.casts_shadow,
            rendering: node.rendering,
            layer: node.layer,
            drag: node.drag.clone(),
            drag_space: node.drag_space,
        });
    }
    blended
}

/// The blended look of a node that exists in `own` but not in `other`, or `None` when it is not
/// shown at `t`.
fn one_sided<'a, S>(
    node: &'a ViewNode<S>,
    side: Side,
    t: f32,
    (own, own_worlds): (&'a View<S>, &HashMap<&str, Transform>),
    (other, other_worlds): (&View<S>, &HashMap<&str, Transform>),
) -> Option<Blended<'a, S>> {
    let world = own_worlds[node.id.as_ref()];
    // How present the node is: 1 in its own view, 0 in the other one.
    let presence = match side {
        Side::From => 1.0 - t,
        Side::To => t,
    };
    match &node.presence {
        Presence::Pop => (presence >= 0.5).then(|| Blended::unchanged(node, world)),
        Presence::Fade => {
            // A group fades as a whole: a child whose parent fades with it inherits the opacity
            // of the parent when it is drawn, so only the top of the group scales its own.
            let mut top = node;
            for _ in 0..own.len() {
                let Some(parent) = top
                    .parent
                    .as_deref()
                    .and_then(|parent| own.get(parent))
                    .filter(|parent| {
                        other.get(&parent.id).is_none() && parent.presence == Presence::Fade
                    })
                else {
                    break;
                };
                top = parent;
            }
            if presence * top.opacity < INVISIBLE {
                return None;
            }
            let mut entry = Blended::unchanged(node, world);
            if std::ptr::eq(top, node) {
                entry.opacity *= presence;
            }
            Some(entry)
        }
        Presence::Emerge { from } => {
            let origin = other_worlds
                .get(from.as_ref())
                .or_else(|| own_worlds.get(from.as_ref()))
                .unwrap_or(&world);
            let hidden = Transform {
                translation: origin.translation,
                rotation: world.rotation,
                scale: Vec3::ZERO,
            };
            if presence * node.opacity < INVISIBLE {
                return None;
            }
            let mut entry = Blended::unchanged(node, hidden.blend(&world, presence));
            entry.opacity *= presence;
            Some(entry)
        }
    }
}

/// Blends any number of views with the given non-negative `weights`.
///
/// Views with a zero weight are skipped; the result is an empty view when no weight is positive.
/// `views` and `weights` are paired up, extra entries of the longer slice are ignored. The
/// discrete properties (parent, drag spec and presence) come from the heaviest view, with ties
/// going to the earlier one.
#[must_use]
pub fn blend_views<S: Clone>(views: &[&View<S>], weights: &[f32]) -> View<S> {
    let active: Vec<(&View<S>, f32)> = views
        .iter()
        .zip(weights)
        .filter(|(_, weight)| **weight > 0.0)
        .map(|(view, weight)| (*view, *weight))
        .collect();
    let Some(heaviest) = active
        .iter()
        .copied()
        .reduce(|best, next| if next.1 > best.1 { next } else { best })
        .map(|(view, _)| view)
    else {
        return View::new();
    };
    let mut active = active.into_iter();
    let Some((first, mut total)) = active.next() else {
        return View::new();
    };
    let mut blended = first.clone();
    for (view, weight) in active {
        total += weight;
        blended = blend_pair(&blended, view, weight / total, Some(heaviest));
    }
    blended
}

#[cfg(test)]
mod tests {
    use bevy::math::Vec2;

    use super::*;
    use crate::rendering::Rendering;
    use bevy::color::Mix;

    type TestView = View<()>;

    fn dot(id: &'static str, x: f32) -> ViewNode<()> {
        ViewNode::circle(id, 1.0).at_xy(Vec2::new(x, 0.0))
    }

    #[test]
    fn matched_nodes_interpolate_position_and_color() {
        let a: TestView =
            View::new().with(dot("n", 0.0).with_color(bevy::color::palettes::basic::RED));
        let b: TestView =
            View::new().with(dot("n", 10.0).with_color(bevy::color::palettes::basic::BLUE));
        let mid = lerp_views(&a, &b, 0.5);
        assert!((mid.world_position("n").unwrap().x - 5.0).abs() < 1e-5);
        let color = mid.get("n").unwrap().color;
        assert!(color.red > 0.0 && color.blue > 0.0);
    }

    #[test]
    fn endpoints_reproduce_the_inputs() {
        let a: TestView = View::new().with(dot("n", 1.0));
        let b: TestView = View::new().with(dot("n", 9.0));
        assert!((lerp_views(&a, &b, 0.0).world_position("n").unwrap().x - 1.0).abs() < 1e-5);
        assert!((lerp_views(&a, &b, 1.0).world_position("n").unwrap().x - 9.0).abs() < 1e-5);
    }

    #[test]
    fn popping_nodes_switch_at_the_midpoint() {
        let a: TestView = View::new().with(dot("only_a", 0.0).presence(Presence::Pop));
        let b: TestView = View::new().with(dot("only_b", 0.0).presence(Presence::Pop));
        let early = lerp_views(&a, &b, 0.25);
        assert!(early.get("only_a").is_some() && early.get("only_b").is_none());
        let late = lerp_views(&a, &b, 0.75);
        assert!(late.get("only_a").is_none() && late.get("only_b").is_some());
    }

    #[test]
    fn unmatched_nodes_fade_by_default() {
        let a: TestView = View::new().with(dot("leaving", 0.0));
        let b: TestView = View::new().with(dot("entering", 0.0));
        let quarter = lerp_views(&a, &b, 0.25);
        assert!((quarter.get("leaving").unwrap().opacity - 0.75).abs() < 1e-5);
        assert!((quarter.get("entering").unwrap().opacity - 0.25).abs() < 1e-5);
        let end = lerp_views(&a, &b, 1.0);
        assert!(end.get("leaving").is_none(), "an invisible node is dropped");
        assert!((end.get("entering").unwrap().opacity - 1.0).abs() < 1e-5);
        let start = lerp_views(&a, &b, 0.0);
        assert!(start.get("entering").is_none());
    }

    #[test]
    fn fading_scales_the_own_opacity() {
        let a: TestView = View::new().with(dot("n", 0.0).with_opacity(0.5));
        let faded = lerp_views(&a, &View::new(), 0.5);
        assert!((faded.get("n").unwrap().opacity - 0.25).abs() < 1e-5);
    }

    #[test]
    fn a_group_fades_through_its_top_node() {
        let b: TestView = View::new()
            .with(dot("group", 0.0))
            .with(dot("child", 1.0).with_parent("group"));
        let mid = lerp_views(&View::new(), &b, 0.5);
        assert!((mid.get("group").unwrap().opacity - 0.5).abs() < 1e-5);
        assert!(
            (mid.get("child").unwrap().opacity - 1.0).abs() < 1e-5,
            "the child is drawn with the opacity of its parent"
        );
        assert_eq!(mid.get("child").unwrap().parent.as_deref(), Some("group"));
        assert!(lerp_views(&View::new(), &b, 0.005).is_empty());
    }

    #[test]
    fn rendering_mode_and_layer_come_from_the_nearer_view() {
        let a: TestView = View::new().with(dot("n", 0.0));
        let b: TestView = View::new().with(dot("n", 1.0).spatial().on_layer(2));
        let early = lerp_views(&a, &b, 0.25);
        assert_eq!(early.get("n").unwrap().layer, 0);
        assert_eq!(early.get("n").unwrap().rendering, None);
        let late = lerp_views(&a, &b, 0.75);
        assert_eq!(late.get("n").unwrap().layer, 2);
        assert_eq!(late.get("n").unwrap().rendering, Some(Rendering::Spatial));
        let refs = [&a, &b];
        let heavy = blend_views(&refs, &[0.25, 0.75]);
        assert_eq!(heavy.get("n").unwrap().layer, 2);
        let tie = blend_views(&refs, &[0.5, 0.5]);
        assert_eq!(tie.get("n").unwrap().layer, 0);
    }

    #[test]
    fn one_sided_nodes_keep_their_rendering_mode_and_layer() {
        let b: TestView = View::new().with(dot("hud", 0.0).spatial().on_layer(2));
        let mid = lerp_views(&View::new(), &b, 0.5);
        assert_eq!(mid.get("hud").unwrap().layer, 2);
        assert_eq!(mid.get("hud").unwrap().rendering, Some(Rendering::Spatial));
    }

    #[test]
    fn matched_nodes_blend_opacity() {
        let a: TestView = View::new().with(dot("n", 0.0).with_opacity(0.0));
        let b: TestView = View::new().with(dot("n", 0.0));
        assert!((lerp_views(&a, &b, 0.25).get("n").unwrap().opacity - 0.25).abs() < 1e-5);
    }

    #[test]
    fn emerging_nodes_grow_from_their_origin() {
        let origin = dot("origin", 10.0);
        let a: TestView = View::new().with(origin.clone());
        let b: TestView = View::new().with(origin).with(
            ViewNode::circle("child", 2.0)
                .at_xy(Vec2::new(20.0, 0.0))
                .emerge_from("origin"),
        );
        let start = lerp_views(&a, &b, 0.02);
        let child = start.get("child").unwrap();
        assert!(child.transform.scale.x < 0.2);
        assert!((start.world_position("child").unwrap().x - 10.2).abs() < 0.1);
        let mid = lerp_views(&a, &b, 0.5);
        assert!((mid.world_position("child").unwrap().x - 15.0).abs() < 1e-4);
        assert!((mid.get("child").unwrap().transform.scale.x - 1.0).abs() < 1e-4);
        let end = lerp_views(&a, &b, 1.0);
        assert!((end.get("child").unwrap().transform.scale.x - 2.0).abs() < 1e-4);
        assert!(lerp_views(&a, &b, 0.0).get("child").is_none());
    }

    #[test]
    fn leaving_nodes_shrink_into_their_origin() {
        let origin = dot("origin", 10.0);
        let a: TestView = View::new().with(origin.clone()).with(
            ViewNode::circle("child", 2.0)
                .at_xy(Vec2::new(20.0, 0.0))
                .emerge_from("origin"),
        );
        let b: TestView = View::new().with(origin);
        let mid = lerp_views(&a, &b, 0.5);
        assert!((mid.world_position("child").unwrap().x - 15.0).abs() < 1e-4);
        assert!((mid.get("child").unwrap().transform.scale.x - 1.0).abs() < 1e-4);
    }

    #[test]
    fn rotations_blend_along_the_shortest_arc_in_both_directions() {
        let turned = |degrees: f32| -> TestView {
            View::new().with(dot("n", 0.0).with_rotation_xy(degrees.to_radians()))
        };
        for (from, to) in [(350.0, 10.0), (10.0, 350.0)] {
            let mid = lerp_views(&turned(from), &turned(to), 0.5);
            let rotation = mid.get("n").unwrap().transform.rotation;
            assert!(rotation.angle_between(bevy::math::Quat::IDENTITY) < 1e-3);
        }
    }

    #[test]
    fn matched_colors_blend_in_oklab_and_keep_alpha() {
        let a: TestView =
            View::new().with(dot("n", 0.0).with_color(bevy::color::palettes::basic::RED));
        let b: TestView =
            View::new().with(dot("n", 0.0).with_color(bevy::color::palettes::basic::BLUE));
        let mid = lerp_views(&a, &b, 0.5).get("n").unwrap().color;
        assert!((mid.alpha - 1.0).abs() < 1e-5);
        let linear = bevy::color::LinearRgba::from(bevy::color::palettes::basic::RED).mix(
            &bevy::color::LinearRgba::from(bevy::color::palettes::basic::BLUE),
            0.5,
        );
        assert!(mid != linear, "the blend is not a plain linear mix");
    }

    #[test]
    fn reparented_nodes_move_in_world_space() {
        let a: TestView = View::new()
            .with(dot("left", -10.0))
            .with(dot("right", 10.0))
            .with(dot("item", 0.0).with_parent("left"));
        let b: TestView = View::new()
            .with(dot("left", -10.0))
            .with(dot("right", 10.0))
            .with(dot("item", 0.0).with_parent("right"));
        // World position of the item: -10 in `a`, 10 in `b`.
        let mid = lerp_views(&a, &b, 0.5);
        assert!(mid.world_position("item").unwrap().x.abs() < 1e-4);
        assert_eq!(mid.get("item").unwrap().parent.as_deref(), Some("right"));
        let quarter = lerp_views(&a, &b, 0.25);
        assert!((quarter.world_position("item").unwrap().x + 5.0).abs() < 1e-4);
    }

    #[cfg(feature = "gradmesh")]
    mod styles {
        use bevy::color::Color;
        use bevy_gradmesh::Style;

        use super::*;

        fn style(negative: Color, positive: Color) -> Style {
            Style::dual_directional_up(negative, positive)
        }

        fn dark_to_bright_red() -> Style {
            style(Color::srgb(0.2, 0.0, 0.0), Color::srgb(1.0, 0.0, 0.0))
        }

        fn green_to_white() -> Style {
            style(Color::srgb(0.0, 0.2, 0.0), Color::srgb(1.0, 1.0, 1.0))
        }

        fn styled(style: Style) -> TestView {
            View::new().with(dot("n", 0.0).with_style(style))
        }

        #[test]
        fn matched_styles_blend_with_style_lerp() {
            let (a, b) = (dark_to_bright_red(), green_to_white());
            let mid = lerp_views(&styled(a.clone()), &styled(b.clone()), 0.3);
            let blended = mid.get("n").unwrap().style.as_ref().unwrap();
            assert_eq!(*blended, a.lerp(&b, 0.3));
        }

        #[test]
        fn unstyled_nodes_stay_unstyled() {
            let a: TestView = View::new().with(dot("n", 0.0));
            let b: TestView = View::new().with(dot("n", 4.0));
            assert!(lerp_views(&a, &b, 0.5).get("n").unwrap().style.is_none());
        }

        #[test]
        fn a_styled_node_blends_with_an_unstyled_one() {
            let a = styled(dark_to_bright_red());
            let b: TestView = View::new().with(dot("n", 4.0));
            assert!(lerp_views(&a, &b, 0.5).get("n").unwrap().style.is_some());
            assert!(lerp_views(&b, &a, 0.5).get("n").unwrap().style.is_some());
        }

        #[test]
        fn unmatched_nodes_keep_their_style() {
            let a = styled(dark_to_bright_red());
            let b: TestView = View::new();
            assert!(lerp_views(&a, &b, 0.25).get("n").unwrap().style.is_some());
        }
    }

    #[cfg(feature = "gradmesh")]
    mod geometry {
        use bevy_gradmesh::{Shape, ShapeDetail};

        use super::*;
        use crate::Geometry;

        fn sphere(radius: f32) -> ViewNode<()> {
            ViewNode::sphere("n", radius)
        }

        #[test]
        fn matched_shapes_morph_with_the_blend() {
            let a: TestView = View::new().with(sphere(1.0));
            let b: TestView = View::new().with(sphere(3.0));
            let mid = lerp_views(&a, &b, 0.5);
            assert_eq!(
                mid.get("n").unwrap().geometry,
                Geometry::Grad(Shape::sphere(2.0, ShapeDetail::Medium))
            );
        }

        #[test]
        fn shapes_of_different_kinds_come_from_the_nearer_view() {
            let a: TestView = View::new().with(sphere(1.0));
            let b: TestView = View::new().with(ViewNode::cuboid("n", bevy::math::Vec3::ONE, 0.0));
            assert_eq!(
                lerp_views(&a, &b, 0.25).get("n").unwrap().geometry,
                a.get("n").unwrap().geometry
            );
            assert_eq!(
                lerp_views(&a, &b, 0.75).get("n").unwrap().geometry,
                b.get("n").unwrap().geometry
            );
        }
    }

    #[test]
    fn the_heaviest_view_supplies_the_discrete_properties() {
        let views: Vec<TestView> = [("a", 0.0), ("b", 10.0), ("c", 20.0)]
            .iter()
            .map(|&(parent, x)| {
                View::new()
                    .with(dot("a", 0.0))
                    .with(dot("b", 0.0))
                    .with(dot("c", 0.0))
                    .with(dot("n", x).with_parent(parent))
            })
            .collect();
        let refs: Vec<&TestView> = views.iter().collect();
        let heavy_last = blend_views(&refs, &[0.3, 0.3, 0.4]);
        assert_eq!(heavy_last.get("n").unwrap().parent.as_deref(), Some("c"));
        let heavy_middle = blend_views(&refs, &[0.2, 0.5, 0.3]);
        assert_eq!(heavy_middle.get("n").unwrap().parent.as_deref(), Some("b"));
        let tie = blend_views(&refs, &[0.4, 0.4, 0.2]);
        assert_eq!(tie.get("n").unwrap().parent.as_deref(), Some("a"));
    }

    #[test]
    fn blend_uses_the_weights() {
        let views: Vec<TestView> = [0.0, 10.0, 20.0]
            .iter()
            .map(|&x| View::new().with(dot("n", x)))
            .collect();
        let refs: Vec<&TestView> = views.iter().collect();
        let blended = blend_views(&refs, &[0.5, 0.25, 0.25]);
        // Mean position: 0.5 * 0 + 0.25 * 10 + 0.25 * 20 = 7.5.
        assert!((blended.world_position("n").unwrap().x - 7.5).abs() < 1e-4);
        assert!(blend_views(&refs, &[0.0, 0.0, 0.0]).is_empty());
    }
}
