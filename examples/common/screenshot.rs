//! Automatic screenshot capture utility for headless rendering.
//!
//! When the `DSD_SCREENSHOT` environment variable is set, this module configures
//! the render plugin for synchronous shader compilation and adds a system that
//! waits for the scene to fully render before capturing a screenshot and exiting.
//! This is used by the `make screenshots` target to generate README images without
//! manual intervention.
//!
//! # Timing
//!
//! The Bevy screenshot pipeline spans multiple frames:
//! 1. Spawn `Screenshot` entity in Update
//! 2. Next ExtractSchedule: entity is extracted to the render world
//! 3. Next Render: screenshot is rendered to a texture and copied to a GPU buffer
//! 4. Async GPU readback completes
//! 5. Next Update: `ScreenshotCaptured` event fires
//!
//! This plugin detects when the first frame has actually rendered by observing
//! `FrameCount` (which increments during the `Last` schedule after rendering).
//! It then spawns the screenshot entity and waits for the `ScreenshotCaptured`
//! observer before exiting — no frame counters or sleep needed.
//!
//! Configure `RenderPlugin` with `synchronous_pipeline_compilation: true` so all
//! shaders compile before the first frame. Without this, the first frame may render
//! black while shaders are still compiling asynchronously.
//!
//! ```rust,no_run
//! use bevy::prelude::*;
//! use bevy::render::RenderPlugin;
//!
//! fn setup() {}
//!
//! fn main() {
//!     App::new()
//!         .add_plugins(DefaultPlugins.set(RenderPlugin {
//!             synchronous_pipeline_compilation: true,
//!             ..default()
//!         }))
//!         .add_plugins(screenshot::AutoScreenshotPlugin::new("shapes.png"))
//!         .add_systems(Startup, setup)
//!         .run();
//! }
//! ```
//!
//! The screenshot will only be taken when `DSD_SCREENSHOT=1` is set in the environment.

use std::path::PathBuf;
use std::time::Duration;

use bevy::diagnostic::FrameCount;
use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use bevy::time::TimeUpdateStrategy;

use super::script::DragTimeline;

/// Plugin that adds automatic screenshot capture when `DSD_SCREENSHOT` is set.
///
/// When enabled, the app will wait for the first frame to render, capture a
/// screenshot of the primary window, save it to the given filename, and then exit.
pub struct AutoScreenshotPlugin {
    filename: String,
    title: Option<String>,
    description: Option<String>,
    /// Minimum frame count to wait before taking the screenshot (default 1).
    /// Complex 3D scenes may need more frames for shaders to compile.
    wait_frames: u32,
    /// If true, capture multiple frames for animation instead of a single screenshot.
    /// The filename is used as a base: frames are saved as `{base}_frame_0000.png`, etc.
    /// The number of frames to capture is controlled by `animation_frames`.
    animation: bool,
    /// Maximum number of frames to capture for animation.
    animation_frames: u32,
    /// If set, capture a second screenshot after the first is saved, using this filename.
    second_shot: Option<String>,
}

impl AutoScreenshotPlugin {
    /// Create a new screenshot plugin that saves to the given filename.
    ///
    /// The screenshot is only taken when the `DSD_SCREENSHOT` environment variable
    /// is set to a non-empty value (e.g., `DSD_SCREENSHOT=1`).
    pub fn new(filename: impl Into<String>) -> Self {
        Self {
            filename: filename.into(),
            title: None,
            description: None,
            wait_frames: 1,
            animation: false,
            animation_frames: 600,
            second_shot: None,
        }
    }

    /// Set a second screenshot captured after the first is saved.
    ///
    /// The caller is responsible for changing the state (e.g., opening a radial menu)
    /// between the two shots. The second shot uses the same wait-frames logic so the
    /// view has time to settle after the state change.
    pub fn with_second_shot(mut self, filename: impl Into<String>) -> Self {
        self.second_shot = Some(filename.into());
        self
    }

    /// Set the title displayed in the screenshot overlay banner.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Set the description displayed in the screenshot overlay banner.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set the minimum frame count to wait before taking the screenshot.
    ///
    /// Complex 3D scenes with many materials may need more frames for all
    /// shaders to compile and render correctly. Default is 1.
    pub fn wait_frames(mut self, frames: u32) -> Self {
        self.wait_frames = frames.max(1);
        self
    }

