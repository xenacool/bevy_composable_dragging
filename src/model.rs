//! The interface state together with the view that is currently shown.

use bevy::ecs::entity::Entity;
use bevy::ecs::resource::Resource;
use bevy::math::{Dir3, Vec3};
use bevy::prelude::Ray3d;

use crate::behavior::{DragBehavior, DragContext, Frame, ViewFn};
use crate::space::{DragSpace, Pointer};
use crate::transition::{DEFAULT_DURATION, Transition};
use crate::view::{NodeId, View};

/// A pointer press that may turn into a drag.
#[cfg_attr(not(feature = "pbr"), allow(dead_code))]
struct PendingDrag {
    node: NodeId,
    camera: Entity,
    /// The space of the drag, with a camera-facing space already fixed to a plane.
    space: DragSpace,
    /// Where the press landed in the space.
    point: Vec3,
}

/// How many times one pointer position may chain into a new drag before the engine stops.
const MAX_CHAINS: usize = 16;

/// A drag in progress.
#[cfg_attr(not(feature = "pbr"), allow(dead_code))]
struct Session<S> {
    node: NodeId,
    camera: Entity,
    behavior: DragBehavior<S>,
    space: DragSpace,
    grab_offset: Vec3,
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
/// `DraggablePlugin` inserts it as a resource. Read the state with
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
    pub fn new(state: S, view_fn: impl Fn(&S) -> View<S> + Clone + Send + Sync + 'static) -> Self {
        Self::from_view_fn(state, ViewFn::new(view_fn))
    }

    pub(crate) fn from_view_fn(state: S, view_fn: ViewFn<S>) -> Self {
        let shown = view_fn.call(&state);
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
        self.shown = self.view_fn.call(&state);
        self.state = state;
    }

    /// The view that should be on screen right now.
    pub fn view(&self) -> &View<S> {
        &self.shown
    }

