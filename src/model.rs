//! The interface state together with the view that is currently shown.

use std::sync::Arc;

use bevy::ecs::entity::Entity;
use bevy::ecs::resource::Resource;
use bevy::math::Vec2;

use crate::behavior::{DragBehavior, DragContext, Frame, ViewFn};
use crate::transition::{DEFAULT_DURATION, Transition};
use crate::view::{NodeId, View};

/// A pointer press that may turn into a drag.
#[cfg_attr(not(feature = "pbr"), allow(dead_code))]
struct PendingDrag {
    node: NodeId,
    camera: Entity,
    pointer: Vec2,
}

/// How many times one pointer position may chain into a new drag before the engine stops.
const MAX_CHAINS: usize = 16;

/// A drag in progress.
#[cfg_attr(not(feature = "pbr"), allow(dead_code))]
struct Session<S> {
    node: NodeId,
    camera: Entity,
    behavior: DragBehavior<S>,
    grab_offset: Vec2,
    drop_state: S,
    /// How long the animation after a drop takes; `None` uses the model's default.
    drop_transition: Option<f32>,
    /// The branch of the last frame.
    branch: Option<u64>,
    /// A running blend after the branch changed; its target is the latest frame's view.
    blend: Option<Transition<S>>,
}

/// The state of a draggable interface, the view that is currently shown, and the drag in
/// progress, if any.
///
/// [`DraggablePlugin`](crate::DraggablePlugin) inserts it as a resource. Read the state with
/// [`state`](Self::state); change it from game logic with [`set_state`](Self::set_state).
#[derive(Resource)]
pub struct DragModel<S: Send + Sync + 'static> {
    state: S,
    view_fn: ViewFn<S>,
    shown: View<S>,
    pending: Option<PendingDrag>,
    session: Option<Session<S>>,
    transition: Option<Transition<S>>,
    transition_duration: f32,
}

