//! The state of the lab and the interface that edits it: shape chips dragged onto grid cells,
//! placed pieces dragged between cells, and a radial menu for everything about how the pieces
//! are simulated.

use avian3d::prelude::{Collider, RigidBody};
use bevy::prelude::*;
use bevy_composable_dragging::hud::{HudHost, Layout, Screen, Theme};
use bevy_composable_dragging::ops::closest_states;
use bevy_composable_dragging::radial::{Petal, RadialHost, Ring, glyph, push_menu};
use bevy_composable_dragging::{View, ViewNode};
use bevy_gradmesh::{Shape, ShapeDetail};

use crate::drag::speed_limit;
use crate::scenario::{ColliderKind, DETAILS, Primitive, detail_label};

/// The columns and rows of cells a piece can rest in.
pub const COLUMNS: i32 = 7;
pub const ROWS: i32 = 4;
/// The distance between the centres of two cells, which is also the size of a piece at scale 1.
pub const CELL: f32 = 1.6;
/// The size of a piece across at its widest, as a fraction of a cell.
const FILL: f32 = 0.7;
/// The height of the centre of the lowest row above the ground.
const FIRST_ROW: f32 = 1.0;
/// How far a chip may be from a cell for the preview to settle into it.
const SNAP: f32 = 0.5;

/// The sizes of a piece against the size it has in the editor.
pub const SCALES: [f32; 3] = [0.01, 1.0, 100.0];
/// The fixed steps of the simulation, as steps per second.
pub const RATES: [u32; 4] = [30, 60, 120, 240];
/// The solver substeps of each step.
pub const SUBSTEPS: [u32; 3] = [1, 6, 12];
/// The values of restitution and friction to choose from.
pub const COEFFICIENTS: [f32; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];
/// Gravity strengths.
pub const GRAVITIES: [f32; 5] = [0.0, 1.6, 4.0, 9.81, 20.0];
/// Board rotation angles in degrees.
pub const BOARD_ROTATIONS: [f32; 5] = [0.0, 45.0, 90.0, 135.0, 180.0];
/// Board width multipliers.
pub const BOARD_SIZES: [f32; 4] = [0.5, 1.0, 1.5, 2.0];
/// Floor restitution values.
pub const FLOOR_RESTITUTIONS: [f32; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];
/// Render layer counts.
pub const LAYER_COUNTS: [u8; 3] = [1, 2, 4];
/// Density multipliers.
pub const DENSITIES: [f32; 5] = [0.1, 0.5, 1.0, 5.0, 10.0];
/// Camera distance from the board.
pub const CAMERA_DISTANCES: [f32; 4] = [8.0, 14.0, 20.0, 30.0];
/// Camera height above the board.
pub const CAMERA_HEIGHTS: [f32; 4] = [2.0, 5.0, 9.0, 15.0];

/// A place a piece can rest in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub column: i32,
    pub row: i32,
}

impl Cell {
    /// Where the centre of the cell is in the editor, on the drag plane.
    pub fn center(self) -> Vec2 {
        Vec2::new(
            (self.column - COLUMNS / 2) as f32 * CELL,
            FIRST_ROW + self.row as f32 * CELL,
        )
    }

    /// Every cell, row by row from the bottom.
    pub fn all() -> impl Iterator<Item = Self> {
        (0..ROWS).flat_map(|row| (0..COLUMNS).map(move |column| Self { column, row }))
    }
}

/// A shape placed in a cell.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Piece {
    pub id: u32,
    pub primitive: Primitive,
    pub cell: Cell,
}

/// A wall placed on the board.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wall {
    pub id: u32,
    /// Size of the wall in world units (half-extents for avian cuboid).
    pub size: Vec3,
    /// Position of the wall center.
    pub position: Vec3,
}