    /// Enable multi-frame animation capture mode, capped at `frames` frames.
    ///
    /// Instead of capturing a single screenshot, this captures one PNG frame per app tick
    /// while the scripted drag runs. Each tick advances the clock by exactly `1 / ANIMATION_FPS`
    /// seconds, so the frames encode to an animated WebP that plays back at real speed.
    pub fn animation(mut self, frames: u32) -> Self {
        self.animation = true;
        self.animation_frames = frames.max(2);
        self
    }
}

impl Plugin for AutoScreenshotPlugin {
    fn build(&self, app: &mut App) {
        // Only enable when DSD_SCREENSHOT is set
        if std::env::var("DSD_SCREENSHOT").is_err() {
            return;
        }

        app.insert_resource(AutoScreenshotFilename(self.filename.clone()))
            .insert_resource(ScreenshotState::default())
            .insert_resource(WaitFrames(self.wait_frames))
            .insert_resource(AnimationConfig {
                total_frames: self.animation_frames,
                captured: 0,
                saved: 0,
            });

        // Add title overlay if configured
        if self.title.is_some() || self.description.is_some() {
            app.insert_resource(ScreenshotTitle {
                title: self.title.clone(),
                description: self.description.clone(),
            })
            .add_systems(Startup, spawn_title_overlay);
        }

        if self.animation {
            // Saving a screenshot stalls the app, so the clock is stepped by hand: one tick is
            // always one frame of the animation, whatever the renderer's speed.
            app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
                1.0 / ANIMATION_FPS,
            )))
            .add_systems(
                Update,
                (wait_for_animation_frames, quit_after_animation_saved).chain(),
            );
        } else if self.second_shot.is_some() {
            app.insert_resource(SecondShotFilename(self.second_shot.clone().unwrap()))
                .add_systems(
                    Update,
                    (
                        wait_for_first_frame,
                        capture_second_shot,
                        quit_after_second_shot_saved,
                    )
                        .chain(),
                );
        } else {
            app.add_systems(
                Update,
                (wait_for_first_frame, quit_after_screenshot_saved).chain(),
            );
        }
    }
}

/// Title and description for the screenshot overlay banner.
#[derive(Resource)]
struct ScreenshotTitle {
    title: Option<String>,
    description: Option<String>,
}

/// Component marking the title overlay entity so it can be removed after screenshot.
#[derive(Component)]
struct TitleOverlay;

/// Spawn a semi-transparent banner at the top of the window showing the title and description.
fn spawn_title_overlay(
    mut commands: Commands,
    title: Res<ScreenshotTitle>,
    window: Query<&Window, With<Window>>,
) {
    let window = window.single().expect("no window found");
    let width = window.resolution.width() as f32;
    let height = window.resolution.height() as f32;

    // Title text using Text2d
    if let Some(ref t) = title.title {
        commands.spawn((
            Text2d(t.clone()),
            TextColor(Color::srgb(1.0, 1.0, 1.0)),
            TextFont {
                font_size: FontSize::Px(18.0),
                ..default()
            },
            Transform::from_xyz(width / 2.0 - 100.0, height / 2.0 - 30.0, 1000.0),
            bevy::camera::visibility::RenderLayers::layer(super::OVERLAY_LAYER),
            TitleOverlay,
            Name::new("Title Overlay Text"),
        ));
    }
    // Description text using Text2d
    if let Some(ref d) = title.description {
        commands.spawn((
            Text2d(d.clone()),
            TextColor(Color::srgba(1.0, 1.0, 1.0, 0.7)),
            TextFont {
                font_size: FontSize::Px(14.0),
                ..default()
            },
            Transform::from_xyz(width / 2.0 - 100.0, height / 2.0 - 6.0, 1000.0),
            bevy::camera::visibility::RenderLayers::layer(super::OVERLAY_LAYER),
            TitleOverlay,
            Name::new("Title Overlay Description"),
        ));
    }
}

/// Target filename stored as a resource so the observer can access it.
#[derive(Resource)]
struct AutoScreenshotFilename(String);