impl<S: Clone + Send + Sync + 'static> DragModel<S> {
    /// Creates a model that shows `state` rendered with `view_fn`.
    pub fn new(state: S, view_fn: impl Fn(&S) -> View<S> + Send + Sync + 'static) -> Self {
        Self::from_view_fn(state, Arc::new(view_fn))
    }

    pub(crate) fn from_view_fn(state: S, view_fn: ViewFn<S>) -> Self {
        let shown = view_fn(&state);
        Self {
            state,
            view_fn,
            shown,
            pending: None,
            session: None,
            transition: None,
            transition_duration: DEFAULT_DURATION,
        }
    }

    /// The current state. While a drag is in progress this is the state the drag started from.
    pub fn state(&self) -> &S {
        &self.state
    }

    /// Replaces the state, cancelling any drag in progress.
    pub fn set_state(&mut self, state: S) {
        self.pending = None;
        self.session = None;
        self.transition = None;
        self.shown = (self.view_fn)(&state);
        self.state = state;
    }

    /// The view that should be on screen right now.
    pub fn view(&self) -> &View<S> {
        &self.shown
    }

    /// Sets how long the animation after a drop takes. Zero snaps instantly.
    pub fn set_transition_duration(&mut self, seconds: f32) {
        self.transition_duration = seconds.max(0.0);
    }

    /// Whether the view is animating, either towards the state a drag was dropped on or between
    /// branches of the drag in progress.
    pub fn is_transitioning(&self) -> bool {
        self.transition.is_some()
            || self
                .session
                .as_ref()
                .is_some_and(|session| session.blend.is_some())
    }

    /// Advances the drop animation by `seconds`.
    pub(crate) fn advance_transition(&mut self, seconds: f32) {
        if let Some(session) = self.session.as_mut()
            && let Some(blend) = session.blend.as_mut()
        {
            blend.advance(seconds);
            if blend.is_finished() {
                self.shown = blend.target().clone();
                session.blend = None;
            } else {
                self.shown = pinned(blend.current(), blend.target(), &session.node);
            }
            return;
        }
        let Some(transition) = self.transition.as_mut() else {
            return;
        };
        transition.advance(seconds);
        if transition.is_finished() {
            self.shown = (self.view_fn)(&self.state);
            self.transition = None;
        } else {
            self.shown = transition.current();
        }
    }

    /// Whether a drag is in progress.
    pub fn is_dragging(&self) -> bool {
        self.session.is_some()
    }

    /// The id of the node being dragged.
    pub fn dragged_node(&self) -> Option<&str> {
        self.session.as_ref().map(|session| session.node.as_ref())
    }

    /// Remembers a press on `node` so that a following drag can start. Returns whether the node
    /// can be dragged.
    #[cfg_attr(not(feature = "pbr"), allow(dead_code))]
    pub(crate) fn press(&mut self, node: &str, camera: Entity, pointer: Vec2) -> bool {
        let interactive = self
            .transition
            .as_ref()
            .map_or(&self.shown, Transition::target);
        let candidate = interactive
            .get(node)
            .filter(|candidate| candidate.drag.is_some() && self.session.is_none())
            .map(|candidate| candidate.id.clone());
        let draggable = candidate.is_some();
        self.pending = candidate.map(|node| PendingDrag {
            node,
            camera,
            pointer,
        });
        draggable
    }

    /// The node of the pending press.
    #[cfg_attr(not(feature = "pbr"), allow(dead_code))]
    pub(crate) fn pending_node(&self) -> Option<&str> {
        self.pending.as_ref().map(|pending| pending.node.as_ref())
    }

    /// Turns the pending press into a drag. Returns the camera the pointer is projected through.
    #[cfg_attr(not(feature = "pbr"), allow(dead_code))]
    pub(crate) fn start_drag(&mut self) -> Option<Entity> {
        let pending = self.pending.take()?;
        let base = (self.view_fn)(&self.state);
        let factory = base.get(&pending.node)?.drag.clone()?;
        let grab_offset = self
            .shown
            .position(&pending.node)
            .or_else(|| base.position(&pending.node))
            .map_or(Vec2::ZERO, |anchor| pending.pointer - anchor);
        let context = DragContext::new(
            Arc::clone(&self.view_fn),
            pending.node.clone(),
            self.state.clone(),
        )
        .with_grab_offset(grab_offset);
        let behavior = factory().compile(&context);
        self.transition = None;
        self.session = Some(Session {
            node: pending.node,
            camera: pending.camera,
            behavior,
            grab_offset,
            drop_state: self.state.clone(),
            drop_transition: None,
            branch: None,
            blend: None,
        });
        Some(pending.camera)
    }

    /// The camera of the drag in progress.
    #[cfg_attr(not(feature = "pbr"), allow(dead_code))]
    pub(crate) fn drag_camera(&self) -> Option<Entity> {
        self.session.as_ref().map(|session| session.camera)
    }

    /// Updates the preview for a new pointer position (in world coordinates).
    #[cfg_attr(not(feature = "pbr"), allow(dead_code))]
    pub(crate) fn drive(&mut self, pointer: Vec2) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        let first = (session.behavior)(pointer - session.grab_offset);
        let Chained {
            frame,
            followed,
            transition: chain_transition,
        } = follow_chains(&self.view_fn, session, &mut self.state, pointer, first);
        session.drop_state = frame.drop_state;
        session.drop_transition = frame.drop_transition;
        let switched = followed || session.branch.is_some_and(|branch| branch != frame.branch);
        session.branch = Some(frame.branch);
        if switched {
            let duration = chain_transition
                .or(frame.branch_transition)
                .unwrap_or(self.transition_duration);
            session.blend = (duration > 0.0)
                .then(|| Transition::new(self.shown.clone(), frame.view.clone(), duration));
        } else if let Some(blend) = session.blend.as_mut() {
            blend.retarget(frame.view.clone());
        }
        self.shown = match session.blend.as_ref() {
            Some(blend) => pinned(blend.current(), blend.target(), &session.node),
            None => frame.view,
        };
    }

    /// Ends the drag by switching to the state the last preview pointed at.
    #[cfg_attr(not(feature = "pbr"), allow(dead_code))]
    pub(crate) fn drop_drag(&mut self) {
        self.pending = None;
        if let Some(session) = self.session.take() {
            self.state = session.drop_state;
            let target = (self.view_fn)(&self.state);
            let duration = session
                .drop_transition
                .map_or(self.transition_duration, |seconds| seconds.max(0.0));
            if duration > 0.0 {
                let from = self.shown.clone();
                self.transition = Some(Transition::new(from, target, duration));
            } else {
                self.shown = target;
            }
        }
    }
}

/// The outcome of [`follow_chains`].
struct Chained<S> {
    /// The frame to show.
    frame: Frame<S>,
    /// Whether the drag chained into a new one.
    followed: bool,
    /// The blend time the last chaining asked for.
    transition: Option<f32>,
}

