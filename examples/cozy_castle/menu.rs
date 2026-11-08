//! The tools and precision of the castle, and what its radial menu holds.
//!
//! The menu is the library [`radial`] menu on the [`hud`](bevy_composable_dragging::hud): this
//! module only says what the castle puts in it. [`Ring`] and [`Item`] implement the traits of the
//! library for the rings and the petals, and [`Castle`] implements them for the state, which
//! says where the menu pops up and what the status panel says.
//!
//! With nothing selected the hub rests as a small button in a corner and acts on the ground: it
//! places pieces and offers the tools of the ground (Select, Draw, Sculpt, Cutaway). Selecting a
//! piece or a wall pops the hub up over it, and it offers the tools that edit what is selected.
//! Choosing a tool that moves, turns, scales or reshapes shrinks the hub to a chip above the
//! element and reveals the gizmo or the handles of the curve, so the hub never covers what it
//! edits. A status panel in the corner says what is selected and which tool and precision are
//! chosen.
//!
//! The engine has press, drag and release, so the menu works in modes. Pressing the hub or a
//! petal and releasing it commits a fixed state: a ring opens, a tool or a precision is chosen,
//! or a piece is armed. An armed piece is placed by the next drag across the ground. The tool
//! chosen decides what a drag of a piece or a wall does next, and the precision sets the steps
//! of those drags and of the gizmo.

use std::f32::consts::{FRAC_PI_4, PI};

use bevy::prelude::*;
use bevy_composable_dragging::hud::{HudHost, Layout, Screen};
use bevy_composable_dragging::ops::{Snap, across_plane, along_axis, around_axis, custom, state};
use bevy_composable_dragging::radial::glyph::{dots, outline, rectangle};
use bevy_composable_dragging::radial::{self, RadialHost, Ring as _};
use bevy_composable_dragging::{DragBehavior, DragSpace, DragSpec, Pointer};
use bevy_gradmesh::{Shape, ShapeDetail, ShapePart};

use crate::select::select_spec;
use crate::solid::cutaway;
use crate::terrain::sculpt;
use crate::wall::{Profile, draw};
use crate::{Castle, PieceKind, move_spec, piece_id};

/// How far a copy of a piece or wall is put from the original.
const COPY_OFFSET: f32 = 0.6;

/// A group of the pieces in the menu.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Category {
    Towers,
    Gates,
    Paths,
    Decor,
}

impl Category {
    pub const ALL: [Self; 4] = [Self::Towers, Self::Gates, Self::Paths, Self::Decor];

    /// The pieces of the category.
    pub fn pieces(self) -> &'static [PieceKind] {
        match self {
            Self::Towers => &[PieceKind::Tower, PieceKind::Keep],
            Self::Gates => &[
                PieceKind::ArchGate,
                PieceKind::SquareGate,
                PieceKind::WoodGate,
            ],
            Self::Paths => &[PieceKind::Cobble],
            Self::Decor => &[
                PieceKind::Outpost,
                PieceKind::Torch,
                PieceKind::Banner,
                PieceKind::Cottage,
            ],
        }
    }
}

/// What a drag of a piece or a wall, or of the ground, does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    /// Pressing selects. Pressing the ground deselects.
    Select,
    /// Drags move, and pieces align to their neighbours.
    Move,
    /// Drags turn about the vertical axis through the piece.
    Rotate,
    /// Drags raise and lower.
    Raise,
    /// The handles of the gizmo scale a wall.
    Scale,
    /// The handles of the curve of a wall are shown, to move its control points.
    Reshape,
    /// Drags across the ground draw a wall.
    Draw,
    /// Dragging up from a point of the ground raises it, dragging down lowers it.
    Sculpt,
    /// Dragging across the ground cuts through the walls and towers under it.
    Cutaway,
    /// Pressing copies the piece or wall.
    Duplicate,
    /// Pressing removes the piece or wall.
    Delete,
}

impl Tool {
    fn label(self) -> &'static str {
        match self {
            Self::Select => "Select",
            Self::Move => "Move",
            Self::Rotate => "Rotate",
            Self::Raise => "Raise",
            Self::Scale => "Scale",
            Self::Reshape => "Reshape",
            Self::Draw => "Draw wall",
            Self::Sculpt => "Sculpt",
            Self::Cutaway => "Cutaway",
            Self::Duplicate => "Duplicate",
            Self::Delete => "Delete",
        }
    }

    /// Whether the tool shows the gizmo of the selection: the other tools leave it hidden.
    pub fn reveals_gizmo(self) -> bool {
        matches!(self, Self::Move | Self::Rotate | Self::Raise | Self::Scale)
    }

    /// Whether the tool edits the selection in place, by its gizmo or its handles or by pressing
    /// it, so the menu keeps clear of it.
    fn edits_in_place(self) -> bool {
        self.reveals_gizmo() || matches!(self, Self::Reshape | Self::Duplicate | Self::Delete)
    }
}

/// What the menu acts on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Context {
    /// Nothing is selected, so the menu works on the ground: it places pieces, draws walls,
    /// sculpts the terrain and cuts.
    Ground,
    /// A piece is selected, so the menu edits that.
    Selection,
    /// A wall is selected, which can also be scaled and reshaped.
    Wall,
}

impl Context {
    /// The tools that make sense in the context, in the order of the ring.
    pub fn tools(self) -> &'static [Tool] {
        match self {
            Self::Ground => &[Tool::Select, Tool::Draw, Tool::Sculpt, Tool::Cutaway],
            Self::Selection => &[
                Tool::Select,
                Tool::Move,
                Tool::Rotate,
                Tool::Raise,
                Tool::Duplicate,
                Tool::Delete,
            ],
            Self::Wall => &[
                Tool::Select,
                Tool::Move,
                Tool::Rotate,
                Tool::Raise,
                Tool::Scale,
                Tool::Reshape,
                Tool::Duplicate,
                Tool::Delete,
            ],
        }
    }
}

/// How finely drags and the gizmo step.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Precision {
    /// No steps.
    Free,
    /// Steps of a tenth of a unit and a twelfth of a half turn.
    Fine,
    /// Steps of half a unit and an eighth of a turn.
    Coarse,
}

impl Precision {
    pub const ALL: [Self; 3] = [Self::Free, Self::Fine, Self::Coarse];

    /// The steps of lengths, and of the factors scale handles multiply by.
    pub fn snap(self) -> Snap {
        match self {
            Self::Free => Snap::NONE,
            Self::Fine => Snap::step(0.1),
            Self::Coarse => Snap::step(0.5),
        }
    }

    /// The radius of the brush that sculpts the terrain.
    pub fn brush(self) -> f32 {
        match self {
            Self::Free => 0.6,
            Self::Fine => 0.35,
            Self::Coarse => 1.0,
        }
    }

    /// The steps of angles, in radians.
    pub fn turn(self) -> Snap {
        match self {
            Self::Free => Snap::NONE,
            Self::Fine => Snap::step(PI / 12.0),
            Self::Coarse => Snap::step(FRAC_PI_4),
        }
    }
}

