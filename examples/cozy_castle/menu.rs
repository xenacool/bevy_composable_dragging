//! What the radial menu of the castle holds.
//!
//! The menu is the library [`radial`] menu on the [`hud`](bevy_composable_dragging::hud): this
//! module only says what the castle puts in it. [`Ring`] and [`Item`] implement the traits of the
//! library for the rings and the petals, and [`Castle`] implements them for the state, which
//! says where the menu pops up and what the status panel says.
//!
//! The hub rests as a small button in a corner and places pieces. Selecting a piece or a wall
//! pops the hub up over it, and with a wall selected it also offers the tools that edit it.
//! Choosing the Scale tool shrinks the hub to a chip above the wall and reveals the gizmo, so
//! the hub never covers what it edits.
//!
//! The engine has press, drag and release, so the menu works in modes. Pressing the hub or a
//! petal and releasing it commits a fixed state: a ring opens, a tool is chosen, or a piece is
//! armed. An armed piece is placed by the next drag across the ground.

use bevy::prelude::*;
use bevy_composable_dragging::hud::{HudHost, Layout, Screen};
use bevy_composable_dragging::ops::{custom, state};
use bevy_composable_dragging::radial::glyph::{outline, rectangle};
use bevy_composable_dragging::radial::{self, RadialHost, Ring as _};
use bevy_composable_dragging::{DragBehavior, DragSpec, Pointer};
use bevy_gradmesh::{Shape, ShapeDetail};

use crate::{Castle, PieceKind, move_spec, piece_id};

/// What a press on a wall does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    /// Pressing selects.
    Select,
    /// The handles of the gizmo scale the selected wall.
    Scale,
}

impl Tool {
    fn label(self) -> &'static str {
        match self {
            Self::Select => "Select",
            Self::Scale => "Scale",
        }
    }
}

/// A ring of petals around the hub.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ring {
    /// The pieces to place, and the tools when a wall is selected.
    Root,
    /// The tools that edit the selected wall.
    Tools,
}

impl radial::Ring<Castle> for Ring {
    type Item = Item;

    fn name(&self) -> String {
        match *self {
            Self::Root => "root".into(),
            Self::Tools => "tools".into(),
        }
    }

    fn items(&self, castle: &Castle) -> Vec<Item> {
        match *self {
            Self::Root => {
                let mut items: Vec<Item> = PieceKind::ALL.map(Item::Piece).to_vec();
                if castle.selected.is_some_and(|id| castle.wall(id).is_some()) {
                    items.push(Item::Open(Self::Tools));
                }
                items
            }
            Self::Tools => vec![Item::Tool(Tool::Select), Item::Tool(Tool::Scale)],
        }
    }

    /// Every ring, which are all in every view.
    fn all(_castle: &Castle) -> Vec<Self> {
        vec![Self::Root, Self::Tools]
    }
}

/// A petal.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Item {
    /// Opens a ring.
    Open(Ring),
    /// Arms a piece to be placed.
    Piece(PieceKind),
    /// Chooses a tool.
    Tool(Tool),
}

impl radial::Petal<Castle> for Item {
    fn name(&self) -> String {
        match *self {
            Self::Open(ring) => ring.name(),
            Self::Piece(kind) => format!("{kind:?}"),
            Self::Tool(tool) => format!("{tool:?}"),
        }
    }

    fn label(&self, _castle: &Castle) -> String {
        match *self {
            Self::Open(_) => "Edit".into(),
            Self::Piece(kind) => kind.label().into(),
            Self::Tool(tool) => tool.label().into(),
        }
    }

    /// The color of the petal, dark enough for its white glyph to read on it.
    fn accent(&self) -> Color {
        match *self {
            Self::Open(_) | Self::Tool(_) => Color::srgb(0.24, 0.42, 0.8),
            Self::Piece(kind) => kind.color().mix(&Color::BLACK, 0.38),
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
            Self::Tool(tool) => castle.tool == tool,
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
            Self::Tool(tool) => castle.tool = tool,
        }
        castle.menu = None;
        castle
    }
}

impl PieceKind {
    /// The pieces in the order of the menu.
    pub const ALL: [Self; 3] = [Self::Tower, Self::Keep, Self::ArchGate];

