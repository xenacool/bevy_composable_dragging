//! A virtual mouse for end-to-end tests of an example: it moves over the real app, presses and
//! releases, so everything between the pointer and the state runs: picking, the cameras, the drag
//! engine, the tweens and the view that follows.
//!
//! The app is the one of the example, with the default plugins but no window system and no GPU.

use std::marker::PhantomData;
use std::time::Duration;

use bevy::camera::RenderTarget;
use bevy::log::LogPlugin;
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::{Location, PointerAction, PointerButton, PointerId, PointerInput};
use bevy::prelude::*;
use bevy::render::RenderPlugin;
use bevy::render::settings::{RenderCreation, WgpuSettings};
use bevy::time::TimeUpdateStrategy;
use bevy::window::{PrimaryWindow, WindowRef, WindowResized};
use bevy::winit::WinitPlugin;
use bevy_composable_dragging::DragModel;

use super::HudCamera;

/// The length of a frame of the test app.
const FRAME: Duration = Duration::from_millis(16);

/// The most frames a tween of the HUD takes to finish.
const MAX_SETTLE: usize = 60;

/// An app with the state `S` in a `DragModel`, and the virtual mouse that drives it. `C` marks
/// the camera of the scene, as opposed to the camera of the HUD.
pub struct Mouse<S, C> {
    /// The app that the mouse drives.
    pub app: App,
    /// Where the pointer is, in pixels from the top left of the window.
    pub cursor: Vec2,
    scene: PhantomData<(S, C)>,
}

