//! A headless, deterministic physics app: avian on a fixed timestep, no window and no GPU.
//!
//! Avian only reads meshes, so the colliders of gradmesh shapes can be stepped and measured in a
//! plain Bevy app. Every call to [`Lab::step`] advances exactly one fixed step.

use std::collections::HashMap;
use std::time::Duration;

use avian3d::dynamics::solver::islands::WakeBody;
use avian3d::prelude::*;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;

use crate::mesh_check::vertices;
use crate::scenario::ColliderKind;

/// How the world is stepped.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rig {
    /// The length of a fixed step in seconds.
    pub step: f32,
    /// How many solver substeps each step is divided into.
    pub substeps: u32,
    /// The strength of gravity, pulling down.
    pub gravity: f32,
}

impl Default for Rig {
    fn default() -> Self {
        Self {
            step: 1.0 / 60.0,
            substeps: 6,
            gravity: 9.81,
        }
    }
}

/// A body to spawn.
#[derive(Clone)]
pub struct Body {
    pub collider: Collider,
    pub position: Vec3,
    pub rotation: Quat,
    pub velocity: Vec3,
    pub restitution: f32,
    pub friction: f32,
    pub density: f32,
    pub kind: RigidBody,
    pub swept: bool,
    /// The corners of the mesh the collider was built from, in the body's own space.
    pub points: Vec<Vec3>,
}

impl Body {
    /// A body of `mesh` with a collider of `kind`, or `None` when avian cannot build one.
    pub fn from_mesh(mesh: &Mesh, kind: ColliderKind, position: Vec3) -> Option<Self> {
        let mut body = Self::at(kind.build(mesh)?, position);
        body.points = vertices(mesh);
        Some(body)
    }

    /// A dynamic body at `position` with the usual material.
    pub fn at(collider: Collider, position: Vec3) -> Self {
        Self {
            collider,
            position,
            rotation: Quat::IDENTITY,
            velocity: Vec3::ZERO,
            restitution: 0.0,
            friction: 0.5,
            density: 1.0,
            kind: RigidBody::Dynamic,
            swept: false,
            points: Vec::new(),
        }
    }
}

/// What a body is doing right now.
#[derive(Clone, Copy, Debug)]
pub struct Reading {
    pub position: Vec3,
    /// The centre of mass in the world, which is the point that locked axes hold, and which
    /// differs from `position` when the mass is not centred on the origin of the body.
    pub center: Vec3,
    pub velocity: Vec3,
    /// The lowest corner of the mesh in the world, or the bottom of the box around the collider
    /// when the body has no mesh. Unlike the box of a trimesh, which is the rotated box of its
    /// local space and so grows as the body turns, it does not depend on the collider kind.
    pub lowest: f32,
    pub mass: f32,
    pub asleep: bool,
}

impl Reading {
    /// Kinetic plus potential energy against the ground plane at height zero, moving linearly.
    pub fn energy(&self, gravity: f32) -> f32 {
        self.mass * (gravity * self.position.y + 0.5 * self.velocity.length_squared())
    }
}

/// The app that is stepped.
pub struct Lab {
    app: App,
    points: HashMap<Entity, Vec<Vec3>>,
}

