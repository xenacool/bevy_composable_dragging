//! A scheduler: drag meetings between lanes and in time, and resize them from their edges.
//!
//! A meeting body combines two operators: `closest` snaps it to the nearest lane, and inside each
//! lane `vary` slides its start and end together within the day. The edge handles only vary one
//! time and keep it in order with the other.

use bevy::prelude::*;
use bevy_composable_dragging::constraint::{Constraint, and, at_least, at_most, in_order};
use bevy_composable_dragging::ops::{closest, vary_constrained};
use bevy_composable_dragging::{DragSpec, DraggablePlugin, View, ViewNode, param};

mod common;
use common::screenshot::AutoScreenshotPlugin;
use common::script::ScriptedDragPlugin;

const LANES: usize = 3;
const DAY: f32 = 10.0;
const HOUR_WIDTH: f32 = 0.8;
const LANE_SPACING: f32 = 1.3;
const HANDLE_WIDTH: f32 = 0.16;

#[derive(Clone)]
struct Meeting {
    lane: usize,
    start: f32,
    end: f32,
}

#[derive(Clone)]
struct Schedule {
    meetings: Vec<Meeting>,
}

fn hour_x(hour: f32) -> f32 {
    (hour - DAY / 2.0) * HOUR_WIDTH
}

fn lane_y(lane: usize) -> f32 {
    ((LANES - 1) as f32 / 2.0 - lane as f32) * LANE_SPACING
}

/// Slides the meeting along its lane, keeping its length and the day bounds.
fn slide(schedule: &Schedule, index: usize, lane: usize) -> DragSpec<Schedule> {
    let mut moved = schedule.clone();
    moved.meetings[index].lane = lane;
    let length = schedule.meetings[index].end - schedule.meetings[index].start;
    let keep_length = Constraint::new(move |state: &Schedule| {
        let meeting = &state.meetings[index];
        (meeting.end - meeting.start - length).abs()
    });
    let within_day = and([
        at_least(param!(Schedule, meetings[index].start), 0.0),
        at_most(param!(Schedule, meetings[index].end), DAY),
    ]);
    vary_constrained(
        moved,
        [
            param!(Schedule, meetings[index].start),
            param!(Schedule, meetings[index].end),
        ],
        and([keep_length, within_day]),
    )
}

/// Resizes one edge of the meeting, keeping the start before the end and both inside the day.
fn resize(schedule: &Schedule, index: usize, edge: Edge) -> DragSpec<Schedule> {
    let start = param!(Schedule, meetings[index].start);
    let end = param!(Schedule, meetings[index].end);
    let constraint = and([
        in_order([start.clone(), end.clone()]),
        at_least(start.clone(), 0.0),
        at_most(end.clone(), DAY),
    ]);
    let moving = match edge {
        Edge::Start => start,
        Edge::End => end,
    };
    vary_constrained(schedule.clone(), [moving], constraint)
}

#[derive(Clone, Copy)]
enum Edge {
    Start,
    End,
}

fn view(schedule: &Schedule) -> View<Schedule> {
    let mut view = View::new();
    for lane in 0..LANES {
        view.push(
            ViewNode::rectangle(
                format!("lane-{lane}"),
                Vec2::new(DAY * HOUR_WIDTH + 0.4, 1.1),
            )
            .at_xy(Vec2::new(0.0, lane_y(lane)))
            .with_color(Color::srgb(0.2, 0.22, 0.27)),
        );
    }
    for (index, meeting) in schedule.meetings.iter().enumerate() {
        let hue = index as f32 * 360.0 / schedule.meetings.len() as f32;
        let color = Color::hsl(hue, 0.6, 0.55);
        let width = (meeting.end - meeting.start) * HOUR_WIDTH;
        let center = Vec2::new(
            hour_x(f32::midpoint(meeting.start, meeting.end)),
            lane_y(meeting.lane),
        );
        let body = schedule.clone();
        view.push(
            ViewNode::rectangle(format!("meeting-{index}"), Vec2::new(width, 0.9))
                .at_xy(center)
                .with_z(0.1)
                .with_color(color)
                .draggable(move || closest((0..LANES).map(|lane| slide(&body, index, lane)))),
        );
        for (name, edge, hour) in [
            ("start", Edge::Start, meeting.start),
            ("end", Edge::End, meeting.end),
        ] {
            let handle = schedule.clone();
            view.push(
                ViewNode::rectangle(
                    format!("meeting-{index}-{name}"),
                    Vec2::new(HANDLE_WIDTH, 0.9),
                )
                .at_xy(Vec2::new(hour_x(hour), lane_y(meeting.lane)))
                .with_z(0.2)
                .with_color(Color::srgb(0.95, 0.95, 0.98))
                .draggable(move || resize(&handle, index, edge)),
            );
        }
    }
    view
}


/// Run this example. Called by the WASM runner.
pub fn run(app: &mut App) {
    let schedule = Schedule {
        meetings: vec![
            Meeting {
                lane: 0,
                start: 1.0,
                end: 3.0,
            },
            Meeting {
                lane: 0,
                start: 5.0,
                end: 8.0,
            },
            Meeting {
                lane: 1,
                start: 2.0,
                end: 4.5,
            },
            Meeting {
                lane: 2,
                start: 0.5,
                end: 2.5,
            },
            Meeting {
                lane: 2,
                start: 6.0,
                end: 9.0,
            },
        ],
    };
    app
        .add_plugins(common::default_plugins("scheduler"))
        .add_plugins(DraggablePlugin::new(schedule, view))
        .add_plugins(ScriptedDragPlugin::straight(
            Vec2::new(hour_x(3.25), lane_y(1)),
            Vec2::new(hour_x(4.3), lane_y(2) + 0.15),
        ))
        .add_plugins(
            AutoScreenshotPlugin::new("screenshot_scheduler.webp")
                .with_title("scheduler")
                .with_description("closest + vary")
                .animation(600),
        )
        .add_systems(Startup, |mut commands: Commands| {
            common::spawn_cameras(&mut commands, 6.0);
        })
        ;
}

fn main() {
    let schedule = Schedule {
        meetings: vec![
            Meeting {
                lane: 0,
                start: 1.0,
                end: 3.0,
            },
            Meeting {
                lane: 0,
                start: 5.0,
                end: 8.0,
            },
            Meeting {
                lane: 1,
                start: 2.0,
                end: 4.5,
            },
            Meeting {
                lane: 2,
                start: 0.5,
                end: 2.5,
            },
            Meeting {
                lane: 2,
                start: 6.0,
                end: 9.0,
            },
        ],
    };
    App::new()
        .add_plugins(common::default_plugins("scheduler"))
        .add_plugins(DraggablePlugin::new(schedule, view))
        .add_plugins(ScriptedDragPlugin::straight(
            Vec2::new(hour_x(3.25), lane_y(1)),
            Vec2::new(hour_x(4.3), lane_y(2) + 0.15),
        ))
        .add_plugins(
            AutoScreenshotPlugin::new("screenshot_scheduler.webp")
                .with_title("scheduler")
                .with_description("closest + vary")
                .animation(600),
        )
        .add_systems(Startup, |mut commands: Commands| {
            common::spawn_cameras(&mut commands, 6.0);
        })
        .run();
}