/// Continues the drag from the state of every frame that asks for chaining: the state becomes
/// the new start state and the session carries on with the chosen node and spec. A request that
/// cannot be followed, because the node or its spec is missing, is dropped.
fn follow_chains<S: Clone + Send + Sync + 'static>(
    view_fn: &ViewFn<S>,
    session: &mut Session<S>,
    state: &mut S,
    pointer: Vec2,
    mut frame: Frame<S>,
) -> Chained<S> {
    let mut followed = false;
    let mut transition = None;
    for _ in 0..MAX_CHAINS {
        let Some(chain) = frame.chain.take() else {
            break;
        };
        let new_state = frame.drop_state.clone();
        let node = chain.dragged.unwrap_or_else(|| session.node.clone());
        let view = view_fn(&new_state);
        let Some(spec) = chain
            .follow
            .or_else(|| view.get(&node)?.drag.as_ref().map(|factory| factory()))
        else {
            break;
        };
        let context = DragContext::new(Arc::clone(view_fn), node.clone(), new_state.clone())
            .with_grab_offset(session.grab_offset);
        session.behavior = spec.compile(&context);
        session.node = node;
        *state = new_state;
        followed = true;
        transition = chain.transition;
        frame = (session.behavior)(pointer - session.grab_offset);
    }
    Chained {
        frame,
        followed,
        transition,
    }
}