/// Minimum frame count to wait before taking the screenshot.
#[derive(Resource)]
struct WaitFrames(u32);

/// Tracks the screenshot lifecycle: pending → triggered → saved → quit.
///
/// Exposed so examples can react to the first screenshot being saved, for example
/// to open a radial menu before the second shot is captured.
#[derive(Resource, Default)]
pub struct ScreenshotState {
    /// `true` after we spawned the `Screenshot` entity.
    pub triggered: bool,
    /// `true` after the `ScreenshotCaptured` observer ran and the file is saved.
    pub saved: bool,
    /// `true` after the second shot has been triggered.
    pub second_triggered: bool,
    /// `true` after the second shot is saved.
    pub second_saved: bool,
    /// Set by the example after it completes the state change (e.g., opens a menu).
    /// The second shot fires only after this flag is `true`, establishing a happens-before
    /// relationship: the first screenshot is saved → the example changes state and signals
    /// readiness → the second screenshot is captured.
    pub ready_for_second_shot: bool,
    /// The frame count when `ready_for_second_shot` was set. The second shot waits at least one
    /// frame after this so the changed state (e.g., an open menu) is actually rendered.
    pub ready_frame: u32,
}

/// Filename of the second screenshot shot.
#[derive(Resource)]
struct SecondShotFilename(String);

/// Frame rate of animation captures, in frames per second of simulated time.
const ANIMATION_FPS: f32 = 30.0;

/// Seconds of simulated time to keep capturing after the drag is released.
const ANIMATION_HOLD_SECONDS: f32 = 0.5;

/// Animation capture state. `total_frames` is a safety cap; the drag timeline normally ends first.
#[derive(Resource)]
struct AnimationConfig {
    total_frames: u32,
    captured: u32,
    saved: u32,
}

/// Wait for the first rendered frame, then spawn the screenshot entity.
fn wait_for_first_frame(
    mut state: ResMut<ScreenshotState>,
    filename: Res<AutoScreenshotFilename>,
    mut commands: Commands,
    frame_count: Res<FrameCount>,
    window_query: Query<Entity, With<Window>>,
    wait_frames: Res<WaitFrames>,
) {
    if state.triggered {
        return;
    }

    if window_query.is_empty() {
        return;
    }

    if frame_count.0 >= wait_frames.0 {
        state.triggered = true;
        info!(
            "Taking screenshot after {} frames rendered (frame {}) -> {}",
            wait_frames.0, frame_count.0, filename.0
        );
        let path = filename.0.clone();
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path.clone()))
            .observe(move |_trigger: On<ScreenshotCaptured>| {
                info!("Screenshot saved to {}", path);
            })
            .observe(mark_screenshot_done);
    }
}

/// Observer on the screenshot entity: marks the screenshot as saved.
fn mark_screenshot_done(_trigger: On<ScreenshotCaptured>, mut state: ResMut<ScreenshotState>) {
    state.saved = true;
    info!("Screenshot capture complete");
}

/// After the screenshot file is written, exit the app.
fn quit_after_screenshot_saved(
    state: Res<ScreenshotState>,
    mut app_exit: bevy::ecs::message::MessageWriter<AppExit>,
) {
    if state.saved {
        info!("Exiting after screenshot");
        app_exit.write(AppExit::Success);
    }
}

/// After the first screenshot is saved and the example signals readiness, capture the second shot.
///
/// The example changes state (e.g., opens a radial menu) when `saved` becomes `true`, then sets
/// `ready_for_second_shot`. This system fires only after both conditions are met and at least one
/// frame has passed since readiness was signaled, establishing a happens-before relationship:
/// first screenshot saved → state changed → ready signaled → one frame rendered → second screenshot.
fn capture_second_shot(
    mut state: ResMut<ScreenshotState>,
    filename: Res<SecondShotFilename>,
    mut commands: Commands,
    frame_count: Res<FrameCount>,
    window_query: Query<Entity, With<Window>>,
) {
    if state.second_triggered {
        return;
    }
    // Wait until the first screenshot is saved AND the example has signaled readiness.
    if !state.saved || !state.ready_for_second_shot {
        return;
    }
    // Wait at least one frame after readiness so the changed state is rendered.
    if frame_count.0 <= state.ready_frame {
        return;
    }
    if window_query.is_empty() {
        return;
    }
    state.second_triggered = true;
    info!("Taking second screenshot -> {}", filename.0);
    let path = filename.0.clone();
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(path.clone()))
        .observe(move |_trigger: On<ScreenshotCaptured>| {
            info!("Second screenshot saved to {}", path);
        })
        .observe(mark_second_shot_done);
}