/// How the pieces are built and stepped. Every field is an index into a table above, so no value
/// outside the tables can be chosen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Setup {
    pub collider: ColliderKind,
    pub detail: usize,
    pub rate: usize,
    pub substeps: usize,
    pub scale: usize,
    pub restitution: usize,
    pub friction: usize,
    pub gravity: usize,
    pub board_rotation: usize,
    pub board_size: usize,
    pub floor_restitution: usize,
    pub layers: usize,
    pub density: usize,
    pub camera_distance: usize,
    pub camera_height: usize,
}

impl Default for Setup {
    fn default() -> Self {
        Self {
            collider: ColliderKind::ConvexHull,
            detail: 2,
            rate: 1,
            substeps: 1,
            scale: 1,
            restitution: 0,
            friction: 1,
            gravity: 3,
            board_rotation: 0,
            board_size: 1,
            floor_restitution: 0,
            layers: 1,
            density: 2,
            camera_distance: 1,
            camera_height: 2,
        }
    }
}

impl Setup {
    /// The detail level the shapes are built at.
    pub fn detail(&self) -> ShapeDetail {
        DETAILS[self.detail]
    }

    /// The length of a fixed step in seconds.
    pub fn step(&self) -> f32 {
        1.0 / RATES[self.rate] as f32
    }

    /// How many solver substeps each step is divided into.
    pub fn substeps(&self) -> u32 {
        SUBSTEPS[self.substeps]
    }

    /// The size of a piece against the editor.
    pub fn scale(&self) -> f32 {
        SCALES[self.scale]
    }

    /// The restitution of every piece.
    pub fn restitution(&self) -> f32 {
        COEFFICIENTS[self.restitution]
    }

    /// The friction of every piece.
    pub fn friction(&self) -> f32 {
        COEFFICIENTS[self.friction]
    }

    /// Gravity strength.
    pub fn gravity(&self) -> f32 {
        GRAVITIES[self.gravity]
    }

    /// Board rotation in degrees.
    pub fn board_rotation(&self) -> f32 {
        BOARD_ROTATIONS[self.board_rotation]
    }

    /// Board size multiplier.
    pub fn board_size(&self) -> f32 {
        BOARD_SIZES[self.board_size]
    }

    /// Floor restitution.
    pub fn floor_restitution(&self) -> f32 {
        FLOOR_RESTITUTIONS[self.floor_restitution]
    }

    /// Number of render layers.
    pub fn layers(&self) -> u8 {
        LAYER_COUNTS[self.layers]
    }

    /// Density multiplier.
    pub fn density(&self) -> f32 {
        DENSITIES[self.density]
    }

    /// Camera distance from the board.
    pub fn camera_distance(&self) -> f32 {
        CAMERA_DISTANCES[self.camera_distance]
    }

    /// Camera height above the board.
    pub fn camera_height(&self) -> f32 {
        CAMERA_HEIGHTS[self.camera_height]
    }

    /// A one-line summary for the status panel.
    pub fn summary(&self) -> String {
        format!(
            "{} {} | {} Hz x{} | scale {} | bounce {} grip {} | gravity {} | board {}° x{} | floor {} | density {} | layers {} | cam {}x{}",
            self.collider.label(),
            detail_label(self.detail()),
            RATES[self.rate],
            self.substeps(),
            self.scale(),
            self.restitution(),
            self.friction(),
            self.gravity(),
            self.board_rotation(),
            self.board_size(),
            self.floor_restitution(),
            self.density(),
            self.layers(),
            self.camera_distance(),
            self.camera_height(),
        )
    }
}

/// A page of the radial menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Root,
    Collider,
    Detail,
    Rate,
    Substeps,
    Scale,
    Bounce,
    Grip,
    Gravity,
    BoardRotation,
    BoardSize,
    FloorBounce,
    Layers,
    Density,
    CameraDistance,
    CameraHeight,
}