impl Lab {
    /// A world stepped by `rig`, with a static ground whose top is at height zero.
    pub fn new(rig: Rig) -> Self {
        let step = Duration::from_secs_f32(rig.step);
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            AssetPlugin::default(),
            TransformPlugin,
            PhysicsPlugins::default(),
        ))
        .init_asset::<Mesh>()
        .insert_resource(Time::<Fixed>::from_duration(step))
        .insert_resource(TimeUpdateStrategy::ManualDuration(step))
        .insert_resource(SubstepCount(rig.substeps))
        .insert_resource(Gravity(Vec3::NEG_Y * rig.gravity));
        app.finish();
        app.cleanup();
        let mut lab = Self {
            app,
            points: HashMap::new(),
        };
        lab.app.world_mut().spawn((
            RigidBody::Static,
            Collider::cuboid(400.0, 2.0, 400.0),
            Transform::from_xyz(0.0, -1.0, 0.0),
        ));
        lab
    }

    /// Spawns a static box.
    pub fn wall(&mut self, size: Vec3, position: Vec3) -> Entity {
        self.app
            .world_mut()
            .spawn((
                RigidBody::Static,
                Collider::cuboid(size.x, size.y, size.z),
                Transform::from_translation(position),
            ))
            .id()
    }

    /// Spawns `body`.
    pub fn spawn(&mut self, body: Body) -> Entity {
        let mut entity = self.app.world_mut().spawn((
            body.kind,
            body.collider,
            Transform::from_translation(body.position).with_rotation(body.rotation),
            LinearVelocity(body.velocity),
            Restitution::new(body.restitution),
            Friction::new(body.friction),
            ColliderDensity(body.density),
        ));
        if body.swept {
            entity.insert(SweptCcd::default());
        }
        let id = entity.id();
        self.points.insert(id, body.points);
        id
    }

    /// Replaces the collider of `entity` with one of `kind` built from `mesh`, as a morph does
    /// every time its mesh changes. Returns whether avian could build one.
    pub fn replace_mesh(&mut self, entity: Entity, mesh: &Mesh, kind: ColliderKind) -> bool {
        let Some(collider) = kind.build(mesh) else {
            return false;
        };
        self.points.insert(entity, vertices(mesh));
        self.app.world_mut().entity_mut(entity).insert(collider);
        true
    }

    /// Swaps the collider of `entity` for one built from `mesh` and inserts its `RigidBody` again
    /// in the same step, which is what makes avian push the body out of whatever the new
    /// collider overlaps. Returns whether avian could build one.
    pub fn swap_mesh(&mut self, entity: Entity, mesh: &Mesh, kind: ColliderKind) -> bool {
        let built = self.replace_mesh(entity, mesh, kind);
        if built {
            let body = self.app.world().get::<RigidBody>(entity).copied();
            if let Some(body) = body {
                self.app.world_mut().entity_mut(entity).insert(body);
            }
        }
        built
    }

    /// Holds the centre of mass of `entity` in its plane, as the lab does for the pieces it
    /// shows, so that it can only move in x and y.
    pub fn lock_depth(&mut self, entity: Entity) {
        self.app
            .world_mut()
            .entity_mut(entity)
            .insert(LockedAxes::new().lock_translation_z());
    }

    /// Wakes `entity` and the bodies touching it, if they are asleep.
    pub fn wake(&mut self, entity: Entity) {
        let world = self.app.world_mut();
        world.commands().queue(WakeBody(entity));
        world.flush();
    }

    /// Sets the velocity of `entity`.
    pub fn set_velocity(&mut self, entity: Entity, velocity: Vec3) {
        self.app
            .world_mut()
            .entity_mut(entity)
            .insert(LinearVelocity(velocity));
    }

    /// Advances the world by `count` fixed steps.
    pub fn step(&mut self, count: usize) {
        for _ in 0..count {
            self.app.update();
        }
    }

    /// How many seconds of physics have run.
    pub fn seconds(&self) -> f32 {
        self.app.world().resource::<Time<Physics>>().elapsed_secs()
    }

    /// What `entity` is doing, or `None` once it is gone.
    pub fn read(&self, entity: Entity) -> Option<Reading> {
        let body = self.app.world().get_entity(entity).ok()?;
        let position = body.get::<Position>()?.0;
        let rotation = body.get::<Rotation>().map_or(Quat::IDENTITY, |r| r.0);
        let lowest = match self.points.get(&entity).filter(|points| !points.is_empty()) {
            Some(points) => points
                .iter()
                .map(|point| (position + rotation * *point).y)
                .fold(f32::MAX, f32::min),
            None => body.get::<ColliderAabb>().map_or(position.y, |a| a.min.y),
        };
        let local_center = body
            .get::<ComputedCenterOfMass>()
            .map_or(Vec3::ZERO, |c| c.0);
        Some(Reading {
            position,
            center: position + rotation * local_center,
            velocity: body.get::<LinearVelocity>().map_or(Vec3::ZERO, |v| v.0),
            lowest,
            mass: body.get::<ComputedMass>().map_or(0.0, |m| m.value()),
            asleep: body.contains::<Sleeping>(),
        })
    }
}

/// A table of results that is printed and written as a CSV file.
pub struct Table {
    name: &'static str,
    header: Vec<&'static str>,
    rows: Vec<Vec<String>>,
}

impl Table {
    /// A table called `name` with the given columns.
    pub fn new(name: &'static str, header: &[&'static str]) -> Self {
        Self {
            name,
            header: header.to_vec(),
            rows: Vec::new(),
        }
    }

    /// Adds a row.
    pub fn row(&mut self, cells: Vec<String>) {
        assert_eq!(cells.len(), self.header.len(), "{} row", self.name);
        self.rows.push(cells);
    }

    /// The rows added so far.
    pub fn rows(&self) -> &[Vec<String>] {
        &self.rows
    }

    /// Writes `target/physics_lab/<name>.csv`.
    pub fn write(&self) {
        let root = std::env::var_os("CARGO_TARGET_DIR").map_or_else(
            || std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/target")),
            std::path::PathBuf::from,
        );
        let folder = root.join("physics_lab");
        std::fs::create_dir_all(&folder).expect("the results folder can be made");
        let mut text = self.header.join(",");
        for row in &self.rows {
            text.push('\n');
            text.push_str(&row.join(","));
        }
        text.push('\n');
        std::fs::write(folder.join(format!("{}.csv", self.name)), text)
            .expect("the results can be written");
    }
}
