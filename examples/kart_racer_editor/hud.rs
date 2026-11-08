//! The heads-up display: a speed bar and two icons drawn as meshes in front of the camera, the
//! lap, coin and position texts, and the dial that counts down to being put back before a
//! missed checkpoint.
//!
//! Ported from the `bevy_gradmesh` kart racer example. The dial is a ring of `bevy_gradmesh`'s
//! 2D shapes, drawn by a camera of its own that draws nothing else.

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::camera::ScalingMode;
use bevy::camera::visibility::RenderLayers;
use bevy::mesh::Mesh2d;
use bevy::prelude::*;
use bevy::sprite_render::MeshMaterial2d;
use bevy_gradmesh::{
    GradMeshMaterial, SdfMaterial2d, Shape, ShapeDetail, dual_directional, dual_radial_alpha,
    opaque, unit_quad,
};

use crate::kart::{KartCamera, KartState, top_speed};
use crate::race::{LAPS, MISSED_COUNTDOWN, Race};
use crate::track::TrackDesign;

/// The width of the speed bar fill when the kart is at full speed.
const BAR_WIDTH: f32 = 3.9;
/// How far in front of the camera the HUD meshes sit.
const HUD_DEPTH: f32 = -6.5;
/// How far behind the fill of the speed bar its background sits.
const HUD_BACKDROP: f32 = -7.0;
/// The height of the speed bar on screen.
const BAR_HEIGHT: f32 = -2.0;
/// The height of the icons on screen.
const ICON_HEIGHT: f32 = 2.0;

/// The render layer of the countdown dial, which only its own camera draws.
const DIAL_LAYER: usize = 7;
/// How many world units the camera of the dial shows vertically.
const DIAL_VIEW: f32 = 720.0;
/// How far above the middle of the screen the dial sits.
const DIAL_HEIGHT: f32 = 200.0;
/// The radius of the dial.
const DIAL_RADIUS: f32 = 44.0;
/// The width of the ring of the dial.
const DIAL_RIM: f32 = 9.0;

/// Marks the top-level entities of the HUD, so it can be despawned as a whole.
#[derive(Component)]
pub struct HudPart;

/// The node the meshes of the HUD hang from; it follows the camera.
#[derive(Component)]
pub struct HudRoot;

/// The ring that shrinks while the countdown to being put back runs.
#[derive(Component)]
pub struct MissedDial;

/// The number of seconds left, in the middle of the [`MissedDial`].
#[derive(Component)]
pub struct MissedCount;

/// The number of the dial, which is not the ring.
type CountOnly = (With<MissedCount>, Without<MissedDial>);

/// The part of the speed bar that grows with the speed.
#[derive(Component)]
pub struct SpeedBarFill;

/// A text of the HUD and what it shows.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum HudLabel {
    /// The lap and the clock.
    Laps,
    /// The coins collected.
    Coins,
    /// The position of the kart.
    Position,
}

/// How much of the speed bar is filled at `speed` for a kart that has collected `coins`: the
/// bar is full at the top speed those coins allow.
pub fn speed_ratio(speed: f32, coins: u32) -> f32 {
    (speed.abs() / top_speed(coins)).clamp(0.0, 1.0)
}

/// The horizontal scale and the horizontal position of the speed bar fill at a fill `ratio`,
/// which keep its left end where the bar starts.
pub fn speed_bar_fill(ratio: f32) -> (f32, f32) {
    let scale = ratio.max(0.001);
    (scale, -2.0 + (BAR_WIDTH - 0.1) * scale * 0.5)
}

/// The ring of the countdown dial with `remaining` seconds left: the share of the ring that is
/// still there is the share of the countdown that is, and its color goes from green to red.
pub fn dial(remaining: f32) -> SdfMaterial2d {
    let share = (remaining / MISSED_COUNTDOWN).clamp(0.0, 1.0);
    SdfMaterial2d::disc(DIAL_RADIUS)
        .with_rim(DIAL_RIM)
        .with_arc(FRAC_PI_2, TAU * share)
        .with_color(Color::srgb(1.0, 0.25 + 0.65 * share, 0.2))
}