impl Page {
    /// The pages the root opens.
    const SETTINGS: [Self; 14] = [
        Self::Collider,
        Self::Detail,
        Self::Rate,
        Self::Substeps,
        Self::Scale,
        Self::Bounce,
        Self::Grip,
        Self::Gravity,
        Self::BoardRotation,
        Self::BoardSize,
        Self::FloorBounce,
        Self::Layers,
        Self::Density,
        Self::CameraDistance,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::Root => "root",
            Self::Collider => "collider",
            Self::Detail => "detail",
            Self::Rate => "rate",
            Self::Substeps => "substeps",
            Self::Scale => "scale",
            Self::Bounce => "bounce",
            Self::Grip => "grip",
            Self::Gravity => "gravity",
            Self::BoardRotation => "board rotation",
            Self::BoardSize => "board size",
            Self::FloorBounce => "floor bounce",
            Self::Layers => "layers",
            Self::Density => "density",
            Self::CameraDistance => "camera distance",
            Self::CameraHeight => "camera height",
        }
    }
}

/// Something a petal of the menu does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    Open(Page),
    Collider(ColliderKind),
    Detail(usize),
    Rate(usize),
    Substeps(usize),
    Scale(usize),
    Bounce(usize),
    Grip(usize),
    Gravity(usize),
    BoardRotation(usize),
    BoardSize(usize),
    FloorBounce(usize),
    Layers(usize),
    Density(usize),
    CameraDistance(usize),
    CameraHeight(usize),
    Play,
    Clear,
    AddWall,
}

/// The state of the lab.
#[derive(Clone)]
pub struct Lab {
    pub screen: Screen,
    pub menu: Option<Page>,
    pub pieces: Vec<Piece>,
    pub walls: Vec<Wall>,
    pub next_id: u32,
    pub setup: Setup,
    pub running: bool,
}

impl Lab {
    /// A lab with a small tower already set up.
    pub fn new(screen: Screen) -> Self {
        let mut lab = Self {
            screen,
            menu: None,
            pieces: Vec::new(),
            walls: Vec::new(),
            next_id: 0,
            setup: Setup::default(),
            running: false,
        };
        let tower = [
            (Primitive::Cuboid, 3, 0),
            (Primitive::Cuboid, 3, 1),
            (Primitive::Sphere, 3, 2),
            (Primitive::Cylinder, 5, 0),
        ];
        for (primitive, column, row) in tower {
            lab = lab.with_piece(primitive, Cell { column, row });
        }
        lab
    }

    /// Whether a piece rests in `cell`.
    pub fn occupied(&self, cell: Cell) -> bool {
        self.pieces.iter().any(|piece| piece.cell == cell)
    }

    /// The cells nothing rests in.
    pub fn free_cells(&self) -> Vec<Cell> {
        Cell::all().filter(|cell| !self.occupied(*cell)).collect()
    }

    /// The id the next piece of `primitive` gets. Every chip of the palette has the id of the
    /// piece it would become, so a chip being dragged is the node that each state of the drop
    /// shows in a cell, and the drag can tell which cell it is nearest to.
    pub fn chip_number(&self, primitive: Primitive) -> u32 {
        self.next_id + palette_size(palette_index(primitive))
    }

    /// The id of the node of the chip of `primitive`.
    pub fn chip_id(&self, primitive: Primitive) -> String {
        format!("piece-{}", self.chip_number(primitive))
    }

    /// This lab with a new piece of `primitive` in `cell`.
    #[must_use]
    pub fn with_piece(&self, primitive: Primitive, cell: Cell) -> Self {
        let mut lab = self.clone();
        lab.pieces.push(Piece {
            id: self.chip_number(primitive),
            primitive,
            cell,
        });
        lab.next_id += palette_size(Primitive::ALL.len());
        lab
    }

    /// This lab with piece `id` moved to `cell`.
    #[must_use]
    pub fn with_piece_moved(&self, id: u32, cell: Cell) -> Self {
        let mut lab = self.clone();
        if let Some(piece) = lab.pieces.iter_mut().find(|piece| piece.id == id) {
            piece.cell = cell;
        }
        lab
    }

