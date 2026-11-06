//! Blending of views by node id.

use std::collections::{HashMap, HashSet};

use bevy::color::Mix;
use bevy::prelude::Transform;
#[cfg(any(feature = "shapes-2d", feature = "shapes-3d"))]
use bevy_gradmesh::Style;

#[cfg(any(feature = "shapes-2d", feature = "shapes-3d"))]
use crate::shapes::gradient_style;
use crate::view::{View, ViewNode, to_local};

/// Linearly interpolates between two transforms (translation and scale linearly, rotation
/// spherically).
#[must_use]
pub fn lerp_transform(a: &Transform, b: &Transform, t: f32) -> Transform {
    Transform {
        translation: a.translation.lerp(b.translation, t),
        rotation: a.rotation.slerp(b.rotation, t),
        scale: a.scale.lerp(b.scale, t),
    }
}

/// Blends the gradient styles of two matched nodes with `Style::lerp`.
///
/// A node without a style takes part with the gradient derived from its color, so a styled node
/// still blends with an unstyled one. Without any style the result is `None` and the blended
/// color alone decides the gradient.
#[cfg(any(feature = "shapes-2d", feature = "shapes-3d"))]
fn lerp_styles<S>(a: &ViewNode<S>, b: &ViewNode<S>, t: f32) -> Option<Style> {
    match (&a.style, &b.style) {
        (None, None) => None,
        (Some(style_a), Some(style_b)) => Some(style_a.lerp(style_b, t)),
        (Some(style_a), None) => Some(style_a.lerp(&gradient_style(b.color), t)),
        (None, Some(style_b)) => Some(gradient_style(a.color).lerp(style_b, t)),
    }
}

/// Blends two views, matching nodes by id.
///
/// - Matched nodes interpolate their world transform and color; discrete properties (shape and
///   parent) come from the nearer view. Transforms are blended in world space so nodes that
///   change parent move in a straight line.
/// - With the `shapes-2d` or `shapes-3d` feature, matched nodes also blend their gradient styles
///   with `Style::lerp` from `bevy_gradmesh`.
/// - A node that only exists in `a` stays visible while `t < 0.5`; a node that only exists in `b`
///   appears once `t >= 0.5`.
/// - Drag specs are not carried over: a blended view is only ever displayed, never dragged.
#[must_use]
pub fn lerp_views<S>(a: &View<S>, b: &View<S>, t: f32) -> View<S> {
    let t = t.clamp(0.0, 1.0);
    let nearer_is_a = t < 0.5;
    let worlds_a = a.world_transforms();
    let worlds_b = b.world_transforms();

    let sources: Vec<(&ViewNode<S>, Option<&ViewNode<S>>)> = {
        let mut sources = Vec::with_capacity(a.len().max(b.len()));
        for node_a in a.nodes() {
            match b.get(&node_a.id) {
                Some(node_b) => sources.push((node_a, Some(node_b))),
                None if nearer_is_a => sources.push((node_a, None)),
                None => {}
            }
        }
        if !nearer_is_a {
            sources.extend(
                b.nodes()
                    .iter()
                    .filter(|node_b| a.get(&node_b.id).is_none())
                    .map(|node_b| (node_b, None)),
            );
        }
        sources
    };

    let mut worlds: HashMap<&str, Transform> = HashMap::with_capacity(sources.len());
    for (node, _) in &sources {
        let id = node.id.as_ref();
        let world = match (worlds_a.get(id), worlds_b.get(id)) {
            (Some(wa), Some(wb)) => lerp_transform(wa, wb, t),
            (Some(world), None) | (None, Some(world)) => *world,
            (None, None) => node.transform,
        };
        worlds.insert(id, world);
    }
    let present: HashSet<&str> = worlds.keys().copied().collect();

    let mut blended = View::new();
    for (node, other) in sources {
        let id = node.id.as_ref();
        let nearest = match other {
            Some(node_b) if !nearer_is_a => node_b,
            _ => node,
        };
        let parent = nearest
            .parent
            .clone()
            .filter(|parent| present.contains(parent.as_ref()));
        let parent_world = parent
            .as_deref()
            .and_then(|parent| worlds.get(parent).copied());
        let color = match other {
            Some(node_b) => node.color.mix(&node_b.color, t),
            None => node.color,
        };
        #[cfg(any(feature = "shapes-2d", feature = "shapes-3d"))]
        let style = match other {
            Some(node_b) => lerp_styles(node, node_b, t),
            None => node.style.clone(),
        };
        blended.push(ViewNode {
            id: nearest.id.clone(),
            parent,
            transform: to_local(worlds[id], parent_world),
            color,
            #[cfg(any(feature = "shapes-2d", feature = "shapes-3d"))]
            style,
            shape: nearest.shape,
            drag: None,
        });
    }
    blended
}