/// The number in the middle of the dial with `remaining` seconds left, which counts down to
/// one while the countdown runs.
pub fn dial_label(remaining: f32) -> String {
    format!("{:.0}", remaining.ceil().max(1.0))
}

/// The lap and the clock, or the end of the race.
pub fn lap_label(race: &Race) -> String {
    if race.finished() {
        return "Race Complete!".into();
    }
    let clock = if race.started {
        format!("{:.1}s", race.elapsed)
    } else {
        "Ready!".into()
    };
    format!("Lap {} / {LAPS}  {clock}", race.laps)
}

/// How many of the coins are collected.
pub fn coin_label(collected: u32, total: usize) -> String {
    format!("{collected}/{total}")
}

/// The position of the kart, and a warning while it is falling, or else while it is off the
/// road.
pub fn position_label(position: Vec3, off_road: bool, falling: bool) -> String {
    let warning = if falling {
        "  FALLING"
    } else if off_road {
        "  OFF ROAD"
    } else {
        ""
    };
    format!(
        "Pos: {:.1}, {:.1}, {:.1}{warning}",
        position.x, position.y, position.z
    )
}

fn label(text: String, size: f32, color: Color, node: Node, kind: HudLabel) -> impl Bundle {
    (
        Text::new(text),
        TextFont {
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(color),
        node,
        kind,
        HudPart,
    )
}

/// Spawns the speed bar and the icons in front of the camera, and the texts, all marked with
/// [`HudPart`].
pub fn spawn_hud(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<GradMeshMaterial>,
    dials: &mut Assets<SdfMaterial2d>,
) {
    let root = commands
        .spawn((
            Transform::default(),
            Visibility::default(),
            HudRoot,
            HudPart,
        ))
        .id();
    let flat = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);

    commands.spawn((
        Mesh3d(meshes.add(Shape::rectangle(Vec2::new(4.0, 0.35), ShapeDetail::Low).build())),
        MeshMaterial3d(materials.add(opaque(0.05, 0.05, 0.15))),
        Transform::from_xyz(0.0, BAR_HEIGHT, HUD_BACKDROP).with_rotation(flat),
        ChildOf(root),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Shape::rectangle(Vec2::new(BAR_WIDTH, 0.25), ShapeDetail::Low).build())),
        MeshMaterial3d(materials.add(dual_directional(
            [0.0, 0.5, 1.0],
            [0.0, 1.0, 0.5],
            [1.0, 0.0, 0.0],
        ))),
        Transform::from_xyz(-1.95, BAR_HEIGHT, HUD_DEPTH)
            .with_rotation(flat)
            .with_scale(Vec3::new(0.001, 1.0, 1.0)),
        ChildOf(root),
        SpeedBarFill,
    ));
    commands.spawn((
        Mesh3d(meshes.add(Shape::star(5, 0.35, 0.14, ShapeDetail::Medium).build())),
        MeshMaterial3d(materials.add(dual_radial_alpha(
            [1.0, 0.85, 0.2, 1.0],
            [0.8, 0.5, 0.1, 1.0],
        ))),
        Transform::from_xyz(2.3, ICON_HEIGHT, HUD_DEPTH).with_rotation(flat),
        ChildOf(root),
    ));
    commands.spawn((
        Mesh3d(meshes.add(Shape::polygon(6, 0.3, ShapeDetail::Low).build())),
        MeshMaterial3d(materials.add(dual_directional(
            [0.1, 0.4, 0.8],
            [0.3, 0.8, 1.0],
            [0.0, 1.0, 0.0],
        ))),
        Transform::from_xyz(-3.0, ICON_HEIGHT, HUD_DEPTH).with_rotation(flat),
        ChildOf(root),
    ));

    let layer = RenderLayers::layer(DIAL_LAYER);
    commands.spawn((
        Camera2d,
        layer.clone(),
        Camera {
            order: 2,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: ScalingMode::FixedVertical {
                viewport_height: DIAL_VIEW,
            },
            ..OrthographicProjection::default_2d()
        }),
        HudPart,
    ));
    let ring = dial(MISSED_COUNTDOWN);
    commands.spawn((
        Mesh2d(meshes.add(unit_quad())),
        Transform::from_xyz(0.0, DIAL_HEIGHT, 0.0).with_scale(ring.quad_scale()),
        MeshMaterial2d(dials.add(ring)),
        layer.clone(),
        Visibility::Hidden,
        Pickable::IGNORE,
        MissedDial,
        HudPart,
    ));
    commands.spawn((
        Text2d::new(dial_label(MISSED_COUNTDOWN)),
        TextFont {
            font_size: FontSize::Px(48.0),
            ..default()
        },
        TextColor(Color::WHITE),
        Transform::from_xyz(0.0, DIAL_HEIGHT, 1.0),
        layer,
        Visibility::Hidden,
        Pickable::IGNORE,
        MissedCount,
        HudPart,
    ));

    let at = |left: Val, top: Val| Node {
        position_type: PositionType::Absolute,
        left,
        top,
        ..default()
    };
    commands.spawn(label(
        lap_label(&Race::default()),
        28.0,
        Color::srgb(0.3, 0.8, 1.0),
        at(Val::Percent(21.0), Val::Percent(8.0)),
        HudLabel::Laps,
    ));
    commands.spawn(label(
        coin_label(0, 0),
        28.0,
        Color::srgb(1.0, 0.85, 0.2),
        at(Val::Percent(77.0), Val::Percent(8.0)),
        HudLabel::Coins,
    ));
    commands.spawn(label(
        position_label(Vec3::ZERO, false, false),
        18.0,
        Color::srgba(0.6, 0.6, 0.7, 0.9),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(16.0),
            bottom: Val::Px(40.0),
            ..default()
        },
        HudLabel::Position,
    ));
    commands.spawn((
        Text::new("W/Up Accelerate  S/Down Brake  A/D Steer  R Reset"),
        TextFont {
            font_size: FontSize::Px(18.0),
            ..default()
        },
        TextColor(Color::srgba(1.0, 1.0, 1.0, 0.5)),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(16.0),
            bottom: Val::Px(12.0),
            ..default()
        },
        HudPart,
    ));
}

