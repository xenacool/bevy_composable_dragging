//! The `with_floating` modifier.

use std::sync::Arc;

use crate::spec::DragSpec;

/// How far a floating node is lifted along the normal of the drag space so it is drawn above the
/// others.
const FLOAT_LIFT: f32 = 0.5;

/// Maps the distance between the pointer and the node's drop position to the distance the
/// floating node keeps from that position.
type Tether = Arc<dyn Fn(f32) -> f32 + Send + Sync>;

/// Options of [`with_floating_options`](DragSpec::with_floating_options). The defaults match
/// [`with_floating`](DragSpec::with_floating).
#[derive(Clone, Default)]
#[non_exhaustive]
pub struct FloatingOptions {
    /// When set, a copy of the dragged node stays where dropping would put it, drawn with its
    /// alpha multiplied by this opacity. The copy has the id `ghost-<id>` and is not draggable.
    /// Nodes drawn with a gradient style are not faded, only nodes drawn from their color.
    pub ghost: Option<f32>,
    tether: Option<Tether>,
}

impl FloatingOptions {
    /// The defaults: no ghost and no tether.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Leaves a ghost of the dragged node at its drop position, with the given opacity.
    #[must_use]
    pub fn with_ghost(mut self, opacity: f32) -> Self {
        self.ghost = Some(opacity);
        self
    }

    /// Limits how far the floating node strays from where dropping would put it: `tether` maps
    /// the distance between the pointer and that position to the distance the node keeps, for
    /// example `|distance| distance.min(40.0)` for a leash or `|distance| distance * 0.5` for a
    /// rubber band.
    #[must_use]
    pub fn with_tether(mut self, tether: impl Fn(f32) -> f32 + Send + Sync + 'static) -> Self {
        self.tether = Some(Arc::new(tether));
        self
    }
}

impl<S: Clone + Send + Sync + 'static> DragSpec<S> {
    /// Makes the dragged node float freely under the pointer while the rest of the preview
    /// shows what dropping would do. The dragged node is lifted by half a unit so it is drawn
    /// above its neighbors; the drop state is unchanged.
    #[must_use]
    pub fn with_floating(self) -> Self {
        self.with_floating_options(FloatingOptions::new())
    }

    /// Like [`with_floating`](Self::with_floating), with the ghost and tether of
    /// [`FloatingOptions`].
    #[must_use]
    pub fn with_floating_options(self, options: FloatingOptions) -> Self {
        Self::new(move |context| {
            let mut inner = self.compile(context);
            let dragged = context.dragged().to_owned();
            let space = *context.space();
            let options = options.clone();
            Box::new(move |pointer| {
                let mut frame = inner(pointer);
                let home = frame.view.world_position(&dragged);
                if let Some(opacity) = options.ghost
                    && let Some(node) = frame.view.get(&dragged)
                {
                    let mut ghost = node.clone();
                    ghost.id = format!("ghost-{}", node.id).into();
                    ghost.drag = None;
                    ghost.color.alpha *= opacity;
                    frame.view.push(ghost);
                }
                // The node floats in the space of the drag, keeping its own distance from it, and
                // is lifted along its normal.
                let target = match (home, &options.tether) {
                    (Some(home), Some(tether)) => {
                        let home = space.project_point(home);
                        let offset = pointer.point - home;
                        let distance = offset.length();
                        if distance > 1e-6 {
                            home + offset * (tether(distance) / distance)
                        } else {
                            pointer.point
                        }
                    }
                    _ => pointer.point,
                };
                if let (Some(home), Some(mut world)) = (home, frame.view.world_transform(&dragged))
                {
                    let depth = home - space.project_point(home);
                    world.translation = target + depth + *space.normal() * FLOAT_LIFT;
                    frame.view.set_world_transform(&dragged, world);
                }
                frame
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::space::Pointer;
    use bevy::math::Vec2;

    use super::FloatingOptions;
    use crate::ops::closest_states;
    use crate::ops::testing::{Slot, context};

    fn nearest_slots() -> crate::DragSpec<Slot> {
        closest_states([Slot(0), Slot(1), Slot(2)])
    }

    #[test]
    fn the_float_follows_the_pointer_and_is_lifted() {
        let frame =
            nearest_slots().with_floating().compile(&context())(Pointer::from(Vec2::new(7.0, 3.0)));
        let item = frame.view.world_transform("item").unwrap().translation;
        assert!(item.truncate().distance(Vec2::new(7.0, 3.0)) < 1e-4);
        assert!(item.z > 0.4);
        assert_eq!(frame.drop_state, Slot(1));
    }

    #[test]
    fn the_float_is_lifted_along_the_normal_of_the_drag_space() {
        use bevy::math::{Dir3, Vec3};

        use crate::space::DragSpace;
        let ground = context().with_space(DragSpace::plane(Vec3::ZERO, Dir3::Y));
        let pointer = Pointer::at(Vec3::new(7.0, 0.0, 3.0));
        let frame = nearest_slots().with_floating().compile(&ground)(pointer);
        let item = frame.view.world_position("item").unwrap();
        assert!(item.distance(Vec3::new(7.0, 0.5, 3.0)) < 1e-4, "{item}");
    }

    #[test]
    fn a_ghost_stays_where_dropping_would_put_the_node() {
        let spec = nearest_slots().with_floating_options(FloatingOptions::new().with_ghost(0.5));
        let frame = spec.compile(&context())(Pointer::from(Vec2::new(7.0, 3.0)));
        let ghost = frame.view.get("ghost-item").unwrap();
        assert!(ghost.drag.is_none());
        assert!((ghost.color.alpha - 0.5).abs() < 1e-6);
        assert!((frame.view.world_position("ghost-item").unwrap().x - 10.0).abs() < 1e-4);
        assert!((frame.view.world_position("item").unwrap().x - 7.0).abs() < 1e-4);
    }

    #[test]
    fn a_tether_limits_how_far_the_float_strays() {
        let spec = nearest_slots()
            .with_floating_options(FloatingOptions::new().with_tether(|d| d.min(2.0)));
        let mut behavior = spec.compile(&context());
        // Slot 1 is at x = 10; the pointer is 5 to its right, the leash is 2.
        let far = behavior(Pointer::from(Vec2::new(15.0, 0.0)));
        assert!((far.view.world_position("item").unwrap().x - 12.0).abs() < 1e-4);
        let near = behavior(Pointer::from(Vec2::new(11.0, 0.0)));
        assert!((near.view.world_position("item").unwrap().x - 11.0).abs() < 1e-4);
    }

    #[test]
    fn a_rubber_band_scales_the_distance() {
        let spec =
            nearest_slots().with_floating_options(FloatingOptions::new().with_tether(|d| d * 0.5));
        let frame = spec.compile(&context())(Pointer::from(Vec2::new(14.0, 0.0)));
        assert!((frame.view.world_position("item").unwrap().x - 12.0).abs() < 1e-4);
    }
}