/// Blends any number of views with the given non-negative `weights`.
///
/// Views with a zero weight are skipped; the result is an empty view when no weight is positive.
/// `views` and `weights` are paired up, extra entries of the longer slice are ignored.
#[must_use]
pub fn blend_views<S: Clone>(views: &[&View<S>], weights: &[f32]) -> View<S> {
    let mut blended: Option<View<S>> = None;
    let mut total = 0.0;
    for (view, &weight) in views.iter().zip(weights) {
        if weight <= 0.0 {
            continue;
        }
        total += weight;
        blended = Some(match blended {
            None => (*view).clone(),
            Some(current) => lerp_views(&current, view, weight / total),
        });
    }
    blended.unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use bevy::math::Vec2;

    use super::*;

    type TestView = View<()>;

    fn dot(id: &'static str, x: f32) -> ViewNode<()> {
        ViewNode::circle(id, 1.0).at(Vec2::new(x, 0.0))
    }

    #[test]
    fn matched_nodes_interpolate_position_and_color() {
        let a: TestView =
            View::new().with(dot("n", 0.0).with_color(bevy::color::palettes::basic::RED));
        let b: TestView =
            View::new().with(dot("n", 10.0).with_color(bevy::color::palettes::basic::BLUE));
        let mid = lerp_views(&a, &b, 0.5);
        assert!((mid.position("n").unwrap().x - 5.0).abs() < 1e-5);
        let color = mid.get("n").unwrap().color;
        assert!(color.red > 0.0 && color.blue > 0.0);
    }

    #[test]
    fn endpoints_reproduce_the_inputs() {
        let a: TestView = View::new().with(dot("n", 1.0));
        let b: TestView = View::new().with(dot("n", 9.0));
        assert!((lerp_views(&a, &b, 0.0).position("n").unwrap().x - 1.0).abs() < 1e-5);
        assert!((lerp_views(&a, &b, 1.0).position("n").unwrap().x - 9.0).abs() < 1e-5);
    }

    #[test]
    fn unmatched_nodes_switch_at_the_midpoint() {
        let a: TestView = View::new().with(dot("only_a", 0.0));
        let b: TestView = View::new().with(dot("only_b", 0.0));
        let early = lerp_views(&a, &b, 0.25);
        assert!(early.get("only_a").is_some() && early.get("only_b").is_none());
        let late = lerp_views(&a, &b, 0.75);
        assert!(late.get("only_a").is_none() && late.get("only_b").is_some());
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
        assert!(mid.position("item").unwrap().x.abs() < 1e-4);
        assert_eq!(mid.get("item").unwrap().parent.as_deref(), Some("right"));
        let quarter = lerp_views(&a, &b, 0.25);
        assert!((quarter.position("item").unwrap().x + 5.0).abs() < 1e-4);
    }

    #[cfg(any(feature = "shapes-2d", feature = "shapes-3d"))]
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
            assert_eq!(format!("{blended:?}"), format!("{:?}", a.lerp(&b, 0.3)));
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

    #[test]
    fn blend_uses_the_weights() {
        let views: Vec<TestView> = [0.0, 10.0, 20.0]
            .iter()
            .map(|&x| View::new().with(dot("n", x)))
            .collect();
        let refs: Vec<&TestView> = views.iter().collect();
        let blended = blend_views(&refs, &[0.5, 0.25, 0.25]);
        // Mean position: 0.5 * 0 + 0.25 * 10 + 0.25 * 20 = 7.5.
        assert!((blended.position("n").unwrap().x - 7.5).abs() < 1e-4);
        assert!(blend_views(&refs, &[0.0, 0.0, 0.0]).is_empty());
    }
}