/// What the next wall that is drawn is made of.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WallStyle {
    /// The stone wall with battlements.
    Stone,
    /// A thick, tall rampart.
    Rampart,
    /// A thin timber palisade.
    Palisade,
    /// A low garden wall.
    Garden,
}

impl WallStyle {
    pub const ALL: [Self; 4] = [Self::Stone, Self::Rampart, Self::Palisade, Self::Garden];

    /// The cross-section of walls of the style.
    pub fn profile(self) -> Profile {
        match self {
            Self::Stone => Profile::stone(),
            Self::Rampart => Profile::rampart(),
            Self::Palisade => Profile::palisade(),
            Self::Garden => Profile::garden(),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Stone => "Stone",
            Self::Rampart => "Rampart",
            Self::Palisade => "Palisade",
            Self::Garden => "Garden",
        }
    }
}

/// A ring of petals around the hub.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ring {
    /// Opens the rings of pieces, walls, tools and precision.
    Root,
    /// The categories of pieces.
    Pieces,
    /// The styles of the walls that are drawn.
    Walls,
    /// The pieces of a category.
    Category(Category),
    /// The tools of the context.
    Tools,
    /// The precision of drags and of the gizmo.
    Precision,
}

impl radial::Ring<Castle> for Ring {
    type Item = Item;

    fn name(&self) -> String {
        match *self {
            Self::Root => "root".into(),
            Self::Pieces => "pieces".into(),
            Self::Walls => "walls".into(),
            Self::Category(category) => format!("{category:?}"),
            Self::Tools => "tools".into(),
            Self::Precision => "precision".into(),
        }
    }

    /// The petals of the ring in the context of the selection. Pieces are only placed on the
    /// ground, and the tools are those of the ground or of the selection.
    fn items(&self, castle: &Castle) -> Vec<Item> {
        let context = castle.context();
        match *self {
            Self::Root if context == Context::Ground => vec![
                Item::Open(Self::Pieces),
                Item::Open(Self::Walls),
                Item::Open(Self::Tools),
                Item::Open(Self::Precision),
            ],
            Self::Root => vec![Item::Open(Self::Tools), Item::Open(Self::Precision)],
            Self::Pieces => Category::ALL
                .map(|category| Item::Open(Self::Category(category)))
                .to_vec(),
            Self::Category(category) => {
                category.pieces().iter().copied().map(Item::Piece).collect()
            }
            Self::Walls => WallStyle::ALL.map(Item::Wall).to_vec(),
            Self::Tools => context.tools().iter().copied().map(Item::Tool).collect(),
            Self::Precision => Precision::ALL.map(Item::Precision).to_vec(),
        }
    }

    /// Every ring, which are all in every view.
    fn all(_castle: &Castle) -> Vec<Self> {
        let mut rings = vec![Self::Root, Self::Pieces];
        rings.extend(Category::ALL.map(Self::Category));
        rings.extend([Self::Walls, Self::Tools, Self::Precision]);
        rings
    }
}

/// A petal.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Item {
    /// Opens a ring.
    Open(Ring),
    /// Arms a piece to be placed.
    Piece(PieceKind),
    /// Chooses the style of the next wall.
    Wall(WallStyle),
    /// Chooses a tool.
    Tool(Tool),
    /// Chooses the precision.
    Precision(Precision),
}

impl radial::Petal<Castle> for Item {
    fn name(&self) -> String {
        match *self {
            Self::Open(ring) => ring.name(),
            Self::Piece(kind) => format!("{kind:?}"),
            Self::Wall(style) => format!("{style:?}"),
            Self::Tool(tool) => format!("{tool:?}"),
            Self::Precision(precision) => format!("{precision:?}"),
        }
    }

    fn label(&self, castle: &Castle) -> String {
        let context = castle.context();
        match *self {
            Self::Open(Ring::Root) => "Menu".into(),
            Self::Open(Ring::Pieces) => "Pieces".into(),
            Self::Open(Ring::Walls) => "Walls".into(),
            Self::Open(Ring::Tools) if context == Context::Ground => "Ground".into(),
            Self::Open(Ring::Tools) => "Edit".into(),
            Self::Open(Ring::Precision) => "Precision".into(),
            Self::Open(Ring::Category(category)) => format!("{category:?}"),
            Self::Piece(kind) => kind.label().into(),
            Self::Wall(style) => style.label().into(),
            Self::Tool(tool) => tool.label().into(),
            Self::Precision(precision) => format!("{precision:?}"),
        }
    }

    /// The color of the petal, dark enough for its white glyph to read on it.
    fn accent(&self) -> Color {
        match *self {
            Self::Open(Ring::Root) => Color::srgb(0.46, 0.48, 0.56),
            Self::Open(Ring::Walls) | Self::Wall(_) => Color::srgb(0.5, 0.36, 0.3),
            Self::Open(Ring::Tools) | Self::Tool(_) => Color::srgb(0.24, 0.42, 0.8),
            Self::Open(Ring::Precision) | Self::Precision(_) => Color::srgb(0.2, 0.52, 0.32),
            Self::Open(_) => Color::srgb(0.74, 0.5, 0.14),
            Self::Piece(kind) => piece_color(kind).mix(&Color::BLACK, 0.38),
        }
    }

    fn glyph(&self) -> Shape {
        self.shape()
    }

    /// Whether the petal is the current choice.
    fn is_active(&self, castle: &Castle) -> bool {
        match *self {
            Self::Open(_) => false,
            Self::Piece(kind) => castle.armed == Some(kind),
            Self::Wall(style) => castle.wall_style == style,
            Self::Tool(tool) => castle.tool == tool,
            Self::Precision(precision) => castle.precision == precision,
        }
    }

    /// The castle after pressing the petal: a ring opens, or the choice is made and the menu
    /// closes.
    fn press(&self, castle: &Castle) -> Castle {
        let mut castle = castle.clone();
        match *self {
            Self::Open(ring) => {
                castle.menu = Some(ring);
                return castle;
            }
            Self::Piece(kind) => castle.armed = Some(kind),
            Self::Wall(style) => {
                castle.wall_style = style;
                castle.tool = Tool::Draw;
            }
            Self::Tool(tool) => castle.tool = tool,
            Self::Precision(precision) => castle.precision = precision,
        }
        castle.menu = None;
        castle
    }
}

impl PieceKind {
    fn label(self) -> &'static str {
        match self {
            Self::Tower => "Tower",
            Self::Keep => "Keep",
            Self::ArchGate => "Arch gate",
            Self::SquareGate => "Square gate",
            Self::WoodGate => "Wood gate",
            Self::Cobble => "Cobble",
            Self::Outpost => "Outpost",
            Self::Torch => "Torch",
            Self::Banner => "Banner",
            Self::Cottage => "Cottage",
        }
    }
}

fn piece_color(kind: PieceKind) -> Color {
    match kind {
        PieceKind::Tower | PieceKind::ArchGate | PieceKind::SquareGate => {
            Color::srgb(0.62, 0.62, 0.66)
        }
        PieceKind::Keep | PieceKind::Cobble => Color::srgb(0.85, 0.8, 0.7),
        PieceKind::WoodGate | PieceKind::Torch => Color::srgb(0.6, 0.4, 0.22),
        PieceKind::Outpost => Color::srgb(0.86, 0.7, 0.32),
        PieceKind::Banner => Color::srgb(0.85, 0.2, 0.22),
        PieceKind::Cottage => Color::srgb(0.8, 0.38, 0.28),
    }
}