/// Keeps the HUD meshes at the camera.
pub fn follow_hud_camera(
    camera: Query<&Transform, (With<KartCamera>, Without<HudRoot>)>,
    mut root: Query<&mut Transform, With<HudRoot>>,
) {
    if let (Ok(camera), Ok(mut root)) = (camera.single(), root.single_mut()) {
        root.translation = camera.translation;
        root.rotation = camera.rotation;
    }
}

/// Fills the speed bar to the speed of the kart.
pub fn update_speed_bar(state: Res<KartState>, mut bar: Query<&mut Transform, With<SpeedBarFill>>) {
    if let Ok(mut transform) = bar.single_mut() {
        let (scale, x) = speed_bar_fill(speed_ratio(state.speed, state.coins));
        transform.scale.x = scale;
        transform.translation.x = x;
    }
}

/// Shows the dial while the countdown to being put back runs, and hides it otherwise.
pub fn update_missed_dial(
    race: Res<Race>,
    mut materials: ResMut<Assets<SdfMaterial2d>>,
    mut rings: Query<(&MeshMaterial2d<SdfMaterial2d>, &mut Visibility), With<MissedDial>>,
    mut counts: Query<(&mut Text2d, &mut Visibility), CountOnly>,
) {
    let shown = |remaining: Option<f32>| {
        if remaining.is_some() {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        }
    };
    for (handle, mut visibility) in &mut rings {
        visibility.set_if_neq(shown(race.countdown));
        if let (Some(remaining), Some(mut material)) =
            (race.countdown, materials.get_mut(&handle.0))
        {
            *material = dial(remaining);
        }
    }
    for (mut text, mut visibility) in &mut counts {
        visibility.set_if_neq(shown(race.countdown));
        if let Some(remaining) = race.countdown {
            text.0 = dial_label(remaining);
        }
    }
}