impl<S: Clone + Send + Sync + 'static, C: Component> Mouse<S, C> {
    /// An app with the default plugins but no window system and no GPU, to which `build` adds
    /// the example, ready to be driven.
    pub fn new(build: impl FnOnce(&mut App)) -> Self {
        let mut app = App::new();
        app.add_plugins(
            DefaultPlugins
                .set(RenderPlugin {
                    render_creation: RenderCreation::Automatic(Box::new(WgpuSettings {
                        backends: None,
                        ..default()
                    })),
                    ..default()
                })
                .disable::<WinitPlugin>()
                .disable::<LogPlugin>(),
        )
        .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME));
        build(&mut app);
        app.finish();
        app.cleanup();
        let mut mouse = Self {
            app,
            cursor: Vec2::ZERO,
            scene: PhantomData,
        };
        for _ in 0..MAX_SETTLE {
            mouse.app.update();
        }
        mouse
    }

    /// Runs frames until every tween has finished.
    pub fn settle(&mut self) {
        for _ in 0..MAX_SETTLE {
            self.app.update();
            if !self
                .app
                .world()
                .resource::<DragModel<S>>()
                .is_transitioning()
            {
                break;
            }
        }
    }

    /// Replaces the state and waits for the view to settle.
    pub fn show(&mut self, state: S) {
        self.app
            .world_mut()
            .resource_mut::<DragModel<S>>()
            .set_state(state);
        self.settle();
    }

    /// Resizes the window, as the user does, and waits for the HUD to lay itself out again.
    pub fn resize(&mut self, width: f32, height: f32) {
        let mut windows = self
            .app
            .world_mut()
            .query_filtered::<(Entity, &mut Window), With<PrimaryWindow>>();
        let (window, mut resolution) = windows
            .single_mut(self.app.world_mut())
            .expect("a primary window");
        resolution.resolution.set(width, height);
        self.app.world_mut().write_message(WindowResized {
            window,
            width,
            height,
        });
        for _ in 0..MAX_SETTLE {
            self.app.update();
        }
    }

    /// The state.
    pub fn state(&self) -> &S {
        self.app.world().resource::<DragModel<S>>().state()
    }

    /// Where on the window the HUD node `id` of the view is, in pixels from the top left.
    pub fn position_of(&mut self, id: &str) -> Vec2 {
        let world = self
            .app
            .world()
            .resource::<DragModel<S>>()
            .view()
            .world_position(id)
            .unwrap_or_else(|| panic!("the view has no node {id}"));
        let mut cameras = self
            .app
            .world_mut()
            .query_filtered::<(&Camera, &GlobalTransform), With<HudCamera>>();
        let (camera, transform) = cameras.single(self.app.world()).expect("one HUD camera");
        camera
            .world_to_viewport(transform, world.truncate().extend(0.0))
            .unwrap_or_else(|error| panic!("{id} is off screen: {error:?}"))
    }

    /// Where on the window the world point `world` is as the scene camera sees it.
    pub fn scene_point(&mut self, world: Vec3) -> Vec2 {
        let mut cameras = self
            .app
            .world_mut()
            .query_filtered::<(&Camera, &GlobalTransform), With<C>>();
        let (camera, transform) = cameras.single(self.app.world()).expect("one scene camera");
        camera
            .world_to_viewport(transform, world)
            .unwrap_or_else(|error| panic!("{world} is off screen: {error:?}"))
    }

    /// The point of the horizontal plane at `height` that the scene camera shows at the pixel
    /// `pixel` of the window.
    pub fn ground_point(&mut self, pixel: Vec2, height: f32) -> Vec3 {
        let mut cameras = self
            .app
            .world_mut()
            .query_filtered::<(&Camera, &GlobalTransform), With<C>>();
        let (camera, transform) = cameras.single(self.app.world()).expect("one scene camera");
        let ray = camera
            .viewport_to_world(transform, pixel)
            .unwrap_or_else(|error| panic!("{pixel} is not in the window: {error:?}"));
        let distance = ray
            .intersect_plane(Vec3::Y * height, InfinitePlane3d::new(Vec3::Y))
            .expect("the camera looks at the ground");
        ray.get_point(distance)
    }

    /// Where on the window the scene node `id` of the view is.
    pub fn scene_position_of(&mut self, id: &str) -> Vec2 {
        let world = self
            .app
            .world()
            .resource::<DragModel<S>>()
            .view()
            .world_position(id)
            .unwrap_or_else(|| panic!("the view has no node {id}"));
        self.scene_point(world)
    }

    fn send(&mut self, action: PointerAction) {
        let window = self
            .app
            .world_mut()
            .query_filtered::<Entity, With<PrimaryWindow>>()
            .single(self.app.world())
            .expect("a primary window");
        let target = RenderTarget::Window(WindowRef::Entity(window))
            .normalize(Some(window))
            .expect("the window is a render target");
        self.app.world_mut().write_message(PointerInput::new(
            PointerId::Mouse,
            Location {
                target,
                position: self.cursor,
            },
            action,
        ));
        self.app.update();
    }

    /// Moves the pointer to `to`, in pixels from the top left of the window.
    pub fn move_to(&mut self, to: Vec2) {
        let delta = to - self.cursor;
        self.cursor = to;
        self.send(PointerAction::Move { delta });
    }

    /// Presses the primary button.
    pub fn press(&mut self) {
        self.send(PointerAction::Press(PointerButton::Primary));
    }

    /// Releases the primary button.
    pub fn release(&mut self) {
        self.send(PointerAction::Release(PointerButton::Primary));
    }

    /// The names of the entities under the pointer, nearest to the camera first.
    pub fn hovered(&mut self) -> Vec<String> {
        let map = self.app.world().resource::<HoverMap>();
        let mut hits: Vec<(Entity, f32)> = map
            .get(&PointerId::Mouse)
            .map(|hits| {
                hits.iter()
                    .map(|(entity, hit)| (*entity, hit.depth))
                    .collect()
            })
            .unwrap_or_default();
        hits.sort_by(|a, b| a.1.total_cmp(&b.1));
        hits.into_iter()
            .map(|(entity, _)| {
                self.app
                    .world()
                    .get::<Name>(entity)
                    .map_or_else(|| format!("{entity}"), ToString::to_string)
            })
            .collect()
    }

    /// Moves onto the HUD node `id`, presses and releases without moving, and waits for the
    /// tweens.
    pub fn click(&mut self, id: &str) {
        let at = self.position_of(id);
        self.move_to(at);
        self.press();
        self.release();
        self.settle();
    }

    /// Like [`click`](Self::click), but the pointer moves a pixel while the button is down, as a
    /// hand on a mouse does.
    pub fn click_jittering(&mut self, id: &str) {
        let at = self.position_of(id);
        self.move_to(at);
        self.press();
        self.move_to(at + Vec2::X);
        self.release();
        self.settle();
    }

    /// Presses at `from`, drags through `steps` to the last of them in pixels from the top left
    /// of the window, releases and waits for the tweens.
    pub fn drag(&mut self, from: Vec2, steps: &[Vec2]) {
        self.move_to(from + Vec2::X);
        self.move_to(from);
        self.press();
        for step in steps {
            self.move_to(*step);
        }
        self.release();
        self.settle();
    }
}