    /// The states a drag of a chip of `primitive` can drop in: nothing changes, or a piece is
    /// placed in one of the free cells.
    pub fn placements(&self, primitive: Primitive) -> Vec<Self> {
        let mut states = vec![self.clone()];
        states.extend(
            self.free_cells()
                .into_iter()
                .map(|cell| self.with_piece(primitive, cell)),
        );
        states
    }

    /// The states a drag of piece `id` can drop in: it stays, or it moves to a free cell.
    pub fn relocations(&self, id: u32) -> Vec<Self> {
        let mut states = vec![self.clone()];
        states.extend(
            self.free_cells()
                .into_iter()
                .map(|cell| self.with_piece_moved(id, cell)),
        );
        states
    }
}

/// What a body of the simulation is made of.
pub struct Parts {
    pub mesh: Mesh,
    pub collider: Collider,
    pub position: Vec3,
    pub body: RigidBody,
    pub restitution: f32,
    pub friction: f32,
    /// The fastest the body may be dragged, from its smallest dimension and the step.
    pub speed_limit: f32,
}

/// The body of `piece` under `setup`, or `None` when avian cannot build a collider for the
/// shape. A shape that is flat has no volume to rest on, so it falls back to a hull of its mesh
/// that is thickened by the collider itself.
pub fn parts(piece: &Piece, setup: &Setup) -> Option<Parts> {
    let scale = setup.scale();
    let shape: Shape = piece.primitive.shape(FILL * CELL * scale, setup.detail());
    let mesh = shape.build();
    let collider = setup.collider.build(&mesh)?;
    let size = shape.bounds().size();
    let smallest = size.to_array().into_iter().fold(f32::MAX, f32::min);
    let center = piece.cell.center() * scale;
    Some(Parts {
        mesh,
        collider,
        position: Vec3::new(center.x, center.y, 0.0),
        body: RigidBody::Dynamic,
        restitution: setup.restitution(),
        friction: setup.friction(),
        speed_limit: speed_limit(smallest.max(f32::EPSILON), setup.step()),
    })
}

impl Petal<Lab> for Item {
    fn name(&self) -> String {
        match self {
            Self::Open(page) => format!("open-{}", page.name()),
            Self::Collider(kind) => format!("collider-{}", kind.label()),
            Self::Detail(index) => format!("detail-{index}"),
            Self::Rate(index) => format!("rate-{index}"),
            Self::Substeps(index) => format!("substeps-{index}"),
            Self::Scale(index) => format!("scale-{index}"),
            Self::Bounce(index) => format!("bounce-{index}"),
            Self::Grip(index) => format!("grip-{index}"),
            Self::Gravity(index) => format!("gravity-{index}"),
            Self::BoardRotation(index) => format!("board-rotation-{index}"),
            Self::BoardSize(index) => format!("board-size-{index}"),
            Self::FloorBounce(index) => format!("floor-bounce-{index}"),
            Self::Layers(index) => format!("layers-{index}"),
            Self::Density(index) => format!("density-{index}"),
            Self::CameraDistance(index) => format!("camera-distance-{index}"),
            Self::CameraHeight(index) => format!("camera-height-{index}"),
            Self::Play => "play".into(),
            Self::Clear => "clear".into(),
            Self::AddWall => "add-wall".into(),
        }
    }