// ---------------------------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------------------------

impl HudHost for Castle {
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

impl RadialHost for Castle {
    type Ring = Ring;

    fn root(&self) -> Ring {
        Ring::Root
    }

    fn menu(&self) -> Option<Ring> {
        self.menu
    }

    fn with_menu(&self, menu: Option<Ring>) -> Self {
        Self {
            menu,
            ..self.clone()
        }
    }

    /// Over the selection, which the hub pops up over, or else in the corner the hub rests in.
    fn pop_center(&self, layout: &Layout) -> Vec2 {
        let Some(at) = self
            .selection_anchor()
            .and_then(|anchor| self.screen.project(anchor))
        else {
            return layout.home();
        };
        let lift = if self.is_chip() {
            Vec2::Y * layout.chip_lift()
        } else {
            Vec2::ZERO
        };
        layout.keep_inside(at + lift)
    }

    fn is_popped(&self) -> bool {
        self.selected.is_some()
    }

    /// The hub has shrunk to a chip above the selection, which a tool that edits it in place
    /// does once the menu is closed.
    fn is_chip(&self) -> bool {
        self.selected.is_some() && self.menu.is_none() && self.tool.edits_in_place()
    }

    fn armed(&self) -> Option<Item> {
        self.armed.map(Item::Piece)
    }

    fn disarmed(&self) -> Self {
        Self {
            armed: None,
            ..self.clone()
        }
    }

    /// What is selected or being placed, and the tool and the precision.
    fn status(&self) -> String {
        let first = match self.armed {
            Some(kind) => format!("Placing {}", kind.label()),
            None => self.selection_name(),
        };
        format!("{first}\n{} - {:?}", self.tool.label(), self.precision)
    }
}

impl Castle {
    /// What the menu acts on: a wall or a piece that is selected, or else the ground.
    pub fn context(&self) -> Context {
        match self.selected {
            None => Context::Ground,
            Some(id) if self.walls.iter().any(|piece| piece.id == id) => Context::Wall,
            Some(_) => Context::Selection,
        }
    }

    /// The point of the scene the menu pops up over: the middle of the selected piece or wall.
    fn selection_anchor(&self) -> Option<Vec3> {
        let id = self.selected?;
        if let Some(piece) = self.piece(id) {
            return Some(piece.transform.translation + Vec3::Y * piece.kind.top() / 2.0);
        }
        let wall = &self.walls.iter().find(|piece| piece.id == id)?.wall;
        let count = wall.curve.points.len();
        let middle = (0..count).map(|index| wall.point(index)).sum::<Vec3>() / count as f32;
        let ground = self.terrain.surface(Vec2::new(middle.x, middle.z));
        Some(middle + Vec3::Y * (ground + wall.scaled_profile().top() / 2.0))
    }

    /// The name of what is selected, or of the ground.
    fn selection_name(&self) -> String {
        match self.selected {
            None => "Ground".into(),
            Some(id) => self
                .piece(id)
                .map_or("Wall", |piece| piece.kind.label())
                .into(),
        }
    }

    /// The castle with a new piece of `kind` at `position`, and the id of the piece.
    pub fn created(&self, kind: PieceKind, position: Vec2) -> (Self, u32) {
        let mut castle = self.clone();
        let id = castle.next_id;
        castle.add(kind, position.x, position.y, 0.0);
        (castle, id)
    }

    /// The castle without piece or wall `id`.
    pub fn without(&self, id: u32) -> Self {
        let mut castle = self.clone();
        castle.placed.retain(|piece| piece.id != id);
        castle.walls.retain(|piece| piece.id != id);
        if castle.selected == Some(id) {
            castle.selected = None;
        }
        castle
    }