/// Observer on the second screenshot entity: marks it as saved.
fn mark_second_shot_done(_trigger: On<ScreenshotCaptured>, mut state: ResMut<ScreenshotState>) {
    state.second_saved = true;
    info!("Second screenshot capture complete");
}

/// Exit after both screenshots are saved.
fn quit_after_second_shot_saved(
    state: Res<ScreenshotState>,
    mut app_exit: bevy::ecs::message::MessageWriter<AppExit>,
) {
    if state.second_saved {
        info!("Exiting after second screenshot");
        app_exit.write(AppExit::Success);
    }
}

/// Captures one frame per app tick while the scripted drag runs.
///
/// The clock advances `1 / ANIMATION_FPS` seconds per tick, so the saved frames play back at
/// `ANIMATION_FPS` in real time however fast or slow the renderer is. Captures are not gated on
/// the previous one finishing: a screenshot takes several frames to read back from the GPU,
/// and each frame has its own file, so completion order does not matter.
fn wait_for_animation_frames(
    mut config: ResMut<AnimationConfig>,
    filename: Res<AutoScreenshotFilename>,
    mut commands: Commands,
    time: Res<Time<Real>>,
    timeline: Option<Res<DragTimeline>>,
    window_query: Query<Entity, With<Window>>,
) {
    if window_query.is_empty() || config.captured >= config.total_frames {
        return;
    }

    // Capture starts with the drag and stops shortly after it is released.
    let Some(timeline) = timeline else {
        return;
    };
    let now = time.elapsed_secs();
    if now > timeline.end + ANIMATION_HOLD_SECONDS {
        return;
    }

    let frame_idx = config.captured;
    config.captured += 1;

    let frame_path = animation_frame_path(&filename.0, frame_idx);
    info!(
        "Capturing animation frame {}/{} -> {}",
        frame_idx + 1,
        config.total_frames,
        frame_path.display()
    );

    let path_str = frame_path.display().to_string();
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(path_str.clone()))
        .observe(move |_trigger: On<ScreenshotCaptured>| {
            info!("Animation frame saved: {}", path_str);
        })
        .observe(mark_animation_frame_saved);
}

/// Counts an animation frame as saved once its file has been written.
fn mark_animation_frame_saved(
    _trigger: On<ScreenshotCaptured>,
    mut config: ResMut<AnimationConfig>,
) {
    config.saved += 1;
}

/// Path of the PNG for animation frame `index`, in a `frames/` subdirectory next to the base
/// filename. The directory is gitignored so intermediate frames never pollute the repository.
fn animation_frame_path(base: &str, index: u32) -> PathBuf {
    let path = PathBuf::from(base);
    let stem = path
        .file_stem()
        .unwrap_or_else(|| std::ffi::OsStr::new("frame"));
    let dir = path.parent().unwrap_or(".".as_ref());
    dir.join("frames")
        .join(format!("{}_frame_{:04}.png", stem.to_string_lossy(), index))
}

/// Exit once capturing has stopped and every captured frame has been written to disk.
fn quit_after_animation_saved(
    config: Res<AnimationConfig>,
    time: Res<Time<Real>>,
    timeline: Option<Res<DragTimeline>>,
    mut app_exit: bevy::ecs::message::MessageWriter<AppExit>,
) {
    let Some(timeline) = timeline else {
        return;
    };
    let capture_over = time.elapsed_secs() > timeline.end + ANIMATION_HOLD_SECONDS
        || config.captured >= config.total_frames;
    if capture_over && config.saved >= config.captured {
        info!("All {} animation frames saved. Exiting.", config.captured);
        app_exit.write(AppExit::Success);
    }
}