    fn label(&self, lab: &Lab) -> String {
        match self {
            Self::Open(page) => page.name(),
            Self::Collider(kind) => kind.label().into(),
            Self::Detail(index) => detail_label(DETAILS[*index]).into(),
            Self::Rate(index) => format!("{} Hz", RATES[*index]),
            Self::Substeps(index) => format!("{} sub", SUBSTEPS[*index]),
            Self::Scale(index) => format!("x{}", SCALES[*index]),
            Self::Bounce(index) | Self::Grip(index) => format!("{}", COEFFICIENTS[*index]),
            Self::Gravity(index) => format!("{:.1}", GRAVITIES[*index]),
            Self::BoardRotation(index) => format!("{}°", BOARD_ROTATIONS[*index]),
            Self::BoardSize(index) => format!("x{}", BOARD_SIZES[*index]),
            Self::FloorBounce(index) => format!("{}", FLOOR_RESTITUTIONS[*index]),
            Self::Layers(index) => format!("{}", LAYER_COUNTS[*index]),
            Self::Density(index) => format!("{}", DENSITIES[*index]),
            Self::CameraDistance(index) => format!("{}", CAMERA_DISTANCES[*index]),
            Self::CameraHeight(index) => format!("{}", CAMERA_HEIGHTS[*index]),
            Self::Play if lab.running => "stop".into(),
            Self::Play => "play".into(),
            Self::Clear => "clear".into(),
            Self::AddWall => "add wall".into(),
        }
    }

    fn accent(&self) -> Color {
        match self {
            Self::Open(_) => Color::srgb(0.35, 0.5, 0.85),
            Self::Play => Color::srgb(0.3, 0.75, 0.45),
            Self::Clear => Color::srgb(0.85, 0.35, 0.3),
            Self::AddWall => Color::srgb(0.6, 0.5, 0.85),
            _ => Color::srgb(0.9, 0.65, 0.25),
        }
    }

    fn glyph(&self) -> Shape {
        glyph::dots(5.0, &[(0.0, 0.0)])
    }

    fn is_active(&self, lab: &Lab) -> bool {
        let setup = &lab.setup;
        match self {
            Self::Collider(kind) => setup.collider == *kind,
            Self::Detail(index) => setup.detail == *index,
            Self::Rate(index) => setup.rate == *index,
            Self::Substeps(index) => setup.substeps == *index,
            Self::Scale(index) => setup.scale == *index,
            Self::Bounce(index) => setup.restitution == *index,
            Self::Grip(index) => setup.friction == *index,
            Self::Gravity(index) => setup.gravity == *index,
            Self::BoardRotation(index) => setup.board_rotation == *index,
            Self::BoardSize(index) => setup.board_size == *index,
            Self::FloorBounce(index) => setup.floor_restitution == *index,
            Self::Layers(index) => setup.layers == *index,
            Self::Density(index) => setup.density == *index,
            Self::CameraDistance(index) => setup.camera_distance == *index,
            Self::CameraHeight(index) => setup.camera_height == *index,
            Self::Play => lab.running,
            Self::Open(_) | Self::Clear | Self::AddWall => false,
        }
    }

    fn press(&self, lab: &Lab) -> Lab {
        let mut next = Lab {
            menu: None,
            ..lab.clone()
        };
        match self {
            Self::Open(page) => next.menu = Some(*page),
            Self::Collider(kind) => next.setup.collider = *kind,
            Self::Detail(index) => next.setup.detail = *index,
            Self::Rate(index) => next.setup.rate = *index,
            Self::Substeps(index) => next.setup.substeps = *index,
            Self::Scale(index) => next.setup.scale = *index,
            Self::Bounce(index) => next.setup.restitution = *index,
            Self::Grip(index) => next.setup.friction = *index,
            Self::Gravity(index) => next.setup.gravity = *index,
            Self::BoardRotation(index) => next.setup.board_rotation = *index,
            Self::BoardSize(index) => next.setup.board_size = *index,
            Self::FloorBounce(index) => next.setup.floor_restitution = *index,
            Self::Layers(index) => next.setup.layers = *index,
            Self::Density(index) => next.setup.density = *index,
            Self::CameraDistance(index) => next.setup.camera_distance = *index,
            Self::CameraHeight(index) => next.setup.camera_height = *index,
            Self::Play => next.running = !lab.running,
            Self::Clear => {
                next.pieces.clear();
                next.walls.clear();
                next.running = false;
            }
            Self::AddWall => {
                let wall_id = next.next_id;
                next.next_id += 1;
                next.walls.push(Wall {
                    id: wall_id,
                    size: Vec3::new(0.15, 2.0, 2.0),
                    position: Vec3::new(0.0, 2.0, 0.0),
                });
            }
        }
        next
    }
}

