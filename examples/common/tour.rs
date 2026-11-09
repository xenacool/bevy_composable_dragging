//! A scripted pointer that plays a tour: several drags, clicks and pauses in a row, for the
//! animations of the larger examples.
//!
//! Like [`ScriptedDragPlugin`](super::script::ScriptedDragPlugin) it only runs when
//! `DSD_SCREENSHOT` is set, sends real pointer input at the speed of a finger, and tells the
//! capture plugin when the animation starts and ends. Unlike it, a tour finds where to press and
//! where to go when a step begins, from the world as it is then: the pixel of a node of the view,
//! or of a point in the scene, so a step can aim at something an earlier step has made.

use std::collections::VecDeque;

use bevy::camera::NormalizedRenderTarget;
use bevy::picking::pointer::{Location, PointerAction, PointerButton, PointerId, PointerInput};
use bevy::prelude::*;
use bevy::window::{PrimaryWindow, WindowRef};
use bevy_composable_dragging::DragModel;

use super::script::DragTimeline;

/// Frames to wait before the tour starts, so shaders compile and picking sees the scene.
const WARM_UP_FRAMES: u32 = 5;

/// Pointer speed in screen heights per second, as for a finger on a touch screen.
const FINGER_SPEED: f32 = 0.67;

/// The shortest time a glide takes, in seconds, so a short move still reads as a move.
const MIN_GLIDE: f32 = 0.25;

/// Where a step aims, in pixels from the top left of the window, found when the step begins.
/// `None` waits for the target to exist, for example while the camera is not placed yet.
pub type Point = Box<dyn Fn(&mut World) -> Option<Vec2> + Send + Sync>;

/// A fixed pixel of the window.
pub fn pixel(at: Vec2) -> Point {
    Box::new(move |_| Some(at))
}

/// The pixel where the camera marked `C` shows the world point that `at` finds.
pub fn through_camera<C: Component>(
    at: impl Fn(&mut World) -> Option<Vec3> + Send + Sync + 'static,
) -> Point {
    Box::new(move |world| {
        let target = at(world)?;
        let (camera, transform) = world
            .query_filtered::<(&Camera, &GlobalTransform), With<C>>()
            .single(world)
            .ok()?;
        camera.world_to_viewport(transform, target).ok()
    })
}

/// The pixel where the camera marked `C` shows the node `id` of the view of the state `S`.
pub fn node<S: Clone + Send + Sync + 'static, C: Component>(id: &'static str) -> Point {
    through_camera::<C>(move |world| world.resource::<DragModel<S>>().view().world_position(id))
}

/// One step of a tour.
pub enum Step {
    /// Does nothing for this many seconds.
    Wait(f32),
    /// Moves the pointer to the point, with ease-in-out, at the speed of a finger.
    Glide(Point),
    /// Presses the primary button.
    Press,
    /// Releases the primary button.
    Release,
}

/// The steps of a tour, in order.
#[derive(Default)]
pub struct Tour(VecDeque<Step>);

impl Tour {
    /// A tour that does nothing yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a pause of `seconds`.
    pub fn wait(mut self, seconds: f32) -> Self {
        self.0.push_back(Step::Wait(seconds));
        self
    }

    /// Adds a glide of the pointer to `to`.
    pub fn glide(mut self, to: Point) -> Self {
        self.0.push_back(Step::Glide(to));
        self
    }

    /// Adds a press.
    pub fn press(mut self) -> Self {
        self.0.push_back(Step::Press);
        self
    }

    /// Adds a release.
    pub fn release(mut self) -> Self {
        self.0.push_back(Step::Release);
        self
    }

    /// Glides to `from`, presses, glides to each of `via` in turn and releases at the last.
    pub fn drag(self, from: Point, via: impl IntoIterator<Item = Point>) -> Self {
        via.into_iter()
            .fold(self.glide(from).press(), Self::glide)
            .release()
    }

    /// Glides to `at`, then presses and releases without moving.
    pub fn click(self, at: Point) -> Self {
        self.glide(at).press().release()
    }
}