    /// The view that the one on screen settles on: the target of the animation that is running,
    /// or the view on screen when none is. The scenes of the nodes are read from it, since a
    /// blend of two views carries none.
    #[cfg_attr(not(feature = "pbr"), allow(dead_code))]
    pub(crate) fn settled_view(&self) -> &View<S> {
        self.session
            .as_ref()
            .and_then(|session| session.blend.as_ref())
            .or(self.transition.as_ref())
            .map_or(&self.shown, Transition::target)
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
            self.shown = self.view_fn.call(&self.state);
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

    /// Remembers a press so that a following drag can start. Returns whether a node can be
    /// dragged.
    ///
    /// The node is `hint`, the node the picking backend reported, or the nearest node that `ray`
    /// hits (see [`View::hit_test_in`]) when the view has no node `hint`. Only nodes whose render
    /// layer `visible` accepts can be hit, which keeps a HUD camera and a scene camera from
    /// picking each other's nodes. A node that is not draggable gives way to its nearest
    /// draggable ancestor. The drag happens in the space of that node, or in `default_space` when
    /// it has none; a camera-facing space is fixed to the plane through the node that faces
    /// against `camera_forward`.
    #[cfg_attr(not(feature = "pbr"), allow(dead_code))]
    pub(crate) fn press(
        &mut self,
        hint: &str,
        camera: Entity,
        ray: Ray3d,
        camera_forward: Dir3,
        default_space: DragSpace,
        visible: impl Fn(usize) -> bool,
    ) -> bool {
        let interactive = self
            .transition
            .as_ref()
            .map_or(&self.shown, Transition::target);
        let picked = if interactive.get(hint).is_some() {
            interactive.draggable_ancestor(hint)
        } else {
            interactive
                .hit_test_in(ray, visible)
                .and_then(|hit| interactive.draggable_ancestor(&hit.id))
        }
        .filter(|_| self.session.is_none());
        self.pending = picked.map(|node| {
            let home = interactive.world_position(&node.id).unwrap_or(Vec3::ZERO);
            let space = node
                .drag_space
                .unwrap_or(default_space)
                .fixed(camera_forward, home);
            PendingDrag {
                node: node.id.clone(),
                camera,
                space,
                point: space
                    .resolve(ray)
                    .unwrap_or_else(|| space.project_point(home)),
            }
        });
        self.pending.is_some()
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
        let base = self.view_fn.call(&self.state);
        let spec = base.get(&pending.node)?.drag_spec(&self.state)?;
        let grab_offset = self
            .shown
            .world_position(&pending.node)
            .or_else(|| base.world_position(&pending.node))
            .map_or(Vec3::ZERO, |anchor| {
                pending.point - pending.space.project_point(anchor)
            });
        let context = DragContext::with_view_fn(
            self.view_fn.clone(),
            pending.node.clone(),
            self.state.clone(),
        )
        .with_grab_offset(grab_offset)
        .with_space(pending.space);
        let behavior = spec.compile(&context);
        self.transition = None;
        self.session = Some(Session {
            node: pending.node,
            camera: pending.camera,
            behavior,
            space: pending.space,
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

    /// Updates the preview for a new pointer ray. A ray that does not reach the space of the
    /// drag keeps the last frame.
    #[cfg_attr(not(feature = "pbr"), allow(dead_code))]
    pub(crate) fn drive(&mut self, ray: Ray3d) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        let Some(point) = session.space.resolve(ray) else {
            return;
        };
        let pointer = Pointer { ray, point };
        let first = (session.behavior)(pointer.moved_to(point - session.grab_offset));
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

    /// Completes a press that was released without moving, a click: the node is dragged and
    /// dropped on the spot, so a button acts the same whether or not the pointer moved. Returns
    /// whether a node was dropped.
    #[cfg_attr(not(feature = "pbr"), allow(dead_code))]
    pub(crate) fn tap(&mut self, ray: Ray3d) -> bool {
        if self.start_drag().is_none() {
            return false;
        }
        self.drive(ray);
        self.drop_drag();
        true
    }

    /// Ends the drag by switching to the state the last preview pointed at.
    #[cfg_attr(not(feature = "pbr"), allow(dead_code))]
    pub(crate) fn drop_drag(&mut self) {
        self.pending = None;
        if let Some(session) = self.session.take() {
            self.state = session.drop_state;
            let target = self.view_fn.call(&self.state);
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
    pointer: Pointer,
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
        let view = view_fn.call(&new_state);
        let Some(spec) = chain
            .follow
            .or_else(|| view.get(&node)?.drag_spec(&new_state))
        else {
            break;
        };
        let context = DragContext::with_view_fn(view_fn.clone(), node.clone(), new_state.clone())
            .with_grab_offset(session.grab_offset)
            .with_space(session.space);
        session.behavior = spec.compile(&context);
        session.node = node;
        *state = new_state;
        followed = true;
        transition = chain.transition;
        frame = (session.behavior)(pointer.moved_to(pointer.point - session.grab_offset));
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
    if let (Some(latest), Some(mut world)) = (
        latest.world_transform(dragged),
        view.world_transform(dragged),
    ) {
        world.translation = latest.translation;
        view.set_world_transform(dragged, world);
    }
    view
}

#[cfg(test)]
mod tests {
    use bevy::math::Vec2;
    use bevy::prelude::Transform;

    use super::*;
    use crate::ops::{between, closest_states};
    use crate::view::ViewNode;

    /// The ray of a pointer that looks straight down at `point` of the XY plane.
    fn ray_at(point: Vec2) -> Ray3d {
        Ray3d::new(point.extend(10.0), Dir3::NEG_Z)
    }

    fn press_at<S: Clone + Send + Sync + 'static>(
        model: &mut DragModel<S>,
        node: &str,
        point: Vec2,
    ) -> bool {
        model.press(
            node,
            Entity::PLACEHOLDER,
            ray_at(point),
            Dir3::NEG_Z,
            DragSpace::default(),
            |_| true,
        )
    }

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
                .at_xy(Vec2::new(x, 0.0))
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
        assert!(press_at(&mut model, "knob", Vec2::new(-50.0, 0.0)));
        assert!(model.start_drag().is_some());
        assert_eq!(model.dragged_node(), Some("knob"));

        model.drive(ray_at(Vec2::new(0.0, 0.0)));
        let preview = model.view().world_position("knob").unwrap();
        assert!(preview.x.abs() < 1e-3, "knob should be halfway: {preview}");

        model.drive(ray_at(Vec2::new(30.0, 20.0)));
        model.drop_drag();
        assert_eq!(*model.state(), Switch::On);
        assert!(!model.is_dragging());
        assert!((model.view().world_position("knob").unwrap().x - 50.0).abs() < 1e-3);
    }

    #[test]
    fn dropping_animates_from_the_preview_to_the_new_layout() {
        let mut model = model();
        press_at(&mut model, "knob", Vec2::new(-50.0, 0.0));
        model.start_drag();
        model.drive(ray_at(Vec2::new(30.0, 0.0)));
        let released_at = model.view().world_position("knob").unwrap().x;
        model.drop_drag();
        assert!(model.is_transitioning());
        assert_eq!(*model.state(), Switch::On);

        model.advance_transition(0.05);
        let mid = model.view().world_position("knob").unwrap().x;
        assert!(mid > released_at && mid < 50.0);

        model.advance_transition(1.0);
        assert!(!model.is_transitioning());
        assert!((model.view().world_position("knob").unwrap().x - 50.0).abs() < 1e-3);
    }

    #[test]
    fn a_node_can_be_grabbed_again_while_animating() {
        let mut model = model();
        press_at(&mut model, "knob", Vec2::new(-50.0, 0.0));
        model.start_drag();
        model.drive(ray_at(Vec2::new(30.0, 0.0)));
        model.drop_drag();
        model.advance_transition(0.05);
        let visible = model.view().world_position("knob").unwrap();
        assert!(press_at(&mut model, "knob", visible.truncate()));
        assert!(model.start_drag().is_some());
        assert!(!model.is_transitioning());
    }

    #[test]
    fn the_grab_offset_keeps_the_knob_under_the_cursor() {
        let mut model = model();
        // Grab the knob 10 units to the right of its center.
        press_at(&mut model, "knob", Vec2::new(-40.0, 0.0));
        model.start_drag();
        model.drive(ray_at(Vec2::new(-30.0, 0.0)));
        assert!((model.view().world_position("knob").unwrap().x + 40.0).abs() < 1e-3);
    }

    #[test]
    fn dropping_near_the_start_keeps_the_state() {
        let mut model = model();
        press_at(&mut model, "knob", Vec2::new(-50.0, 0.0));
        model.start_drag();
        model.drive(ray_at(Vec2::new(-40.0, 0.0)));
        model.drop_drag();
        assert_eq!(*model.state(), Switch::Off);
    }

    #[test]
    fn a_press_released_without_moving_is_a_tap_that_drops_where_it_stands() {
        let mut model = model();
        model.set_transition_duration(0.0);
        assert!(press_at(&mut model, "knob", Vec2::new(-50.0, 0.0)));
        assert!(model.tap(ray_at(Vec2::new(-50.0, 0.0))));
        assert!(!model.is_dragging());
        assert_eq!(*model.state(), Switch::Off);
        assert!(press_at(&mut model, "knob", Vec2::new(-30.0, 0.0)));
        assert!(model.tap(ray_at(Vec2::new(-30.0, 0.0))));
        assert_eq!(*model.state(), Switch::Off);
        assert!(press_at(&mut model, "knob", Vec2::new(-50.0, 0.0)));
        model.start_drag();
        assert!(
            !model.tap(ray_at(Vec2::new(-50.0, 0.0))),
            "a drag has no tap"
        );
    }

    #[test]
    fn a_tap_without_a_press_does_nothing() {
        let mut model = model();
        assert!(!model.tap(ray_at(Vec2::ZERO)));
        assert!(!model.is_dragging());
    }

    #[test]
    fn pressing_a_missing_node_does_not_start_a_drag() {
        let mut model = model();
        assert!(!press_at(&mut model, "nothing", Vec2::ZERO));
        assert!(model.start_drag().is_none());
    }

    #[test]
    fn set_state_cancels_a_drag() {
        let mut model = model();
        press_at(&mut model, "knob", Vec2::new(-50.0, 0.0));
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
                    .at_xy(Vec2::new(0.0, lane.0 as f32 * 100.0))
                    .draggable(|| closest_states([Lane(0), Lane(1)])),
            )
            .with(
                ViewNode::circle("bystander", 10.0)
                    .at_xy(Vec2::new(50.0, -(lane.0 as f32) * 100.0)),
            )
    }

    #[test]
    fn switching_branches_blends_the_preview_over_time() {
        let mut model = DragModel::new(Lane(0), lanes_view);
        press_at(&mut model, "item", Vec2::ZERO);
        model.start_drag();
        model.drive(ray_at(Vec2::new(0.0, 10.0)));
        assert!(!model.is_transitioning());
        assert_eq!(model.view().world_position("item").unwrap().y, 0.0);

        // Crossing to the other lane starts a blend for the rest of the view instead of a jump.
        model.drive(ray_at(Vec2::new(0.0, 90.0)));
        assert!(model.is_transitioning());
        let bystander =
            |model: &DragModel<Lane>| model.view().world_position("bystander").unwrap().y;
        assert!(
            bystander(&model).abs() < 1e-3,
            "the blend starts at the old preview"
        );
        // The dragged node never lags behind: it is already in its new lane.
        assert!((model.view().world_position("item").unwrap().y - 100.0).abs() < 1e-3);

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
                .at_xy(Vec2::new(walker.0 as f32 * 100.0, 0.0))
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
        press_at(&mut model, "walker", Vec2::ZERO);
        model.start_drag();
        model.drive(ray_at(Vec2::new(240.0, 0.0)));
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
        press_at(&mut model, "walker", Vec2::ZERO);
        model.start_drag();
        model.drive(ray_at(Vec2::new(90.0, 0.0)));
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
        press_at(&mut model, "item", Vec2::ZERO);
        model.start_drag();
        model.drive(ray_at(Vec2::new(0.0, 10.0)));
        model.drive(ray_at(Vec2::new(0.0, 90.0)));
        assert!(!model.is_transitioning());
        assert!((model.view().world_position("bystander").unwrap().y + 100.0).abs() < 1e-3);
    }

    /// A state whose only job is to carry the drag of a knob that follows the pointer point.
    #[derive(Clone, PartialEq, Debug)]
    struct Probe;

    /// A knob at `home` that moves to the pointer point of its drag space, whatever the space is.
    fn probe_view(space: DragSpace, home: Vec3) -> impl Fn(&Probe) -> View<Probe> + Clone {
        move |_| {
            View::new().with(ViewNode::circle("knob", 1.0).at(home).draggable_in(
                space,
                move || {
                    crate::ops::custom(move |context| {
                        let context = context.clone();
                        Box::new(move |pointer| {
                            let mut view = context.render(&Probe);
                            view.set_world_transform(
                                "knob",
                                Transform::from_translation(pointer.point),
                            );
                            Frame::new(view, Probe, 0.0)
                        })
                    })
                },
            ))
        }
    }

    fn probe_model(space: DragSpace, home: Vec3) -> DragModel<Probe> {
        let mut model = DragModel::new(Probe, probe_view(space, home));
        model.set_transition_duration(0.0);
        model
    }

    fn press_ray(model: &mut DragModel<Probe>, ray: Ray3d, forward: Dir3) -> bool {
        model.press(
            "knob",
            Entity::PLACEHOLDER,
            ray,
            forward,
            DragSpace::default(),
            |_| true,
        )
    }

    fn toward(from: Vec3, to: Vec3) -> Ray3d {
        Ray3d::new(from, Dir3::new(to - from).unwrap())
    }

    #[test]
    fn a_drag_on_an_axis_stays_on_the_axis() {
        let space = DragSpace::axis(Vec3::ZERO, Dir3::X);
        let mut model = probe_model(space, Vec3::ZERO);
        assert!(press_ray(
            &mut model,
            toward(Vec3::new(0.1, 0.0, 10.0), Vec3::ZERO),
            Dir3::NEG_Z
        ));
        assert!(model.start_drag().is_some());
        model.drive(toward(Vec3::new(4.0, 3.0, 10.0), Vec3::new(4.0, 3.0, 0.0)));
        let knob = model.view().world_position("knob").unwrap();
        assert!(knob.distance(Vec3::new(4.0, 0.0, 0.0)) < 1e-3, "{knob}");
        model.drive(toward(
            Vec3::new(-2.0, 1.0, 5.0),
            Vec3::new(-2.0, -1.0, 0.0),
        ));
        let knob = model.view().world_position("knob").unwrap();
        assert!(
            knob.y.abs() < 1e-4 && knob.z.abs() < 1e-4 && knob.x < 0.0,
            "{knob}"
        );
    }

    #[test]
    fn a_drag_on_a_tilted_plane_stays_on_the_plane() {
        let normal = Dir3::new(Vec3::new(0.0, 1.0, 1.0)).unwrap();
        let origin = Vec3::new(0.0, 0.0, 1.0);
        let mut model = probe_model(DragSpace::plane(origin, normal), origin);
        assert!(press_ray(
            &mut model,
            toward(Vec3::new(0.0, 0.0, 10.0), origin),
            Dir3::NEG_Z
        ));
        model.start_drag();
        let ray = toward(Vec3::new(2.0, 3.0, 10.0), Vec3::new(2.0, 0.0, 0.0));
        model.drive(ray);
        let knob = model.view().world_position("knob").unwrap();
        assert!(normal.dot(knob - origin).abs() < 1e-4, "{knob}");
        let hit = DragSpace::plane(origin, normal).resolve(ray).unwrap();
        assert!(knob.distance(hit - Vec3::ZERO) < 1e-3);
    }

    #[test]
    fn a_screen_space_drags_in_a_plane_facing_the_camera() {
        let home = Vec3::new(0.0, 0.0, 3.0);
        let mut model = probe_model(DragSpace::screen(), home);
        assert!(press_ray(
            &mut model,
            toward(Vec3::new(0.0, 0.0, 10.0), home),
            Dir3::NEG_Z
        ));
        model.start_drag();
        model.drive(toward(Vec3::new(1.0, 2.0, 10.0), Vec3::new(1.0, 2.0, 0.0)));
        let knob = model.view().world_position("knob").unwrap();
        assert!(knob.distance(Vec3::new(1.0, 2.0, 3.0)) < 1e-3, "{knob}");
    }

    #[test]
    fn a_node_without_a_space_uses_the_default_one() {
        let mut model = DragModel::new(Probe, |_: &Probe| {
            View::new().with(ViewNode::circle("knob", 1.0).draggable(|| {
                crate::ops::custom(|context| {
                    let context = context.clone();
                    Box::new(move |pointer| {
                        let mut view = context.render(&Probe);
                        view.set_world_transform(
                            "knob",
                            Transform::from_translation(pointer.point),
                        );
                        Frame::new(view, Probe, 0.0)
                    })
                })
            }))
        });
        assert!(model.press(
            "knob",
            Entity::PLACEHOLDER,
            ray_at(Vec2::ZERO),
            Dir3::NEG_Z,
            DragSpace::world_plane(2.0),
            |_| true
        ));
        model.start_drag();
        model.drive(ray_at(Vec2::new(3.0, 1.0)));
        let knob = model.view().world_position("knob").unwrap();
        assert!(knob.distance(Vec3::new(3.0, 1.0, 2.0)) < 1e-4, "{knob}");
    }

    #[test]
    fn a_ray_that_misses_the_space_keeps_the_last_frame() {
        let mut model = probe_model(DragSpace::world_plane(0.0), Vec3::ZERO);
        assert!(press_ray(&mut model, ray_at(Vec2::ZERO), Dir3::NEG_Z));
        model.start_drag();
        model.drive(ray_at(Vec2::new(2.0, 0.0)));
        let before = model.view().world_position("knob").unwrap();
        model.drive(Ray3d::new(Vec3::new(5.0, 5.0, 1.0), Dir3::X));
        assert_eq!(model.view().world_position("knob").unwrap(), before);
    }

    #[test]
    fn the_grab_offset_is_a_vector_in_the_space() {
        let space = DragSpace::plane(Vec3::ZERO, Dir3::Z);
        let mut model = probe_model(space, Vec3::new(5.0, 0.0, 0.0));
        // The knob has diameter one, so a press at x = 5.4 hits it 0.4 from its center.
        assert!(press_ray(
            &mut model,
            ray_at(Vec2::new(5.4, 0.0)),
            Dir3::NEG_Z
        ));
        model.start_drag();
        model.drive(ray_at(Vec2::new(10.4, 0.0)));
        // The probe moves to the point it receives, which has the 0.4 offset removed.
        let knob = model.view().world_position("knob").unwrap();
        assert!(knob.distance(Vec3::new(10.0, 0.0, 0.0)) < 1e-4, "{knob}");
    }

    #[test]
    fn a_press_on_an_unknown_node_falls_back_to_the_ray() {
        let mut model = DragModel::new(Probe, |_: &Probe| {
            View::new().with(ViewNode::circle("knob", 1.0).draggable(|| crate::ops::state(Probe)))
        });
        assert!(model.press(
            "gone",
            Entity::PLACEHOLDER,
            ray_at(Vec2::ZERO),
            Dir3::NEG_Z,
            DragSpace::default(),
            |_| true
        ));
        assert_eq!(model.pending_node(), Some("knob"));
        assert!(!model.press(
            "gone",
            Entity::PLACEHOLDER,
            ray_at(Vec2::new(9.0, 9.0)),
            Dir3::NEG_Z,
            DragSpace::default(),
            |_| true
        ));
    }

    #[test]
    fn a_press_on_a_child_grabs_its_draggable_ancestor() {
        let mut model = DragModel::new(Probe, |_: &Probe| {
            View::new()
                .with(ViewNode::circle("parent", 1.0).draggable(|| crate::ops::state(Probe)))
                .with(
                    ViewNode::circle("child", 1.0)
                        .with_parent("parent")
                        .with_z(1.0),
                )
        });
        assert!(model.press(
            "child",
            Entity::PLACEHOLDER,
            ray_at(Vec2::ZERO),
            Dir3::NEG_Z,
            DragSpace::default(),
            |_| true
        ));
        assert_eq!(model.pending_node(), Some("parent"));
    }

    #[test]
    fn a_press_only_picks_nodes_on_the_layers_the_camera_shows() {
        fn press(model: &mut DragModel<Probe>, visible: fn(usize) -> bool) -> bool {
            model.press(
                "gone",
                Entity::PLACEHOLDER,
                ray_at(Vec2::ZERO),
                Dir3::NEG_Z,
                DragSpace::default(),
                visible,
            )
        }
        let mut model = DragModel::new(Probe, |_: &Probe| {
            View::new()
                .with(ViewNode::circle("scene", 1.0).draggable(|| crate::ops::state(Probe)))
                .with(
                    ViewNode::circle("hud", 1.0)
                        .on_layer(2)
                        .with_z(1.0)
                        .draggable(|| crate::ops::state(Probe)),
                )
        });
        // The HUD node is nearer to both cameras, but only its own camera can pick it.
        assert!(press(&mut model, |layer| layer == 2));
        assert_eq!(model.pending_node(), Some("hud"));
        assert!(press(&mut model, |layer| layer == 0));
        assert_eq!(model.pending_node(), Some("scene"));
        assert!(!press(&mut model, |layer| layer == 5));
    }
}
