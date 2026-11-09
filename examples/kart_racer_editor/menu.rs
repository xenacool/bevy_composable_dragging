//! The radial menu of the editor: the rings, the petals and what pressing them does.
//!
//! The menu is the library [`radial`](bevy_composable_dragging::radial) module. The hub opens the
//! root ring, whose petal opens the ring of tools. A petal is a ring to open or a tool to
//! choose, and pressing it returns the editor after the choice.

use bevy::prelude::*;
use bevy_composable_dragging::radial::glyph::{outline, rectangle};
use bevy_composable_dragging::radial::{self, Petal, RadialHost};
use bevy_gradmesh::{Shape, ShapeDetail};

use crate::state::{Editor, Tool};

/// A ring of petals around the hub.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ring {
    /// Opens the ring of tools.
    Root,
    /// The tools.
    Tools,
}

impl radial::Ring<Editor> for Ring {
    type Item = Item;

    fn name(&self) -> String {
        match self {
            Self::Root => "root",
            Self::Tools => "tools",
        }
        .into()
    }

    fn items(&self, _editor: &Editor) -> Vec<Item> {
        match self {
            Self::Root => vec![Item::Open(Self::Tools)],
            Self::Tools => Tool::ALL.map(Item::Tool).to_vec(),
        }
    }

    fn all(_editor: &Editor) -> Vec<Self> {
        vec![Self::Root, Self::Tools]
    }
}

/// A petal.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Item {
    /// Opens a ring.
    Open(Ring),
    /// Chooses a tool.
    Tool(Tool),
}

impl Petal<Editor> for Item {
    fn name(&self) -> String {
        match self {
            Self::Open(ring) => radial::Ring::<Editor>::name(ring),
            Self::Tool(tool) => format!("{tool:?}"),
        }
    }

    fn label(&self, _editor: &Editor) -> String {
        match self {
            Self::Open(Ring::Root) => "Menu".into(),
            Self::Open(Ring::Tools) => "Tools".into(),
            Self::Tool(tool) => tool.label().into(),
        }
    }

    fn accent(&self) -> Color {
        match self {
            Self::Open(Ring::Root) => Color::srgb(0.46, 0.48, 0.56),
            Self::Open(Ring::Tools) | Self::Tool(Tool::Move) => Color::srgb(0.24, 0.42, 0.8),
            Self::Tool(Tool::Width) => Color::srgb(0.2, 0.52, 0.32),
            Self::Tool(Tool::Hills) => Color::srgb(0.36, 0.5, 0.22),
        }
    }

    fn glyph(&self) -> Shape {
        match self {
            Self::Open(Ring::Root) => radial::glyph::hub(),
            Self::Open(Ring::Tools) => Shape::polygon(6, 10.0, ShapeDetail::Low),
            Self::Tool(tool) => tool_glyph(*tool),
        }
    }

    fn is_active(&self, editor: &Editor) -> bool {
        match self {
            Self::Open(_) => false,
            Self::Tool(tool) => editor.tool == *tool,
        }
    }

    fn press(&self, editor: &Editor) -> Editor {
        match self {
            Self::Open(ring) => editor.with_menu(Some(*ring)),
            Self::Tool(tool) => editor.with_menu(None).with_tool(*tool),
        }
    }
}

fn tool_glyph(tool: Tool) -> Shape {
    match tool {
        Tool::Move => Shape::compound(vec![
            rectangle(-10.0, -1.5, 10.0, 1.5),
            rectangle(-1.5, -10.0, 1.5, 10.0),
        ]),
        Tool::Width => outline(&[
            (-10.0, 0.0),
            (-4.0, 6.0),
            (-4.0, 2.0),
            (4.0, 2.0),
            (4.0, 6.0),
            (10.0, 0.0),
            (4.0, -6.0),
            (4.0, -2.0),
            (-4.0, -2.0),
            (-4.0, -6.0),
        ]),
        Tool::Hills => outline(&[
            (-10.0, -8.0),
            (-3.0, 7.0),
            (1.0, 0.0),
            (4.0, 4.0),
            (10.0, -8.0),
        ]),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use bevy_composable_dragging::radial::Ring as _;

    use super::*;

    fn names(editor: &Editor, ring: Ring) -> Vec<String> {
        ring.items(editor).iter().map(|item| item.name()).collect()
    }

    #[test]
    fn the_root_ring_opens_the_tools_which_are_all_there_is_to_choose() {
        let editor = Editor::initial();
        assert_eq!(names(&editor, Ring::Root), ["tools"]);
        assert_eq!(names(&editor, Ring::Tools), ["Move", "Width", "Hills"]);
        assert_eq!(Ring::all(&editor).len(), 2);
    }

    #[test]
    fn the_names_are_unique_within_a_ring_and_every_petal_reads_and_draws() {
        let editor = Editor::initial();
        for ring in Ring::all(&editor) {
            let items = ring.items(&editor);
            let unique: HashSet<String> = items.iter().map(Petal::name).collect();
            assert_eq!(unique.len(), items.len(), "{ring:?}");
            for item in items {
                assert!(!item.label(&editor).is_empty(), "{item:?}");
                let bounds = item.glyph().bounds();
                let size = (bounds.max - bounds.min).truncate();
                assert!(size.min_element() > 0.5, "{item:?} has a glyph: {size}");
            }
        }
    }

    #[test]
    fn pressing_a_tool_chooses_it_and_closes_the_menu() {
        let editor = Editor::initial()
            .with_selected(Some(1))
            .with_menu(Some(Ring::Tools));
        let chosen = Item::Tool(Tool::Width).press(&editor);
        assert_eq!(chosen.tool, Tool::Width);
        assert_eq!(chosen.menu, None);
        assert!(Item::Tool(Tool::Width).is_active(&chosen));
        assert!(!Item::Tool(Tool::Move).is_active(&chosen));
        assert_eq!(chosen.selected, Some(1));
        assert_eq!(
            chosen.ground, editor.ground,
            "choosing a tool shapes nothing"
        );
    }

    #[test]
    fn pressing_a_ring_opens_it_and_the_hub_goes_back_to_the_root() {
        let opened = Item::Open(Ring::Tools).press(&Editor::initial());
        assert_eq!(opened.menu, Some(Ring::Tools));
        assert_eq!(opened.hub_pressed().menu, Some(Ring::Root));
        assert_eq!(opened.hub_pressed().hub_pressed().menu, None);
    }
}