/// Plays a [`Tour`].
pub struct TourPlugin {
    tour: fn() -> Tour,
}

impl TourPlugin {
    /// Plays the tour that `tour` builds.
    pub fn new(tour: fn() -> Tour) -> Self {
        Self { tour }
    }
}

impl Plugin for TourPlugin {
    fn build(&self, app: &mut App) {
        if std::env::var("DSD_SCREENSHOT").is_err() {
            return;
        }
        app.insert_resource(Playing {
            steps: (self.tour)().0,
            active: None,
            cursor: None,
            warm_up: 0,
            started: false,
            finished: false,
        })
        .add_systems(Update, play);
    }
}

enum Active {
    Wait {
        until: f32,
    },
    Glide {
        from: Vec2,
        to: Vec2,
        start: f32,
        duration: f32,
    },
}

#[derive(Resource)]
struct Playing {
    steps: VecDeque<Step>,
    active: Option<Active>,
    cursor: Option<Vec2>,
    warm_up: u32,
    started: bool,
    finished: bool,
}

fn ease_in_out(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

fn send(world: &mut World, target: &NormalizedRenderTarget, position: Vec2, action: PointerAction) {
    world.write_message(PointerInput::new(
        PointerId::Mouse,
        Location {
            target: target.clone(),
            position,
        },
        action,
    ));
}

fn play(world: &mut World) {
    world.resource_scope(|world, mut playing: Mut<Playing>| {
        if playing.warm_up < WARM_UP_FRAMES {
            playing.warm_up += 1;
            return;
        }
        let Ok((window, height)) = world
            .query_filtered::<(Entity, &Window), With<PrimaryWindow>>()
            .single(world)
            .map(|(entity, window)| (entity, window.height()))
        else {
            return;
        };
        let Some(target) = WindowRef::Primary.normalize(Some(window)) else {
            return;
        };
        let target = NormalizedRenderTarget::Window(target);
        let now = world.resource::<Time<Real>>().elapsed_secs();
        if !playing.started {
            playing.started = true;
            world.insert_resource(DragTimeline { end: f32::INFINITY });
        }

        if playing.active.is_none() {
            let Some(step) = playing.steps.pop_front() else {
                if !playing.finished {
                    playing.finished = true;
                    world.insert_resource(DragTimeline { end: now });
                }
                return;
            };
            match step {
                Step::Wait(seconds) => {
                    playing.active = Some(Active::Wait {
                        until: now + seconds,
                    });
                }
                Step::Glide(to) => {
                    let Some(to) = to(world) else {
                        playing.steps.push_front(Step::Glide(to));
                        return;
                    };
                    let from = playing.cursor.unwrap_or(to);
                    let duration = (from.distance(to) / (FINGER_SPEED * height)).max(MIN_GLIDE);
                    playing.active = Some(Active::Glide {
                        from,
                        to,
                        start: now,
                        duration,
                    });
                }
                Step::Press => {
                    if let Some(at) = playing.cursor {
                        send(
                            world,
                            &target,
                            at,
                            PointerAction::Press(PointerButton::Primary),
                        );
                    }
                    return;
                }
                Step::Release => {
                    if let Some(at) = playing.cursor {
                        send(
                            world,
                            &target,
                            at,
                            PointerAction::Release(PointerButton::Primary),
                        );
                    }
                    return;
                }
            }
        }

        match playing.active {
            Some(Active::Wait { until }) => {
                if now >= until {
                    playing.active = None;
                }
            }
            Some(Active::Glide {
                from,
                to,
                start,
                duration,
            }) => {
                let t = ((now - start) / duration).clamp(0.0, 1.0);
                let position = from.lerp(to, ease_in_out(t));
                let delta = playing.cursor.map_or(Vec2::ZERO, |last| position - last);
                send(world, &target, position, PointerAction::Move { delta });
                playing.cursor = Some(position);
                if t >= 1.0 {
                    playing.active = None;
                }
            }
            None => {}
        }
    });
}
