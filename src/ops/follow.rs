//! The `switch_to_state_and_follow` operator.

use crate::behavior::{DragContext, Frame};
use crate::spec::DragSpec;
use crate::view::NodeId;

/// Switches to `state` right away and continues the drag with the node `dragged` of that state,
/// using the drag spec attached to that node. This is how dragging out of a toolbar creates a
/// new element: the state gains the element and the drag carries on with it.
///
/// When the node is missing from `state` or has no drag spec, the preview shows `state`.
pub fn switch_to_state_and_follow<S: Clone + Send + Sync + 'static>(
    state: S,
    dragged: impl Into<NodeId>,
) -> DragSpec<S> {
    follow(state, dragged.into(), None)
}

/// Like [`switch_to_state_and_follow`], but continues the drag with `spec` instead of the spec
/// attached to the node.
pub fn switch_to_state_and_follow_with<S: Clone + Send + Sync + 'static>(
    state: S,
    dragged: impl Into<NodeId>,
    spec: DragSpec<S>,
) -> DragSpec<S> {
    follow(state, dragged.into(), Some(spec))
}

fn follow<S: Clone + Send + Sync + 'static>(
    state: S,
    dragged: NodeId,
    spec: Option<DragSpec<S>>,
) -> DragSpec<S> {
    DragSpec::new(move |context| {
        let view = context.render(&state);
        let spec = spec
            .clone()
            .or_else(|| view.get(&dragged).and_then(|node| node.drag_spec(&state)));
        let Some(spec) = spec else {
            let state = state.clone();
            return Box::new(move |_| Frame::new(view.clone(), state.clone(), f32::INFINITY));
        };
        let context = DragContext::with_view_fn(context.view_fn(), dragged.clone(), state.clone())
            .with_grab_offset(context.grab_offset());
        spec.compile(&context)
    })
}

#[cfg(test)]
mod tests {
    use crate::space::Pointer;

    use bevy::math::Vec2;

    use super::*;
    use crate::ops::{state, vary};
    use crate::param;
    use crate::view::{View, ViewNode};
    #[derive(Clone)]
    struct Scene {
        present: bool,
        tile: f32,
    }

    /// A tool at x = -50 that creates a tile, which can then be moved along the X axis.
    fn scene(state: &Scene) -> View<Scene> {
        let mut view = View::new().with(
            ViewNode::circle("tool", 1.0)
                .at_xy(Vec2::new(-50.0, 0.0))
                .draggable(|| {
                    switch_to_state_and_follow(
                        Scene {
                            present: true,
                            tile: -50.0,
                        },
                        "tile",
                    )
                }),
        );
        if state.present {
            let current = state.clone();
            view.push(
                ViewNode::circle("tile", 1.0)
                    .at_xy(Vec2::new(state.tile, 0.0))
                    .draggable(move || vary(current.clone(), [param!(Scene, tile)])),
            );
        }
        view
    }

    fn tool_context() -> DragContext<Scene> {
        DragContext::new(
            scene,
            "tool".into(),
            Scene {
                present: false,
                tile: 0.0,
            },
        )
    }

    fn tool_spec() -> DragSpec<Scene> {
        scene(&Scene {
            present: false,
            tile: 0.0,
        })
        .get("tool")
        .and_then(|node| {
            node.drag_spec(&Scene {
                present: false,
                tile: 0.0,
            })
        })
        .unwrap()
    }

    #[test]
    fn creates_the_element_and_follows_it() {
        let mut behavior = tool_spec().compile(&tool_context());
        let frame = behavior(Pointer::from(Vec2::new(30.0, 0.0)));
        assert!(frame.drop_state.present);
        assert!((frame.drop_state.tile - 30.0).abs() < 1e-2);
        assert!(frame.view.get("tile").is_some());
        assert!(frame.gap < 1e-2);
    }

    #[test]
    fn follows_with_an_explicit_spec() {
        let follow = state(Scene {
            present: true,
            tile: 7.0,
        });
        let spec = switch_to_state_and_follow_with(
            Scene {
                present: true,
                tile: -50.0,
            },
            "tile",
            follow,
        );
        let mut behavior = spec.compile(&tool_context());
        assert_eq!(
            behavior(Pointer::from(Vec2::new(30.0, 0.0)))
                .drop_state
                .tile,
            7.0
        );
    }

    #[test]
    fn a_missing_element_shows_the_new_state() {
        let spec = switch_to_state_and_follow(
            Scene {
                present: false,
                tile: 1.0,
            },
            "tile",
        );
        let mut behavior = spec.compile(&tool_context());
        let frame = behavior(Pointer::from(Vec2::ZERO));
        assert_eq!(frame.drop_state.tile, 1.0);
        assert!(frame.gap.is_infinite());
    }
}