/// Writes the lap, the coins and the position into the texts.
pub fn update_hud_text(
    state: Res<KartState>,
    race: Res<Race>,
    design: Res<TrackDesign>,
    mut texts: Query<(&mut Text, &HudLabel)>,
) {
    for (mut text, kind) in &mut texts {
        text.0 = match kind {
            HudLabel::Laps => lap_label(&race),
            HudLabel::Coins => coin_label(state.coins, design.coin_count),
            HudLabel::Position => position_label(state.position, state.off_road(), state.falling()),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kart::MAX_SPEED;

    #[test]
    fn the_speed_bar_fills_with_the_speed() {
        assert_eq!(speed_ratio(0.0, 0), 0.0);
        assert_eq!(speed_ratio(MAX_SPEED / 2.0, 0), 0.5);
        assert_eq!(speed_ratio(-MAX_SPEED / 4.0, 0), 0.25);
        assert_eq!(speed_ratio(MAX_SPEED * 3.0, 0), 1.0);
        assert!(speed_ratio(MAX_SPEED, 5) < 1.0);
    }

    #[test]
    fn the_speed_bar_fill_keeps_its_left_end_in_place() {
        for ratio in [0.0, 0.3, 1.0] {
            let (scale, x) = speed_bar_fill(ratio);
            let left = x - (BAR_WIDTH - 0.1) * scale / 2.0;
            assert!((left + 2.0).abs() < 1e-5);
        }
        assert_eq!(speed_bar_fill(0.0).0, 0.001);
    }

    #[test]
    fn the_lap_label_shows_the_clock_and_the_end_of_the_race() {
        let mut race = Race::default();
        assert_eq!(lap_label(&race), "Lap 0 / 3  Ready!");
        race.started = true;
        race.elapsed = 12.34;
        race.laps = 1;
        assert_eq!(lap_label(&race), "Lap 1 / 3  12.3s");
        race.laps = LAPS;
        assert_eq!(lap_label(&race), "Race Complete!");
    }

    #[test]
    fn coins_and_position_are_formatted() {
        assert_eq!(coin_label(3, 24), "3/24");
        let position = Vec3::new(1.0, 2.25, -3.0);
        assert_eq!(
            position_label(position, false, false),
            "Pos: 1.0, 2.2, -3.0"
        );
        assert_eq!(
            position_label(position, true, false),
            "Pos: 1.0, 2.2, -3.0  OFF ROAD"
        );
        assert_eq!(
            position_label(position, true, true),
            "Pos: 1.0, 2.2, -3.0  FALLING"
        );
    }

    #[test]
    fn the_dial_keeps_the_share_of_the_countdown_that_is_left_and_turns_red() {
        let arc = |remaining: f32| dial(remaining).uniform.shape.w;
        assert!((arc(MISSED_COUNTDOWN) - TAU).abs() < 1e-4);
        assert!((arc(MISSED_COUNTDOWN / 2.0) - TAU / 2.0).abs() < 1e-4);
        assert!(arc(0.0).abs() < 1e-4);
        assert!(
            (arc(MISSED_COUNTDOWN * 3.0) - TAU).abs() < 1e-4,
            "never more than a ring"
        );
        let green = |remaining: f32| dial(remaining).uniform.color.y;
        assert!(green(MISSED_COUNTDOWN) > green(MISSED_COUNTDOWN / 2.0));
        assert!(green(MISSED_COUNTDOWN / 2.0) > green(0.0));
    }

    #[test]
    fn the_dial_counts_down_to_one() {
        assert_eq!(dial_label(MISSED_COUNTDOWN), "5");
        assert_eq!(dial_label(4.2), "5");
        assert_eq!(dial_label(4.0), "4");
        assert_eq!(dial_label(1.0), "1");
        assert_eq!(dial_label(0.3), "1");
    }
}