    fn label(self) -> &'static str {
        match self {
            Self::Tower => "Tower",
            Self::Keep => "Keep",
            Self::ArchGate => "Arch gate",
        }
    }

    fn color(self) -> Color {
        match self {
            Self::Tower | Self::ArchGate => Color::srgb(0.62, 0.62, 0.66),
            Self::Keep => Color::srgb(0.85, 0.8, 0.7),
        }
    }
}

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

    /// The hub has shrunk to a chip above the wall, which the Scale tool does once the menu is
    /// closed.
    fn is_chip(&self) -> bool {
        self.scaling() && self.menu.is_none()
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

    /// What is selected or being placed, and the tool.
    fn status(&self) -> String {
        let first = match self.armed {
            Some(kind) => format!("Placing {}", kind.label()),
            None => self.selection_name(),
        };
        format!("{first}\n{}", self.tool.label())
    }
}

impl Castle {
    /// The point of the scene the menu pops up over: the middle of the selected piece or wall.
    fn selection_anchor(&self) -> Option<Vec3> {
        let id = self.selected?;
        if let Some(piece) = self.piece(id) {
            return Some(piece.position + Vec3::Y * piece.kind.top() / 2.0);
        }
        let wall = &self.wall(id)?;
        Some(wall.pivot + Vec3::Y * wall.profile().top() / 2.0)
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
        castle.add(kind, position.x, position.y);
        (castle, id)
    }
}

/// The drag of the ground: it places the armed piece, or else deselects.
pub fn ground_spec(castle: &Castle) -> DragSpec<Castle> {
    if let Some(kind) = castle.armed {
        place(castle, kind)
    } else {
        state(castle.with_selected(None))
    }
}

/// Places a new piece of `kind`: it appears under the pointer and is then dragged like any
/// other piece. Releasing drops it, selects it and disarms.
///
/// The engine gives a drag the pointer minus the offset at which the dragged node was grabbed, so
/// a node keeps its distance from the pointer. A new piece has no such distance: it is created
/// under the pointer and carried by the pointer itself.
fn place(castle: &Castle, kind: PieceKind) -> DragSpec<Castle> {
    let castle = castle.clone();
    custom(move |context| {
        let context = context.clone();
        let castle = castle.clone();
        let mut dragging: Option<DragBehavior<Castle>> = None;
        Box::new(move |pointer: Pointer| {
            let pointer = pointer.moved_to(pointer.point + context.grab_offset());
            let behavior = dragging.get_or_insert_with(|| {
                let (placed, id) =
                    castle.created(kind, Vec2::new(pointer.point.x, pointer.point.z));
                let context = context
                    .clone()
                    .with_dragged(piece_id(id))
                    .with_start_state(placed.clone());
                move_spec(id).compile(&context)
            });
            let mut frame = behavior(pointer);
            frame.drop_state = frame.drop_state.disarmed();
            frame
        })
    })
}

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

impl Item {
    /// A small shape that says what the petal is.
    fn shape(self) -> Shape {
        match self {
            Self::Open(_) => Shape::polygon(6, 10.0, ShapeDetail::Low),
            Self::Piece(PieceKind::Tower) => outline(&[
                (-5.0, -10.0),
                (5.0, -10.0),
                (5.0, 3.0),
                (0.0, 11.0),
                (-5.0, 3.0),
            ]),
            Self::Piece(PieceKind::Keep) => {
                outline(&[(-8.0, -8.0), (8.0, -8.0), (8.0, 8.0), (-8.0, 8.0)])
            }
            Self::Piece(PieceKind::ArchGate) => outline(&ARCH),
            Self::Tool(Tool::Select) => outline(&[
                (-6.0, 10.0),
                (-6.0, -8.0),
                (-2.0, -4.0),
                (1.0, -11.0),
                (4.0, -10.0),
                (1.0, -3.0),
                (7.0, -3.0),
            ]),
            Self::Tool(Tool::Scale) => Shape::compound(vec![
                rectangle(-10.0, -10.0, -2.0, -2.0),
                rectangle(0.0, 0.0, 10.0, 10.0),
            ]),
        }
    }
}