impl Ring<Lab> for Page {
    type Item = Item;

    fn name(&self) -> String {
        Self::name(*self).into()
    }

    fn items(&self, _lab: &Lab) -> Vec<Item> {
        match self {
            Self::Root => {
                let mut items: Vec<Item> = Self::SETTINGS.into_iter().map(Item::Open).collect();
                items.extend([Item::Play, Item::Clear, Item::AddWall]);
                items
            }
            Self::Collider => ColliderKind::ALL.into_iter().map(Item::Collider).collect(),
            Self::Detail => (0..DETAILS.len()).map(Item::Detail).collect(),
            Self::Rate => (0..RATES.len()).map(Item::Rate).collect(),
            Self::Substeps => (0..SUBSTEPS.len()).map(Item::Substeps).collect(),
            Self::Scale => (0..SCALES.len()).map(Item::Scale).collect(),
            Self::Bounce => (0..COEFFICIENTS.len()).map(Item::Bounce).collect(),
            Self::Grip => (0..COEFFICIENTS.len()).map(Item::Grip).collect(),
            Self::Gravity => (0..GRAVITIES.len()).map(Item::Gravity).collect(),
            Self::BoardRotation => (0..BOARD_ROTATIONS.len())
                .map(Item::BoardRotation)
                .collect(),
            Self::BoardSize => (0..BOARD_SIZES.len()).map(Item::BoardSize).collect(),
            Self::FloorBounce => (0..FLOOR_RESTITUTIONS.len())
                .map(Item::FloorBounce)
                .collect(),
            Self::Layers => (0..LAYER_COUNTS.len()).map(Item::Layers).collect(),
            Self::Density => (0..DENSITIES.len()).map(Item::Density).collect(),
            Self::CameraDistance => (0..CAMERA_DISTANCES.len())
                .map(Item::CameraDistance)
                .collect(),
            Self::CameraHeight => (0..CAMERA_HEIGHTS.len()).map(Item::CameraHeight).collect(),
        }
    }

    fn all(_lab: &Lab) -> Vec<Self> {
        let mut pages = vec![Self::Root];
        pages.extend(Self::SETTINGS);
        pages.push(Self::CameraHeight);
        pages
    }
}

impl HudHost for Lab {
    fn screen(&self) -> &Screen {
        &self.screen
    }

    fn with_screen(&self, screen: Screen) -> Self {
        Self {
            screen,
            ..self.clone()
        }
    }
}

impl RadialHost for Lab {
    type Ring = Page;

    fn root(&self) -> Page {
        Page::Root
    }

    fn menu(&self) -> Option<Page> {
        self.menu
    }

    fn with_menu(&self, menu: Option<Page>) -> Self {
        Self {
            menu,
            ..self.clone()
        }
    }

    fn pop_center(&self, layout: &Layout) -> Vec2 {
        layout.home()
    }

    fn status(&self) -> String {
        let state = if self.running { "running" } else { "editing" };
        format!("physics lab, {state}\n{}", self.setup.summary())
    }
}

/// Where `primitive` is in the palette.
fn palette_index(primitive: Primitive) -> usize {
    Primitive::ALL
        .iter()
        .position(|candidate| *candidate == primitive)
        .unwrap_or(0)
}

/// A count of palette entries as an id offset.
fn palette_size(count: usize) -> u32 {
    u32::try_from(count).expect("the palette has few shapes")
}

/// The colour of a shape of the palette.
pub fn tint(primitive: Primitive) -> Color {
    Color::hsl(
        palette_index(primitive) as f32 * 360.0 / Primitive::ALL.len() as f32,
        0.6,
        0.6,
    )
}