/// The blended `view` with the dragged node taken straight from `latest`, so it never lags
/// behind the pointer while the rest of the preview animates.
fn pinned<S>(mut view: View<S>, latest: &View<S>, dragged: &str) -> View<S> {
    if let Some(position) = latest.position(dragged) {
        view.set_position(dragged, position);
    }
    view
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::{between, closest_states};
    use crate::view::ViewNode;

    #[derive(Clone, Copy, PartialEq, Debug)]
    enum Switch {
        Off,
        On,
    }

    fn switch_view(state: Switch) -> View<Switch> {
        let x = match state {
            Switch::Off => -50.0,
            Switch::On => 50.0,
        };
        View::new().with(
            ViewNode::circle("knob", 20.0)
                .at(Vec2::new(x, 0.0))
                .draggable(|| between([Switch::Off, Switch::On])),
        )
    }

    fn model() -> DragModel<Switch> {
        DragModel::new(Switch::Off, |state| switch_view(*state))
    }

    #[test]
    fn dragging_previews_and_drops_on_the_nearer_state() {
        let mut model = model();
        model.set_transition_duration(0.0);
        assert!(model.press("knob", Entity::PLACEHOLDER, Vec2::new(-50.0, 0.0)));
        assert!(model.start_drag().is_some());
        assert_eq!(model.dragged_node(), Some("knob"));

        model.drive(Vec2::new(0.0, 0.0));
        let preview = model.view().position("knob").unwrap();
        assert!(preview.x.abs() < 1e-3, "knob should be halfway: {preview}");

        model.drive(Vec2::new(30.0, 20.0));
        model.drop_drag();
        assert_eq!(*model.state(), Switch::On);
        assert!(!model.is_dragging());
        assert!((model.view().position("knob").unwrap().x - 50.0).abs() < 1e-3);
    }

    #[test]
    fn dropping_animates_from_the_preview_to_the_new_layout() {
        let mut model = model();
        model.press("knob", Entity::PLACEHOLDER, Vec2::new(-50.0, 0.0));
        model.start_drag();
        model.drive(Vec2::new(30.0, 0.0));
        let released_at = model.view().position("knob").unwrap().x;
        model.drop_drag();
        assert!(model.is_transitioning());
        assert_eq!(*model.state(), Switch::On);

        model.advance_transition(0.05);
        let mid = model.view().position("knob").unwrap().x;
        assert!(mid > released_at && mid < 50.0);

        model.advance_transition(1.0);
        assert!(!model.is_transitioning());
        assert!((model.view().position("knob").unwrap().x - 50.0).abs() < 1e-3);
    }

    #[test]
    fn a_node_can_be_grabbed_again_while_animating() {
        let mut model = model();
        model.press("knob", Entity::PLACEHOLDER, Vec2::new(-50.0, 0.0));
        model.start_drag();
        model.drive(Vec2::new(30.0, 0.0));
        model.drop_drag();
        model.advance_transition(0.05);
        let visible = model.view().position("knob").unwrap();
        assert!(model.press("knob", Entity::PLACEHOLDER, visible));
        assert!(model.start_drag().is_some());
        assert!(!model.is_transitioning());
    }

    #[test]
    fn the_grab_offset_keeps_the_knob_under_the_cursor() {
        let mut model = model();
        // Grab the knob 10 units to the right of its center.
        model.press("knob", Entity::PLACEHOLDER, Vec2::new(-40.0, 0.0));
        model.start_drag();
        model.drive(Vec2::new(-30.0, 0.0));
        assert!((model.view().position("knob").unwrap().x + 40.0).abs() < 1e-3);
    }

    #[test]
    fn dropping_near_the_start_keeps_the_state() {
        let mut model = model();
        model.press("knob", Entity::PLACEHOLDER, Vec2::new(-50.0, 0.0));
        model.start_drag();
        model.drive(Vec2::new(-40.0, 0.0));
        model.drop_drag();
        assert_eq!(*model.state(), Switch::Off);
    }

    #[test]
    fn pressing_a_missing_node_does_not_start_a_drag() {
        let mut model = model();
        assert!(!model.press("nothing", Entity::PLACEHOLDER, Vec2::ZERO));
        assert!(model.start_drag().is_none());
    }

    #[test]
    fn set_state_cancels_a_drag() {
        let mut model = model();
        model.press("knob", Entity::PLACEHOLDER, Vec2::new(-50.0, 0.0));
        model.start_drag();
        model.set_state(Switch::On);
        assert!(!model.is_dragging());
        assert_eq!(*model.state(), Switch::On);
    }

    /// Two lanes the item can be dragged between; the state is the item's lane.
    #[derive(Clone)]
    struct Lane(usize);

    /// The item sits in its lane; a bystander moves the opposite way.
    fn lanes_view(lane: &Lane) -> View<Lane> {
        View::new()
            .with(
                ViewNode::circle("item", 10.0)
                    .at(Vec2::new(0.0, lane.0 as f32 * 100.0))
                    .draggable(|| closest_states([Lane(0), Lane(1)])),
            )
            .with(ViewNode::circle("bystander", 10.0).at(Vec2::new(50.0, -(lane.0 as f32) * 100.0)))
    }

    #[test]
    fn switching_branches_blends_the_preview_over_time() {
        let mut model = DragModel::new(Lane(0), lanes_view);
        model.press("item", Entity::PLACEHOLDER, Vec2::ZERO);
        model.start_drag();
        model.drive(Vec2::new(0.0, 10.0));
        assert!(!model.is_transitioning());
        assert_eq!(model.view().position("item").unwrap().y, 0.0);

        // Crossing to the other lane starts a blend for the rest of the view instead of a jump.
        model.drive(Vec2::new(0.0, 90.0));
        assert!(model.is_transitioning());
        let bystander = |model: &DragModel<Lane>| model.view().position("bystander").unwrap().y;
        assert!(
            bystander(&model).abs() < 1e-3,
            "the blend starts at the old preview"
        );
        // The dragged node never lags behind: it is already in its new lane.
        assert!((model.view().position("item").unwrap().y - 100.0).abs() < 1e-3);

        model.advance_transition(0.05);
        let middle = bystander(&model);
        assert!(middle < 0.0 && middle > -100.0, "blending: {middle}");

        model.advance_transition(1.0);
        assert!(!model.is_transitioning());
        assert!((bystander(&model) + 100.0).abs() < 1e-3);
    }

    /// A walker on cells 100 apart that can move one cell per drag step.
    #[derive(Clone, Copy, PartialEq, Debug)]
    struct Walker(i32);

    fn walker_view(walker: Walker) -> View<Walker> {
        let step = move |delta: i32| Walker((walker.0 + delta).clamp(0, 3));
        View::new().with(
            ViewNode::circle("walker", 10.0)
                .at(Vec2::new(walker.0 as f32 * 100.0, 0.0))
                .draggable(move || {
                    closest_states([step(-1), step(0), step(1)])
                        .with_chaining()
                        .with_drop_transition(0.0)
                }),
        )
    }

    #[test]
    fn chaining_walks_across_several_states_in_one_drag() {
        let mut model = DragModel::new(Walker(0), |walker| walker_view(*walker));
        model.press("walker", Entity::PLACEHOLDER, Vec2::ZERO);
        model.start_drag();
        model.drive(Vec2::new(240.0, 0.0));
        assert!(model.is_dragging());
        assert_eq!(*model.state(), Walker(2), "each chained state is committed");
        assert!(
            model.is_transitioning(),
            "the preview blends into the chain"
        );

        model.drop_drag();
        assert_eq!(*model.state(), Walker(2));
    }

    #[test]
    fn a_spec_chooses_its_own_drop_duration() {
        let mut model = DragModel::new(Walker(0), |walker| walker_view(*walker));
        model.press("walker", Entity::PLACEHOLDER, Vec2::ZERO);
        model.start_drag();
        model.drive(Vec2::new(90.0, 0.0));
        model.drop_drag();
        assert_eq!(*model.state(), Walker(1));
        assert!(
            !model.is_transitioning(),
            "a zero drop transition is instant"
        );
    }

    #[test]
    fn a_zero_transition_duration_switches_branches_instantly() {
        let mut model = DragModel::new(Lane(0), lanes_view);
        model.set_transition_duration(0.0);
        model.press("item", Entity::PLACEHOLDER, Vec2::ZERO);
        model.start_drag();
        model.drive(Vec2::new(0.0, 10.0));
        model.drive(Vec2::new(0.0, 90.0));
        assert!(!model.is_transitioning());
        assert!((model.view().position("bystander").unwrap().y + 100.0).abs() < 1e-3);
    }
}