    /// The castle with a copy of piece or wall `id` put beside it and selected.
    pub fn duplicated(&self, id: u32) -> Self {
        let beside = Vec3::new(COPY_OFFSET, 0.0, COPY_OFFSET);
        let copy = self.next_id;
        let mut castle = if let Some(piece) = self.piece(id) {
            let pose = piece.pose();
            let mut castle = self.clone();
            castle.add(
                piece.kind,
                pose.position.x + COPY_OFFSET,
                pose.position.y + COPY_OFFSET,
                pose.angle,
            );
            if let Some(copy) = castle.placed.last_mut() {
                copy.door = piece.door;
            }
            castle
        } else if let Some(wall) = self.walls.iter().find(|piece| piece.id == id) {
            self.with_wall(wall.wall.moved(beside))
        } else {
            return self.clone();
        };
        castle.selected = Some(copy);
        castle
    }
}

// ---------------------------------------------------------------------------------------------
// What the tools do
// ---------------------------------------------------------------------------------------------

/// The space a press on a piece or a wall at `pivot` drags in under `tool`.
pub fn scene_space(tool: Tool, pivot: Vec3) -> DragSpace {
    match tool {
        Tool::Rotate => DragSpace::plane(pivot, Dir3::Y),
        Tool::Raise => DragSpace::axis(pivot, Dir3::Y),
        _ => DragSpace::plane(Vec3::ZERO, Dir3::Y),
    }
}

/// The drag of piece or wall `id` under the tool of `castle`.
pub fn scene_spec(castle: &Castle, id: u32) -> DragSpec<Castle> {
    let snap = castle.precision.snap();
    match castle.tool {
        Tool::Select | Tool::Draw | Tool::Sculpt | Tool::Cutaway | Tool::Scale | Tool::Reshape => {
            select_spec(castle, id)
        }
        Tool::Move if castle.piece(id).is_some() => move_spec(castle, id),
        Tool::Move => DragSpec::from(
            across_plane(move |castle: &Castle, offset: Vec3| {
                castle
                    .moved(id, Vec3::new(offset.x, 0.0, offset.z))
                    .with_selected(Some(id))
            })
            .snap(snap),
        ),
        Tool::Rotate => DragSpec::from(
            around_axis(move |castle: &Castle, angle: f32| {
                castle.turned(id, Quat::from_axis_angle(Vec3::Y, angle))
            })
            .snap(castle.precision.turn()),
        ),
        Tool::Raise => DragSpec::from(
            along_axis(move |castle: &Castle, distance: f32| castle.moved(id, Vec3::Y * distance))
                .snap(snap),
        ),
        Tool::Duplicate => state(castle.duplicated(id)),
        Tool::Delete => state(castle.without(id)),
    }
}

/// The drag of the ground: it places the armed piece, or draws a wall with the Draw tool, or
/// else deselects.
pub fn ground_spec(castle: &Castle) -> DragSpec<Castle> {
    if let Some(kind) = castle.armed {
        place(castle, kind)
    } else if castle.tool == Tool::Draw {
        draw(castle)
    } else if castle.tool == Tool::Sculpt {
        sculpt(castle)
    } else if castle.tool == Tool::Cutaway {
        cutaway(castle)
    } else {
        state(castle.with_selected(None))
    }
}

/// Places a new piece of `kind`: it appears under the pointer and is then dragged like any
/// other piece, aligning to its neighbours. Releasing drops it, selects it and disarms.
fn place(castle: &Castle, kind: PieceKind) -> DragSpec<Castle> {
    let castle = castle.clone();
    custom(move |context| {
        let context = context.clone();
        let castle = castle.clone();
        let mut dragging: Option<DragBehavior<Castle>> = None;
        Box::new(move |pointer: Pointer| {
            let behavior = dragging.get_or_insert_with(|| {
                let point = pointer.point + context.grab_offset();
                let (placed, id) = castle.created(kind, Vec2::new(point.x, point.z));
                let context = context
                    .clone()
                    .with_dragged(piece_id(id))
                    .with_start_state(placed.clone());
                move_spec(&placed, id).compile(&context)
            });
            let mut frame = behavior(pointer);
            frame.drop_state = frame.drop_state.disarmed();
            frame
        })
    })
}

// ---------------------------------------------------------------------------------------------
// The petals
// ---------------------------------------------------------------------------------------------

const ARCH: [(f32, f32); 11] = [
    (-8.0, -10.0),
    (-8.0, 2.0),
    (-5.0, 7.0),
    (0.0, 10.0),
    (5.0, 7.0),
    (8.0, 2.0),
    (8.0, -10.0),
    (4.0, -10.0),
    (4.0, 1.0),
    (0.0, 5.0),
    (-4.0, 1.0),
];

const HOUSE: [(f32, f32); 5] = [
    (-9.0, -8.0),
    (9.0, -8.0),
    (9.0, 1.0),
    (0.0, 10.0),
    (-9.0, 1.0),
];

impl Item {
    /// A small shape that says what the petal is.
    fn shape(self) -> Shape {
        match self {
            Self::Open(Ring::Root) => Shape::compound(vec![
                rectangle(-9.0, 1.0, -1.0, 9.0),
                rectangle(1.0, 1.0, 9.0, 9.0),
                rectangle(-9.0, -9.0, -1.0, -1.0),
                rectangle(1.0, -9.0, 9.0, -1.0),
            ]),
            Self::Open(Ring::Pieces) => outline(&HOUSE),
            Self::Open(Ring::Walls) => Self::Wall(WallStyle::Rampart).shape(),
            Self::Wall(style) => match style {
                WallStyle::Stone => Shape::compound(vec![
                    rectangle(-10.0, -6.0, 10.0, 2.0),
                    rectangle(-10.0, 4.0, -6.0, 8.0),
                    rectangle(-2.0, 4.0, 2.0, 8.0),
                    rectangle(6.0, 4.0, 10.0, 8.0),
                ]),
                WallStyle::Rampart => Shape::compound(vec![
                    rectangle(-10.0, -9.0, 10.0, 3.0),
                    rectangle(-10.0, 5.0, -5.0, 10.0),
                    rectangle(-2.5, 5.0, 2.5, 10.0),
                    rectangle(5.0, 5.0, 10.0, 10.0),
                ]),
                WallStyle::Palisade => Shape::compound(
                    [-7.0, 0.0, 7.0]
                        .map(|x| {
                            ShapePart::new(outline(&[
                                (x - 3.0, -8.0),
                                (x + 3.0, -8.0),
                                (x + 3.0, 4.0),
                                (x, 10.0),
                                (x - 3.0, 4.0),
                            ]))
                        })
                        .to_vec(),
                ),
                WallStyle::Garden => Shape::compound(vec![rectangle(-10.0, -3.0, 10.0, 3.0)]),
            },
            Self::Open(Ring::Tools) => Shape::polygon(6, 10.0, ShapeDetail::Low),
            Self::Open(Ring::Precision) => Shape::compound(vec![
                ShapePart::new(Shape::disc_rim(10.0, 2.0, ShapeDetail::Low)),
                ShapePart::new(Shape::disc(3.0, ShapeDetail::Low)),
            ]),
            Self::Open(Ring::Category(category)) => match category {
                Category::Towers => Self::Piece(PieceKind::Tower).shape(),
                Category::Gates => Self::Piece(PieceKind::ArchGate).shape(),
                Category::Paths => Self::Piece(PieceKind::Cobble).shape(),
                Category::Decor => Self::Piece(PieceKind::Torch).shape(),
            },
            Self::Piece(kind) => match kind {
                PieceKind::Tower => outline(&[
                    (-5.0, -10.0),
                    (5.0, -10.0),
                    (5.0, 3.0),
                    (0.0, 11.0),
                    (-5.0, 3.0),
                ]),
                PieceKind::Keep => outline(&[(-8.0, -8.0), (8.0, -8.0), (8.0, 8.0), (-8.0, 8.0)]),
                PieceKind::ArchGate => outline(&ARCH),
                PieceKind::SquareGate => outline(&[
                    (-8.0, -10.0),
                    (-8.0, 8.0),
                    (8.0, 8.0),
                    (8.0, -10.0),
                    (4.0, -10.0),
                    (4.0, 2.0),
                    (-4.0, 2.0),
                    (-4.0, -10.0),
                ]),
                PieceKind::WoodGate => Shape::compound(vec![
                    rectangle(-9.0, -10.0, -6.0, 8.0),
                    rectangle(6.0, -10.0, 9.0, 8.0),
                    rectangle(-6.0, 3.0, 6.0, 6.0),
                ]),
                PieceKind::Cobble => dots(3.0, &[(-7.0, -4.0), (0.0, 4.0), (7.0, -4.0)]),
                PieceKind::Outpost => Shape::polygon(3, 11.0, ShapeDetail::Low),
                PieceKind::Torch => outline(&[
                    (0.0, 11.0),
                    (5.0, 2.0),
                    (3.0, -3.0),
                    (0.0, -6.0),
                    (-3.0, -3.0),
                    (-5.0, 2.0),
                ]),
                PieceKind::Banner => outline(&[
                    (-8.0, 10.0),
                    (8.0, 10.0),
                    (8.0, -2.0),
                    (0.0, 2.0),
                    (-8.0, -2.0),
                ]),
                PieceKind::Cottage => outline(&HOUSE),
            },
            Self::Tool(tool) => match tool {
                Tool::Select => outline(&[
                    (-6.0, 10.0),
                    (-6.0, -8.0),
                    (-2.0, -4.0),
                    (1.0, -11.0),
                    (4.0, -10.0),
                    (1.0, -3.0),
                    (7.0, -3.0),
                ]),
                Tool::Move => outline(&[
                    (-3.0, 10.0),
                    (-3.0, 3.0),
                    (-10.0, 3.0),
                    (-10.0, -3.0),
                    (-3.0, -3.0),
                    (-3.0, -10.0),
                    (3.0, -10.0),
                    (3.0, -3.0),
                    (10.0, -3.0),
                    (10.0, 3.0),
                    (3.0, 3.0),
                    (3.0, 10.0),
                ]),
                Tool::Rotate => Shape::disc_rim(9.0, 3.0, ShapeDetail::Low),
                Tool::Raise => outline(&[
                    (0.0, 11.0),
                    (8.0, 2.0),
                    (3.0, 2.0),
                    (3.0, -10.0),
                    (-3.0, -10.0),
                    (-3.0, 2.0),
                    (-8.0, 2.0),
                ]),
                Tool::Scale => Shape::compound(vec![
                    rectangle(-10.0, -10.0, -2.0, -2.0),
                    rectangle(0.0, 0.0, 10.0, 10.0),
                ]),
                Tool::Reshape => dots(3.5, &[(-8.0, -6.0), (0.0, 7.0), (8.0, -4.0)]),
                Tool::Draw => outline(&[(-10.0, -8.0), (-7.0, -10.0), (10.0, 6.0), (8.0, 9.0)]),
                Tool::Cutaway => outline(&[
                    (-10.0, -10.0),
                    (-10.0, 10.0),
                    (-2.0, 10.0),
                    (-2.0, -10.0),
                    (2.0, -10.0),
                    (2.0, 10.0),
                    (10.0, 10.0),
                    (10.0, -10.0),
                ]),
                Tool::Sculpt => outline(&[
                    (-11.0, -8.0),
                    (-3.0, 6.0),
                    (1.0, 0.0),
                    (5.0, 9.0),
                    (11.0, -8.0),
                ]),
                Tool::Duplicate => Shape::compound(vec![
                    rectangle(-10.0, -5.0, -1.0, 4.0),
                    rectangle(2.0, -5.0, 11.0, 4.0),
                ]),
                Tool::Delete => outline(&[
                    (-8.0, -10.0),
                    (0.0, -2.0),
                    (8.0, -10.0),
                    (10.0, -8.0),
                    (2.0, 0.0),
                    (10.0, 8.0),
                    (8.0, 10.0),
                    (0.0, 2.0),
                    (-8.0, 10.0),
                    (-10.0, 8.0),
                    (-2.0, 0.0),
                    (-10.0, -8.0),
                ]),
            },
            Self::Precision(precision) => match precision {
                Precision::Free => dots(4.0, &[(0.0, 0.0)]),
                Precision::Fine => dots(2.5, &[(-5.0, -5.0), (5.0, -5.0), (-5.0, 5.0), (5.0, 5.0)]),
                Precision::Coarse => dots(5.0, &[(-6.0, 0.0), (6.0, 0.0)]),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::f32::consts::FRAC_PI_2;
    use std::sync::Arc;

    use bevy_composable_dragging::radial::{CHIP_SCALE, Label, Petal as _, labels, petal_id};
    use bevy_composable_dragging::{DragContext, View, lerp_views};

    use super::*;
    use crate::common::HUD_LAYER;
    use crate::wall::Wall;

    fn castle() -> Castle {
        let mut castle = Castle::empty();
        castle.add(PieceKind::Tower, 0.0, 0.0, 0.0);
        castle.with_wall(Wall::straight((2.0, 0.0), (4.0, 0.0)))
    }

    fn menu_ids(castle: &Castle) -> Vec<String> {
        crate::view(castle)
            .nodes()
            .iter()
            .map(|node| node.id.to_string())
            .filter(|id| id.starts_with("menu/"))
            .collect()
    }

    fn every_state() -> Vec<Castle> {
        let mut states = vec![Castle::empty()];
        states.extend(Ring::all(&Castle::empty()).into_iter().map(|ring| {
            let mut castle = Castle::empty();
            castle.menu = Some(ring);
            castle
        }));
        states
    }

    /// Drags `node` of `castle` by the spec of the node, to `to`, and returns the drop state.
    fn press(castle: &Castle, node: &str, to: Vec3) -> Castle {
        let view = crate::view(castle);
        let node_ref = view.get(node).unwrap();
        let spec = (node_ref.drag.as_ref().unwrap())();
        let context = DragContext::new(
            Arc::new(crate::view),
            node.to_owned().into(),
            castle.clone(),
        )
        .with_space(node_ref.drag_space.unwrap());
        spec.compile(&context)(Pointer::at(to)).drop_state
    }

    /// Presses `node` of `castle` and releases it where it stands, as a click does.
    fn tap(castle: &Castle, node: &str) -> Castle {
        let at = crate::view(castle).world_position(node).unwrap();
        press(castle, node, at)
    }

    #[test]
    fn every_petal_is_present_in_every_state() {
        let all: HashSet<String> = menu_ids(&Castle::empty()).into_iter().collect();
        let petals = Ring::all(&Castle::empty())
            .iter()
            .map(|ring| ring.items(&Castle::empty()).len())
            .sum::<usize>();
        assert!(petals >= 20, "{petals}");
        for state in every_state() {
            let ids: HashSet<String> = menu_ids(&state).into_iter().collect();
            assert_eq!(ids, all, "{:?}", state.menu);
        }
        assert!(all.contains("menu/hub"));
        assert!(all.contains("menu/tools/Draw"));
    }

    #[test]
    fn petal_ids_are_unique() {
        let ids = menu_ids(&Castle::empty());
        assert_eq!(ids.len(), ids.iter().collect::<HashSet<_>>().len());
    }

    #[test]
    fn the_open_ring_fans_out_and_the_others_wait_at_the_hub_without_size() {
        for ring in Ring::all(&Castle::empty()) {
            let mut state = Castle::empty();
            state.menu = Some(ring);
            let view = crate::view(&state);
            let layout = Layout::new(&state.screen);
            let center = state.pop_center(&layout);
            let half = state.screen.hud / 2.0;
            let mut seen = Vec::new();
            for other in Ring::all(&Castle::empty()) {
                for item in other.items(&Castle::empty()) {
                    let id = petal_id::<Castle>(&other, &item);
                    let node = view.get(&id).unwrap();
                    if other == ring {
                        assert!((node.transform.scale.x - 1.0).abs() < 1e-6, "{id}");
                        let at = node.transform.translation.truncate();
                        assert!(
                            at.x.abs() < half.x && at.y.abs() < half.y,
                            "{id} is on screen: {at}"
                        );
                        assert!(
                            at.distance(center) > layout.hub_radius() + layout.petal_radius(),
                            "{id} clears the hub"
                        );
                        assert!(
                            seen.iter().all(
                                |other: &Vec2| other.distance(at) > layout.petal_radius() * 2.0
                            ),
                            "{id} does not overlap a sibling"
                        );
                        seen.push(at);
                    } else {
                        assert!(node.transform.scale.x < 0.01, "{id}");
                        assert!(node.transform.translation.truncate().distance(center) < 1e-3);
                    }
                }
            }
        }
    }

    #[test]
    fn opening_a_ring_blooms_its_petals_out_of_the_hub_when_the_views_blend() {
        let closed = crate::view(&Castle::empty());
        let mut open = Castle::empty();
        open.menu = Some(Ring::Root);
        let open = crate::view(&open);
        let id = "menu/root/pieces";
        let hub = Castle::empty().pop_center(&Layout::new(&Castle::empty().screen));
        let middle = lerp_views(&closed, &open, 0.5);
        let scale = middle.get(id).unwrap().transform.scale.x;
        assert!(scale > 0.2 && scale < 0.8, "{scale}");
        let from_hub = middle
            .get(id)
            .unwrap()
            .transform
            .translation
            .truncate()
            .distance(hub);
        let all_the_way = open
            .get(id)
            .unwrap()
            .transform
            .translation
            .truncate()
            .distance(hub);
        assert!(from_hub > 0.0 && from_hub < all_the_way);
    }

    #[test]
    fn the_menu_is_flat_on_the_hud_layer_and_glyphs_do_not_press() {
        let view = crate::view(&Castle::empty());
        let menu: Vec<_> = view
            .nodes()
            .iter()
            .filter(|node| node.id.starts_with("menu/"))
            .collect();
        assert!(!menu.is_empty());
        for node in &menu {
            assert_eq!(node.layer, HUD_LAYER, "{}", node.id);
            assert_eq!(
                node.rendering,
                Some(bevy_composable_dragging::Rendering::Flat)
            );
            if node.id.ends_with("/glyph")
                || node.id.ends_with("/active")
                || node.id.ends_with("/border")
                || node.id.ends_with("/pill")
            {
                assert!(node.drag.is_none());
                assert!(node.parent.is_some());
            } else {
                let hidden = node.transform.scale.x < 0.01;
                assert_eq!(node.drag.is_none(), hidden, "{}", node.id);
            }
        }
        assert!(
            view.nodes()
                .iter()
                .filter(|node| !node.id.starts_with("menu/"))
                .all(|node| node.layer == 0)
        );
    }

    #[test]
    fn the_hub_opens_the_first_ring_and_closes_it_and_leaves_other_rings() {
        let start = Castle::empty();
        let open = start.hub_pressed();
        assert_eq!(open.menu, Some(Ring::Root));
        assert_eq!(open.hub_pressed().menu, None);
        let mut inside = Castle::empty();
        inside.menu = Some(Ring::Tools);
        assert_eq!(inside.hub_pressed().menu, Some(Ring::Root));
        assert_eq!(tap(&start, "menu/hub").menu, Some(Ring::Root));
    }

    #[test]
    fn a_petal_opens_a_ring_or_makes_its_choice_and_closes_the_menu() {
        let start = Castle::empty();
        assert_eq!(
            Item::Open(Ring::Tools).press(&start).menu,
            Some(Ring::Tools)
        );
        let chosen = Item::Tool(Tool::Rotate).press(&Item::Open(Ring::Tools).press(&start));
        assert_eq!((chosen.tool, chosen.menu), (Tool::Rotate, None));
        let precise = Item::Precision(Precision::Coarse).press(&start);
        assert_eq!(precise.precision, Precision::Coarse);
        let armed = Item::Piece(PieceKind::Keep).press(&start);
        assert_eq!(armed.armed, Some(PieceKind::Keep));
        assert!(Item::Piece(PieceKind::Keep).is_active(&armed));
        assert!(!Item::Piece(PieceKind::Tower).is_active(&armed));
        let mut open = Castle::empty();
        open.menu = Some(Ring::Tools);
        let through_the_engine = tap(&open, "menu/tools/Draw");
        assert_eq!(through_the_engine.tool, Tool::Draw);
    }

    #[test]
    fn every_piece_is_in_exactly_one_category() {
        for kind in Category::ALL.iter().flat_map(|c| c.pieces()) {
            let count = Category::ALL
                .iter()
                .filter(|c| c.pieces().contains(kind))
                .count();
            assert_eq!(count, 1, "{kind:?}");
        }
        let total: usize = Category::ALL.iter().map(|c| c.pieces().len()).sum();
        assert_eq!(total, 10);
    }

    #[test]
    fn precision_sets_the_steps() {
        assert_eq!(Precision::Free.snap(), Snap::NONE);
        assert_eq!(Precision::Free.turn(), Snap::NONE);
        assert!((Precision::Fine.snap().step - 0.1).abs() < 1e-6);
        assert!((Precision::Coarse.snap().step - 0.5).abs() < 1e-6);
        assert!(Precision::Fine.turn().step < Precision::Coarse.turn().step);
    }

    #[test]
    fn the_select_tool_selects_and_the_move_tool_moves() {
        let mut castle = castle();
        let (piece, wall) = (0, 1);
        castle.tool = Tool::Select;
        let selected = press(&castle, &piece_id(wall), Vec3::new(9.0, 0.0, 9.0));
        assert_eq!(selected.selected, Some(wall));
        assert_eq!(
            selected.walls[0].wall, castle.walls[0].wall,
            "selecting does not move"
        );

        castle.tool = Tool::Move;
        let moved = press(&castle, &piece_id(wall), Vec3::new(3.0 + 1.0, 0.0, 1.0));
        assert_eq!(moved.selected, Some(wall));
        assert!(
            moved.walls[0]
                .wall
                .pivot
                .translation
                .distance(castle.walls[0].wall.pivot.translation)
                > 0.0
                || moved.walls[0].wall == castle.walls[0].wall
        );
        let tower = press(&castle, &piece_id(piece), Vec3::new(0.0, 0.0, 0.02));
        assert_eq!(tower.selected, Some(piece));
    }

    #[test]
    fn the_rotate_raise_duplicate_and_delete_tools_change_the_piece_they_press() {
        let mut castle = castle();
        let wall = 1;
        let pivot = castle.walls[0].wall.pivot.translation;

        castle.tool = Tool::Rotate;
        let view = crate::view(&castle);
        let anchor = view.world_position(&piece_id(wall)).unwrap();
        let turned = press(
            &castle,
            &piece_id(wall),
            pivot + Vec3::new(0.0, 0.0, 2.0) + (anchor - pivot) * 0.0,
        );
        assert_ne!(
            turned.walls[0].wall.pivot.rotation,
            castle.walls[0].wall.pivot.rotation
        );

        castle.tool = Tool::Raise;
        let raised = press(&castle, &piece_id(wall), pivot + Vec3::Y * 0.52);
        assert!((raised.walls[0].wall.pivot.translation.y - 0.5).abs() < 1e-3);

        castle.tool = Tool::Duplicate;
        let copied = press(&castle, &piece_id(wall), pivot);
        assert_eq!(copied.walls.len(), 2);
        assert_eq!(copied.selected, Some(copied.next_id - 1));
        assert!(copied.walls[1].wall.pivot.translation.distance(pivot) > 0.1);

        castle.tool = Tool::Delete;
        castle.selected = Some(wall);
        let removed = press(&castle, &piece_id(wall), pivot);
        assert!(removed.walls.is_empty());
        assert_eq!(removed.selected, None);
        let removed_piece = press(&castle, &piece_id(0), Vec3::ZERO);
        assert!(removed_piece.placed.is_empty());
        assert_eq!(removed_piece.walls.len(), 1);
    }

    #[test]
    fn tools_choose_the_drag_space_of_a_press() {
        let pivot = Vec3::new(1.0, 0.5, 2.0);
        assert_eq!(
            scene_space(Tool::Rotate, pivot),
            DragSpace::plane(pivot, Dir3::Y)
        );
        assert_eq!(
            scene_space(Tool::Raise, pivot),
            DragSpace::axis(pivot, Dir3::Y)
        );
        assert_eq!(
            scene_space(Tool::Move, pivot),
            DragSpace::plane(Vec3::ZERO, Dir3::Y)
        );
        assert_eq!(
            scene_space(Tool::Select, pivot),
            DragSpace::plane(Vec3::ZERO, Dir3::Y)
        );
    }

    #[test]
    fn the_rotate_tool_turns_in_the_steps_of_the_precision() {
        let mut castle = castle();
        castle.tool = Tool::Rotate;
        castle.precision = Precision::Coarse;
        let pivot = castle.walls[0].wall.pivot.translation;
        let start = Vec3::new(1.0, 0.0, 0.0);
        let view = crate::view(&castle);
        let node = view.get(&piece_id(1)).unwrap();
        let spec = (node.drag.as_ref().unwrap())();
        let context = DragContext::new(Arc::new(crate::view), piece_id(1).into(), castle.clone())
            .with_space(node.drag_space.unwrap())
            .with_grab_offset(start);
        let mut behavior = spec.compile(&context);
        let angle = 50_f32.to_radians();
        let at = pivot + Vec3::new(angle.cos(), 0.0, -angle.sin()) - start;
        let frame = behavior(Pointer::at(at));
        let yaw = frame.drop_state.walls[0]
            .wall
            .pivot
            .rotation
            .to_euler(EulerRot::YXZ)
            .0;
        assert!((yaw.abs() - FRAC_PI_2 / 2.0).abs() < 1e-3, "{yaw}");
    }

    #[test]
    fn the_ground_places_the_armed_piece_draws_with_the_draw_tool_and_otherwise_deselects() {
        let ground = DragSpace::plane(Vec3::ZERO, Dir3::Y);
        let drag = |castle: &Castle, points: &[Vec3]| -> Castle {
            let context = DragContext::new(Arc::new(crate::view), "ground".into(), castle.clone())
                .with_space(ground);
            let mut behavior = ground_spec(castle).compile(&context);
            let mut last = None;
            for point in points {
                last = Some(behavior(Pointer::at(*point)));
            }
            last.unwrap().drop_state
        };

        let mut base = castle().with_selected(Some(0));
        let deselected = drag(&base, &[Vec3::new(-3.0, 0.0, 3.0)]);
        assert_eq!(deselected.selected, None);

        base.armed = Some(PieceKind::Keep);
        let placed = drag(
            &base,
            &[Vec3::new(-3.0, 0.0, 3.0), Vec3::new(-1.5, 0.0, 3.5)],
        );
        let keep = placed
            .placed
            .iter()
            .find(|p| p.kind == PieceKind::Keep)
            .unwrap();
        assert!(keep.pose().position.distance(Vec2::new(-1.5, 3.5)) < 1e-3);
        assert_eq!(placed.armed, None);
        assert_eq!(placed.selected, Some(keep.id));

        base.armed = None;
        base.tool = Tool::Draw;
        let line: Vec<Vec3> = (0..30)
            .map(|i| Vec3::new(-3.0 + i as f32 * 0.1, 0.0, -3.0))
            .collect();
        let drawn = drag(&base, &line);
        assert_eq!(drawn.walls.len(), 2);
    }

    #[test]
    fn a_piece_armed_in_the_menu_is_shown_and_can_be_disarmed() {
        let mut castle = Castle::empty();
        assert!(crate::view(&castle).get("menu/armed").is_none());
        castle.armed = Some(PieceKind::Tower);
        assert!(crate::view(&castle).get("menu/armed").is_some());
        assert_eq!(tap(&castle, "menu/armed").armed, None);
    }

    #[test]
    fn the_active_choice_is_marked() {
        let castle = Castle::empty();
        let view = crate::view(&castle);
        assert!(view.get("menu/tools/Select/active").is_some());
        assert!(view.get("menu/tools/Move/active").is_none());
        assert!(view.get("menu/precision/Fine/active").is_some());
    }

    fn selecting(id: u32) -> Castle {
        let mut selected = castle();
        selected.selected = Some(id);
        selected
    }

    fn tools_of(castle: &Castle) -> Vec<Tool> {
        Ring::Tools
            .items(castle)
            .into_iter()
            .filter_map(|item| match item {
                Item::Tool(tool) => Some(tool),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_ground_and_a_selection_offer_different_tools() {
        assert_eq!(Castle::empty().context(), Context::Ground);
        assert_eq!(
            tools_of(&Castle::empty()),
            vec![Tool::Select, Tool::Draw, Tool::Sculpt, Tool::Cutaway]
        );
        let piece = selecting(0);
        assert_eq!(piece.context(), Context::Selection);
        assert_eq!(
            tools_of(&piece),
            vec![
                Tool::Select,
                Tool::Move,
                Tool::Rotate,
                Tool::Raise,
                Tool::Duplicate,
                Tool::Delete
            ]
        );
        let wall = selecting(1);
        assert_eq!(wall.context(), Context::Wall);
        assert_eq!(
            tools_of(&wall),
            vec![
                Tool::Select,
                Tool::Move,
                Tool::Rotate,
                Tool::Raise,
                Tool::Scale,
                Tool::Reshape,
                Tool::Duplicate,
                Tool::Delete
            ]
        );
    }

    #[test]
    fn pieces_are_only_offered_for_the_ground_and_the_menu_stays_complete_in_every_state() {
        let root = |castle: &Castle| Ring::Root.items(castle);
        assert!(root(&Castle::empty()).contains(&Item::Open(Ring::Pieces)));
        assert!(!root(&selecting(0)).contains(&Item::Open(Ring::Pieces)));
        let selected = selecting(0);
        let all: HashSet<String> = menu_ids(&selected).into_iter().collect();
        for ring in Ring::all(&Castle::empty()) {
            let mut state = selected.clone();
            state.menu = Some(ring);
            let ids: HashSet<String> = menu_ids(&state).into_iter().collect();
            assert_eq!(ids, all, "{ring:?}");
        }
        assert!(all.contains("menu/tools/Duplicate"));
        assert!(!all.contains("menu/tools/Draw"));
    }

    #[test]
    fn the_petals_of_a_selection_work_through_the_engine() {
        let mut open = selecting(0);
        open.menu = Some(Ring::Tools);
        let duplicated = tap(&open, "menu/tools/Duplicate");
        assert_eq!(duplicated.tool, Tool::Duplicate);
        assert_eq!(duplicated.menu, None);
    }

    fn label<'a>(labels: &'a [Label], key: &str) -> &'a Label {
        labels.iter().find(|label| label.key == key).unwrap()
    }

    #[test]
    fn every_petal_has_a_label_under_it_and_the_keys_are_unique() {
        for mut state in [Castle::empty(), selecting(0), selecting(1)] {
            state.menu = Some(Ring::Tools);
            let view = crate::view(&state);
            let labels = labels(&state, &view);
            let keys: HashSet<&String> = labels.iter().map(|label| &label.key).collect();
            assert_eq!(keys.len(), labels.len());
            for ring in Ring::all(&Castle::empty()) {
                for item in ring.items(&state) {
                    let id = format!("menu/{}/{}", ring.name(), item.name());
                    let petal = view.get(&id).unwrap().transform.translation;
                    let text = label(&labels, &id);
                    assert!(!text.text.is_empty(), "{id}");
                    assert!(text.at.y < petal.y && (text.at.x - petal.x).abs() < 1e-3);
                    assert!(text.at.z > petal.z, "the text is in front");
                    assert_eq!(text.is_shown(), ring == Ring::Tools, "{id}");
                }
            }
        }
    }

    fn scale_of(view: &View<Castle>, id: &str) -> f32 {
        view.get(id).unwrap().transform.scale.x
    }

    #[test]
    fn the_hub_rests_in_its_corner_and_pops_up_over_the_selection_instead() {
        let ground = Castle::empty();
        let layout = Layout::new(&ground.screen);
        let view = crate::view(&ground);
        assert!(scale_of(&view, "menu/hub") > 0.99);
        assert!(scale_of(&view, "menu/pop") < 0.01);
        assert_eq!(ground.pop_center(&layout), layout.home());

        let selected = selecting(0);
        let view = crate::view(&selected);
        assert!(scale_of(&view, "menu/hub") < 0.01, "the corner hub hides");
        assert!(scale_of(&view, "menu/pop") > 0.99, "the hub pops up");
        let anchor = selected.selection_anchor().unwrap();
        let over = selected.screen.project(anchor).unwrap();
        let pop = view
            .get("menu/pop")
            .unwrap()
            .transform
            .translation
            .truncate();
        assert!(pop.distance(over) < 1.0, "{pop} over {over}");
        assert!(pop.distance(layout.home()) > layout.hub_radius());
    }

    #[test]
    fn a_transforming_tool_shrinks_the_hub_to_a_chip_above_the_element_and_shows_the_gizmo() {
        let mut selected = selecting(0);
        let layout = Layout::new(&selected.screen);
        let gizmo = |castle: &Castle| {
            crate::view(castle)
                .nodes()
                .iter()
                .any(|node| node.id.contains("/gizmo/"))
        };
        assert!(!gizmo(&selected), "select reveals no gizmo");
        let over = selected.pop_center(&layout);
        selected.tool = Tool::Move;
        assert!(gizmo(&selected));
        assert!(selected.is_chip());
        let view = crate::view(&selected);
        assert!((scale_of(&view, "menu/pop") - CHIP_SCALE).abs() < 1e-4);
        assert!(selected.pop_center(&layout).y > over.y, "it lifts clear");
        selected.menu = Some(Ring::Root);
        assert!(!selected.is_chip(), "an open menu is full size");
        selected.menu = None;
        for tool in [Tool::Duplicate, Tool::Delete, Tool::Reshape] {
            selected.tool = tool;
            assert!(
                selected.is_chip() && !gizmo(&selected),
                "{tool:?} is pressed on the element"
            );
        }
        selected.tool = Tool::Select;
        assert!(!selected.is_chip() && !gizmo(&selected));
    }

    #[test]
    fn the_ring_of_a_selection_fans_out_around_the_element_and_stays_on_screen() {
        let mut selected = selecting(0);
        selected.menu = Some(Ring::Tools);
        let layout = Layout::new(&selected.screen);
        let center = selected.pop_center(&layout);
        let view = crate::view(&selected);
        let half = selected.screen.hud / 2.0;
        for item in Ring::Tools.items(&selected) {
            let at = view
                .get(&format!("menu/tools/{}", item.name()))
                .unwrap()
                .transform
                .translation
                .truncate();
            assert!(at.x.abs() < half.x && at.y.abs() < half.y, "{at}");
            assert!(at.distance(center) > layout.hub_radius());
        }
    }

    #[test]
    fn choosing_a_wall_style_arms_drawing_with_it_and_walls_take_the_style() {
        let mut castle = Castle::empty();
        castle.menu = Some(Ring::Walls);
        let chosen = Item::Wall(WallStyle::Palisade).press(&castle);
        assert_eq!(chosen.wall_style, WallStyle::Palisade);
        assert_eq!(chosen.tool, Tool::Draw);
        assert_eq!(chosen.menu, None);
        assert!(Item::Wall(WallStyle::Palisade).is_active(&chosen));
        let stroke: Vec<Vec3> = (0..30)
            .map(|i| Vec3::new(-3.0 + i as f32 * 0.1, 0.0, -3.0))
            .collect();
        let drawn = chosen.with_stroke(&stroke);
        assert_eq!(drawn.walls[0].wall.profile, Profile::palisade());
        let names: HashSet<String> = WallStyle::ALL
            .iter()
            .map(|style| Item::Wall(*style).name())
            .collect();
        assert_eq!(names.len(), WallStyle::ALL.len());
        for style in WallStyle::ALL {
            assert!(!style.label().is_empty());
            let glyph = Item::Wall(style).glyph();
            let bounds = glyph.bounds();
            assert!((bounds.max - bounds.min).truncate().min_element() > 1.0);
        }
    }

    #[test]
    fn labels_read_what_the_petal_is_in_the_context() {
        let ground = Castle::empty();
        let selected = selecting(0);
        let wall = selecting(1);
        assert_eq!(Item::Open(Ring::Tools).label(&ground), "Ground");
        assert_eq!(Item::Open(Ring::Tools).label(&selected), "Edit");
        assert_eq!(Item::Tool(Tool::Draw).label(&ground), "Draw wall");
        assert_eq!(Item::Piece(PieceKind::ArchGate).label(&ground), "Arch gate");
        assert_eq!(Item::Precision(Precision::Coarse).label(&ground), "Coarse");
        for ring in Ring::all(&ground) {
            for state in [&ground, &selected, &wall] {
                for item in ring.items(state) {
                    assert!(!item.label(state).is_empty(), "{item:?}");
                }
            }
        }
    }

    #[test]
    fn a_label_blooms_with_its_petal() {
        let closed = crate::view(&Castle::empty());
        let mut opened = Castle::empty();
        opened.menu = Some(Ring::Root);
        let open = crate::view(&opened);
        let middle = lerp_views(&closed, &open, 0.5);
        let key = "menu/root/tools";
        let shut = label(&labels(&Castle::empty(), &closed), key).clone();
        let half = label(&labels(&opened, &middle), key).clone();
        let full = label(&labels(&opened, &open), key).clone();
        assert!(!shut.is_shown() && full.is_shown());
        assert!(half.scale > shut.scale && half.scale < full.scale);
        let hub = Castle::empty().pop_center(&Layout::new(&Castle::empty().screen));
        assert!(half.at.truncate().distance(hub) < full.at.truncate().distance(hub));
    }

    #[test]
    fn the_status_line_names_the_selection_the_tool_and_the_precision() {
        let mut castle = Castle::empty();
        let view = crate::view(&castle);
        let line = |castle: &Castle| label(&labels(castle, &view), "status").text.clone();
        assert_eq!(line(&castle), "Ground\nSelect - Fine");
        castle = selecting(0);
        castle.tool = Tool::Move;
        castle.precision = Precision::Coarse;
        assert_eq!(line(&castle), "Tower\nMove - Coarse");
        assert_eq!(line(&selecting(1)), "Wall\nSelect - Fine");
        castle.armed = Some(PieceKind::Keep);
        assert_eq!(line(&castle), "Placing Keep\nMove - Coarse");
        let all = labels(&castle, &view);
        let status = label(&all, "status");
        assert!(status.left && status.is_shown());
    }
}