/// The place of the chip of the palette at `index`, in a column left of the cells.
fn chip_position(index: usize) -> Vec2 {
    let x = -(COLUMNS / 2) as f32 * CELL - 2.2;
    Vec2::new(x + (index % 2) as f32 * 1.1, 0.5 + (index / 2) as f32 * 1.1)
}

/// The scene and the menu of the lab.
///
/// While the simulation runs the pieces are not drawn here, because the bodies of the
/// simulation draw themselves.
pub fn view(lab: &Lab, theme: &Theme) -> View<Lab> {
    let mut view = View::new();
    let width = COLUMNS as f32 * CELL;
    view.push(
        ViewNode::cuboid("ground", Vec3::new(width + 6.0, 0.4, 3.0), 0.05)
            .at(Vec3::new(0.0, -0.2, 0.0))
            .with_color(Color::srgb(0.3, 0.32, 0.36)),
    );
    if !lab.running {
        for cell in Cell::all() {
            let at = cell.center();
            view.push(
                ViewNode::rectangle(
                    format!("cell-{}-{}", cell.column, cell.row),
                    Vec2::splat(CELL * 0.92),
                )
                .at(Vec3::new(at.x, at.y, -0.4))
                .with_color(Color::srgba(1.0, 1.0, 1.0, 0.05))
                .pickable(false),
            );
        }
        for (index, primitive) in Primitive::ALL.into_iter().enumerate() {
            let shape = primitive.shape(1.0, lab.setup.detail());
            let chip = ViewNode::grad(lab.chip_id(primitive), shape)
                .at_xy(chip_position(index))
                .scaled(Vec3::splat(0.8))
                .with_color(tint(primitive))
                .draggable_by(move |lab: &Lab| {
                    closest_states(lab.placements(primitive))
                        .with_floating()
                        .with_snap_radius(SNAP)
                });
            view.push(chip);
        }
        for piece in &lab.pieces {
            let id = piece.id;
            let shape = piece.primitive.shape(FILL * CELL, lab.setup.detail());
            view.push(
                ViewNode::grad(format!("piece-{id}"), shape)
                    .at_xy(piece.cell.center())
                    .with_color(tint(piece.primitive))
                    .draggable_by(move |lab: &Lab| {
                        closest_states(lab.relocations(id))
                            .with_floating()
                            .with_snap_radius(SNAP)
                    }),
            );
        }
    }
    let layout = Layout::new(lab.screen(), *theme);
    push_menu(&mut view, lab, &layout, theme);
    view
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    fn lab() -> Lab {
        Lab::new(Screen::default())
    }

    #[test]
    #[allow(clippy::cast_possible_truncation)]
    fn the_cells_are_all_distinct_and_fit_the_grid() {
        let cells: Vec<Cell> = Cell::all().collect();
        assert_eq!(i32::try_from(cells.len()), Ok(COLUMNS * ROWS));
        let centers: HashSet<(i32, i32)> = cells
            .iter()
            .map(|cell| {
                let at = cell.center();
                ((at.x * 100.0) as i32, (at.y * 100.0) as i32)
            })
            .collect();
        assert_eq!(centers.len(), cells.len());
    }

    #[test]
    fn a_chip_can_be_dropped_in_every_free_cell_and_nowhere_else() {
        let lab = lab();
        let free = lab.free_cells().len();
        let placements = lab.placements(Primitive::Torus);
        assert_eq!(placements.len(), free + 1);
        assert_eq!(
            placements[0].pieces, lab.pieces,
            "the first state is no change"
        );
        for placed in &placements[1..] {
            assert_eq!(placed.pieces.len(), lab.pieces.len() + 1);
            let cells: HashSet<(i32, i32)> = placed
                .pieces
                .iter()
                .map(|piece| (piece.cell.column, piece.cell.row))
                .collect();
            assert_eq!(cells.len(), placed.pieces.len(), "two pieces share a cell");
        }
    }

    #[test]
    fn a_placed_piece_keeps_its_id_when_it_moves() {
        let lab = lab();
        let id = lab.pieces[0].id;
        let target = lab.free_cells()[0];
        let moved = lab.with_piece_moved(id, target);
        let piece = moved.pieces.iter().find(|piece| piece.id == id).unwrap();
        assert_eq!(piece.cell, target);
        assert_eq!(moved.pieces.len(), lab.pieces.len());
    }

    #[test]
    fn piece_ids_are_never_reused() {
        let lab = lab().with_piece(Primitive::Cone, Cell { column: 0, row: 3 });
        let ids: HashSet<u32> = lab.pieces.iter().map(|piece| piece.id).collect();
        assert_eq!(ids.len(), lab.pieces.len());
    }

    #[test]
    fn every_setting_has_petals_that_change_only_that_setting() {
        let lab = lab();
        for page in Page::all(&lab) {
            for item in page.items(&lab) {
                let next = item.press(&lab);
                if item != Item::Clear {
                    assert_eq!(next.pieces, lab.pieces, "{item:?} changed the pieces");
                }
                if let Item::Open(opened) = item {
                    assert_eq!(next.menu, Some(opened));
                } else {
                    assert_eq!(next.menu, None, "{item:?} leaves the menu open");
                }
            }
        }
    }

    #[test]
    fn petal_names_are_unique() {
        let lab = lab();
        let mut names = HashSet::new();
        for page in Page::all(&lab) {
            for item in page.items(&lab) {
                if matches!(item, Item::Open(_)) || page == Page::Root {
                    continue;
                }
                assert!(names.insert(item.name()), "{item:?} repeats a name");
            }
        }
    }

    #[test]
    fn play_toggles_and_clear_empties_and_stops() {
        let lab = lab();
        let running = Item::Play.press(&lab);
        assert!(running.running);
        assert!(!Item::Play.press(&running).running);
        let cleared = Item::Clear.press(&running);
        assert!(cleared.pieces.is_empty() && !cleared.running);
    }

    #[test]
    fn the_setup_only_reaches_values_of_the_tables() {
        let lab = lab();
        let mut setup = lab.setup;
        for index in 0..RATES.len() {
            setup.rate = index;
            assert!(setup.step() > 0.0);
        }
        for index in 0..SCALES.len() {
            setup.scale = index;
            assert!(setup.scale() > 0.0);
        }
    }

    #[test]
    fn every_shape_of_the_palette_becomes_a_body_with_a_speed_limit() {
        let mut setup = Setup::default();
        for collider in [ColliderKind::Trimesh, ColliderKind::ConvexHull] {
            setup.collider = collider;
            for primitive in Primitive::ALL {
                let piece = Piece {
                    id: 0,
                    primitive,
                    cell: Cell { column: 3, row: 1 },
                };
                let parts = parts(&piece, &setup)
                    .unwrap_or_else(|| panic!("{primitive:?} has no {collider:?}"));
                assert!(parts.speed_limit.is_finite() && parts.speed_limit > 0.0);
                assert!(parts.position.y > 0.0);
            }
        }
    }

    #[test]
    fn the_view_has_a_chip_for_every_shape_and_hides_the_pieces_while_running() {
        let theme = Theme::new(2);
        let lab = lab();
        let editing = view(&lab, &theme);
        for primitive in Primitive::ALL {
            assert!(editing.get(&lab.chip_id(primitive)).is_some());
        }
        for piece in &lab.pieces {
            assert!(editing.get(&format!("piece-{}", piece.id)).is_some());
        }
        let running = view(&Item::Play.press(&lab), &theme);
        assert!(running.get("piece-0").is_none());
        assert!(running.get("ground").is_some());
        assert!(running.get("menu/hub").is_some());
    }
}
