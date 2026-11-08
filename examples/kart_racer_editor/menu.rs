//! The radial menu of the editor: the rings, the petals and what pressing them does.
//!
//! The menu is the library [`radial`](bevy_composable_dragging::radial) module. The hub opens the
//! root ring, which leads to the tools, to the presets of the road at the selected control point
//! and to the precision of the handles. A petal is a tool, a preset or a precision, and pressing
//! it returns the editor after the choice.

use std::f32::consts::FRAC_PI_4;

use bevy::prelude::*;
use bevy_composable_dragging::radial::glyph::{dots, outline, rectangle};
use bevy_composable_dragging::radial::{self, Petal, RadialHost};
use bevy_gradmesh::{Shape, ShapeDetail, ShapePart};

use crate::state::{Editor, Precision, Tool};

/// A ring of petals around the hub.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ring {
    /// Opens the rings of tools, road presets and precision.
    Root,
    /// The tools.
    Tools,
    /// The presets of the road at the selected control point.
    Road,
    /// How finely the handles step.
    Precision,
}

impl radial::Ring<Editor> for Ring {
    type Item = Item;

    fn name(&self) -> String {
        match self {
            Self::Root => "root",
            Self::Tools => "tools",
            Self::Road => "road",
            Self::Precision => "precision",
        }
        .into()
    }

    fn items(&self, editor: &Editor) -> Vec<Item> {
        let selected = editor.selected.is_some();
        match self {
            Self::Root => {
                let mut items = vec![Item::Open(Self::Tools)];
                if selected {
                    items.push(Item::Open(Self::Road));
                }
                items.push(Item::Open(Self::Precision));
                items
            }
            Self::Tools => Tool::ALL
                .into_iter()
                .filter(|tool| *tool != Tool::Drive || editor.testing || editor.error().is_none())
                .map(Item::Tool)
                .collect(),
            Self::Road if selected => Preset::ALL.map(Item::Road).to_vec(),
            Self::Road => Vec::new(),
            Self::Precision => Precision::ALL.map(Item::Precision).to_vec(),
        }
    }

    fn all(_editor: &Editor) -> Vec<Self> {
        vec![Self::Root, Self::Tools, Self::Road, Self::Precision]
    }
}

/// A change of the road at the selected control point that a petal makes in one press.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Preset {
    /// Takes the banking away.
    Level,
    /// Banks the road five degrees further towards the left.
    BankLeft,
    /// Banks the road five degrees further towards the right.
    BankRight,
    /// Makes the road a tenth wider.
    Wider,
    /// Makes the road a tenth narrower.
    Narrower,
    /// Puts the control point on the ground.
    Ground,
}

impl Preset {
    /// Every preset, in the order of the ring.
    pub const ALL: [Self; 6] = [
        Self::Level,
        Self::BankLeft,
        Self::BankRight,
        Self::Wider,
        Self::Narrower,
        Self::Ground,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Level => "Level",
            Self::BankLeft => "Bank left",
            Self::BankRight => "Bank right",
            Self::Wider => "Wider",
            Self::Narrower => "Narrower",
            Self::Ground => "Ground",
        }
    }

    /// The editor after the preset is applied to control point `index`.
    fn apply(self, editor: &Editor, index: usize) -> Editor {
        match self {
            Self::Level => editor.leveled(index),
            Self::BankLeft => editor.banked(index, -1.0),
            Self::BankRight => editor.banked(index, 1.0),
            Self::Wider => editor.broadened(index, 1),
            Self::Narrower => editor.broadened(index, -1),
            Self::Ground => editor.grounded(index),
        }
    }
}

/// A petal.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Item {
    /// Opens a ring.
    Open(Ring),
    /// Chooses a tool.
    Tool(Tool),
    /// Changes the road at the selected control point.
    Road(Preset),
    /// Chooses a precision.
    Precision(Precision),
}

impl Petal<Editor> for Item {
    fn name(&self) -> String {
        match self {
            Self::Open(ring) => radial::Ring::<Editor>::name(ring),
            Self::Tool(tool) => format!("{tool:?}"),
            Self::Road(preset) => format!("{preset:?}"),
            Self::Precision(precision) => format!("{precision:?}"),
        }
    }

    fn label(&self, _editor: &Editor) -> String {
        match self {
            Self::Open(Ring::Root) => "Menu".into(),
            Self::Open(Ring::Tools) => "Tools".into(),
            Self::Open(Ring::Road) => "Road".into(),
            Self::Open(Ring::Precision) => "Precision".into(),
            Self::Tool(tool) => tool.label().into(),
            Self::Road(preset) => preset.label().into(),
            Self::Precision(precision) => format!("{precision:?}"),
        }
    }

    fn accent(&self) -> Color {
        match self {
            Self::Open(Ring::Root) => Color::srgb(0.46, 0.48, 0.56),
            Self::Open(Ring::Tools) | Self::Tool(Tool::Select | Tool::Move | Tool::Raise) => {
                Color::srgb(0.24, 0.42, 0.8)
            }
            Self::Tool(Tool::Width) => Color::srgb(0.2, 0.52, 0.32),
            Self::Tool(Tool::Roll) => Color::srgb(0.36, 0.34, 0.74),
            Self::Tool(Tool::Insert) => Color::srgb(0.74, 0.5, 0.14),
            Self::Tool(Tool::Delete) => Color::srgb(0.62, 0.2, 0.2),
            Self::Tool(Tool::Hills) => Color::srgb(0.36, 0.5, 0.22),
            Self::Tool(Tool::Hole) => Color::srgb(0.4, 0.3, 0.2),
            Self::Tool(Tool::Drive) => Color::srgb(0.7, 0.3, 0.12),
            Self::Open(Ring::Road) | Self::Road(_) => Color::srgb(0.4, 0.4, 0.44),
            Self::Open(Ring::Precision) | Self::Precision(_) => Color::srgb(0.2, 0.46, 0.5),
        }
    }

    fn glyph(&self) -> Shape {
        match self {
            Self::Open(Ring::Root) => radial::glyph::hub(),
            Self::Open(Ring::Tools) => Shape::polygon(6, 10.0, ShapeDetail::Low),
            Self::Open(Ring::Road) => {
                outline(&[(-4.0, 9.0), (4.0, 9.0), (10.0, -9.0), (-10.0, -9.0)])
            }
            Self::Open(Ring::Precision) => Shape::compound(vec![
                ShapePart::new(Shape::disc_rim(10.0, 2.0, ShapeDetail::Low)),
                ShapePart::new(Shape::disc(3.0, ShapeDetail::Low)),
            ]),
            Self::Tool(tool) => tool_glyph(*tool),
            Self::Road(preset) => preset_glyph(*preset),
            Self::Precision(Precision::Free) => dots(2.5, &[(0.0, 0.0)]),
            Self::Precision(Precision::Fine) => dots(2.0, &[(-7.0, 0.0), (0.0, 0.0), (7.0, 0.0)]),
            Self::Precision(Precision::Coarse) => dots(3.5, &[(-6.0, 0.0), (6.0, 0.0)]),
        }
    }

    fn is_active(&self, editor: &Editor) -> bool {
        match self {
            Self::Open(_) | Self::Road(_) => false,
            Self::Tool(Tool::Drive) => editor.testing,
            Self::Tool(tool) => editor.tool == *tool,
            Self::Precision(precision) => editor.precision == *precision,
        }
    }

    fn press(&self, editor: &Editor) -> Editor {
        match self {
            Self::Open(ring) => editor.with_menu(Some(*ring)),
            Self::Tool(Tool::Drive) => {
                let closed = editor.with_menu(None);
                if editor.testing {
                    closed.with_testing(false)
                } else {
                    closed.with_testing(editor.error().is_none())
                }
            }
            Self::Tool(tool) => editor.with_menu(None).with_tool(*tool),
            Self::Road(preset) => {
                let closed = editor.with_menu(None);
                match editor.selected {
                    Some(index) => preset.apply(&closed, index),
                    None => closed,
                }
            }
            Self::Precision(precision) => editor.with_menu(None).with_precision(*precision),
        }
    }
}

/// A bar of `length` and `thickness` turned by `angle` about the middle of the glyph.
fn bar(angle: f32, length: f32, thickness: f32) -> ShapePart {
    let (half_length, half_thickness) = (length / 2.0, thickness / 2.0);
    let turn = Vec2::from_angle(angle);
    let corner = |x: f32, y: f32| {
        let at = turn.rotate(Vec2::new(x, y));
        (at.x, at.y)
    };
    ShapePart::new(outline(&[
        corner(-half_length, -half_thickness),
        corner(half_length, -half_thickness),
        corner(half_length, half_thickness),
        corner(-half_length, half_thickness),
    ]))
}

/// A part of a glyph from points in the plane of the HUD.
fn part(points: &[(f32, f32)]) -> ShapePart {
    ShapePart::new(outline(points))
}

fn tool_glyph(tool: Tool) -> Shape {
    match tool {
        Tool::Select => outline(&[
            (-6.0, 10.0),
            (-6.0, -8.0),
            (-2.0, -4.0),
            (1.0, -10.0),
            (4.0, -9.0),
            (1.0, -3.0),
            (7.0, -3.0),
        ]),
        Tool::Move => Shape::compound(vec![
            rectangle(-10.0, -1.5, 10.0, 1.5),
            rectangle(-1.5, -10.0, 1.5, 10.0),
        ]),
        Tool::Raise => outline(&[
            (0.0, 10.0),
            (8.0, 0.0),
            (3.0, 0.0),
            (3.0, -9.0),
            (-3.0, -9.0),
            (-3.0, 0.0),
            (-8.0, 0.0),
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
        Tool::Roll => Shape::compound(vec![
            ShapePart::new(Shape::disc_rim(10.0, 2.0, ShapeDetail::Low)),
            bar(0.5, 16.0, 3.0),
        ]),
        Tool::Insert => Shape::compound(vec![
            rectangle(-8.0, -1.5, 8.0, 1.5),
            rectangle(-1.5, -8.0, 1.5, 8.0),
        ]),
        Tool::Delete => {
            Shape::compound(vec![bar(FRAC_PI_4, 20.0, 3.5), bar(-FRAC_PI_4, 20.0, 3.5)])
        }
        Tool::Hills => outline(&[
            (-10.0, -8.0),
            (-3.0, 7.0),
            (1.0, 0.0),
            (4.0, 4.0),
            (10.0, -8.0),
        ]),
        Tool::Hole => Shape::disc_rim(9.0, 3.5, ShapeDetail::Low),
        Tool::Drive => outline(&[(-6.0, -9.0), (-6.0, 9.0), (9.0, 0.0)]),
    }
}

fn preset_glyph(preset: Preset) -> Shape {
    match preset {
        Preset::Level => Shape::compound(vec![rectangle(-10.0, -1.5, 10.0, 1.5)]),
        Preset::BankLeft => Shape::compound(vec![bar(0.45, 20.0, 3.0)]),
        Preset::BankRight => Shape::compound(vec![bar(-0.45, 20.0, 3.0)]),
        Preset::Wider => Shape::compound(vec![
            part(&[(-9.0, 0.0), (-4.0, 5.0), (-4.0, -5.0)]),
            part(&[(9.0, 0.0), (4.0, 5.0), (4.0, -5.0)]),
        ]),
        Preset::Narrower => Shape::compound(vec![
            part(&[(-4.0, 0.0), (-9.0, 5.0), (-9.0, -5.0)]),
            part(&[(4.0, 0.0), (9.0, 5.0), (9.0, -5.0)]),
        ]),
        Preset::Ground => Shape::compound(vec![
            rectangle(-10.0, -9.0, 10.0, -6.0),
            part(&[(0.0, -3.0), (6.0, 4.0), (-6.0, 4.0)]),
        ]),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use bevy_composable_dragging::radial::Ring as _;

    use super::*;

    fn selecting(index: usize) -> Editor {
        Editor::upstream().with_selected(Some(index))
    }

    fn names(editor: &Editor, ring: Ring) -> Vec<String> {
        ring.items(editor).iter().map(|item| item.name()).collect()
    }

    #[test]
    fn the_rings_offer_the_tools_the_presets_and_the_precisions() {
        let nothing = Editor::upstream();
        assert_eq!(names(&nothing, Ring::Root), ["tools", "precision"]);
        assert!(names(&nothing, Ring::Road).is_empty());
        let selected = selecting(2);
        assert_eq!(names(&selected, Ring::Root), ["tools", "road", "precision"]);
        assert_eq!(names(&selected, Ring::Tools).len(), Tool::ALL.len());
        assert_eq!(names(&selected, Ring::Road).len(), Preset::ALL.len());
        assert_eq!(
            names(&selected, Ring::Precision),
            ["Free", "Fine", "Coarse"]
        );
        assert_eq!(Ring::all(&nothing).len(), 4);
    }

    #[test]
    fn the_names_are_unique_within_a_ring_and_every_petal_reads_and_draws() {
        let editor = selecting(0);
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
        let mut editor = selecting(1);
        editor.menu = Some(Ring::Tools);
        let chosen = Item::Tool(Tool::Roll).press(&editor);
        assert_eq!(chosen.tool, Tool::Roll);
        assert_eq!(chosen.menu, None);
        assert!(Item::Tool(Tool::Roll).is_active(&chosen));
        assert!(!Item::Tool(Tool::Width).is_active(&chosen));
        assert_eq!(chosen.selected, Some(1));
    }

    #[test]
    fn the_tools_ring_offers_the_hills_and_the_holes_and_pressing_them_chooses_them() {
        let editor = Editor::upstream();
        let tools = names(&editor, Ring::Tools);
        assert!(tools.contains(&"Hills".to_owned()) && tools.contains(&"Hole".to_owned()));
        for tool in [Tool::Hills, Tool::Hole] {
            let chosen = Item::Tool(tool).press(&editor.with_menu(Some(Ring::Tools)));
            assert_eq!(chosen.tool, tool);
            assert_eq!(chosen.menu, None);
            assert!(Item::Tool(tool).is_active(&chosen));
            assert!(!Item::Tool(Tool::Select).is_active(&chosen));
            assert_eq!(
                chosen.ground, editor.ground,
                "choosing a tool shapes nothing"
            );
        }
    }

    #[test]
    fn pressing_the_test_drive_petal_toggles_the_drive() {
        let editor = Editor::upstream();
        let driving = Item::Tool(Tool::Drive).press(&editor);
        assert!(driving.testing);
        assert!(Item::Tool(Tool::Drive).is_active(&driving));
        assert_eq!(driving.tool, editor.tool, "the tool stays");
        assert!(!Item::Tool(Tool::Drive).press(&driving).testing);
    }

    #[test]
    fn pressing_a_precision_chooses_it() {
        let chosen = Item::Precision(Precision::Coarse).press(&Editor::upstream());
        assert_eq!(chosen.precision, Precision::Coarse);
        assert!(Item::Precision(Precision::Coarse).is_active(&chosen));
        assert!(!Item::Precision(Precision::Fine).is_active(&chosen));
    }

    #[test]
    fn pressing_a_ring_opens_it_and_the_hub_goes_back_to_the_root() {
        let opened = Item::Open(Ring::Road).press(&selecting(0));
        assert_eq!(opened.menu, Some(Ring::Road));
        assert_eq!(opened.hub_pressed().menu, Some(Ring::Root));
        assert_eq!(opened.hub_pressed().hub_pressed().menu, None);
    }

    #[test]
    fn the_presets_change_the_road_at_the_selected_point() {
        let editor = selecting(3);
        let point = |editor: &Editor| editor.design.control_points[3];
        let before = point(&editor);
        let wider = Item::Road(Preset::Wider).press(&editor);
        assert!((point(&wider).width - before.width * 1.1).abs() < 1e-5);
        let narrower = Item::Road(Preset::Narrower).press(&wider);
        assert!((point(&narrower).width - before.width).abs() < 1e-5);
        let right = Item::Road(Preset::BankRight).press(&editor);
        assert!(point(&right).roll > 0.0);
        let left = Item::Road(Preset::BankLeft).press(&editor);
        assert!(point(&left).roll < 0.0);
        let level = Item::Road(Preset::Level).press(&right);
        assert_eq!(point(&level).roll, 0.0);
        let grounded = Item::Road(Preset::Ground).press(&editor);
        assert_eq!(point(&grounded).position.y, 0.0);
        assert_eq!(point(&grounded).position.x, before.position.x);
        assert_eq!(wider.menu, None);
    }

    #[test]
    fn a_preset_without_a_selection_changes_nothing() {
        let editor = Editor::upstream();
        let pressed = Item::Road(Preset::Wider).press(&editor);
        assert_eq!(pressed.design, editor.design);
    }

    #[test]
    fn the_drive_petal_waits_for_a_track_that_can_be_driven_but_always_lets_a_drive_end() {
        let drive = Item::Tool(Tool::Drive);
        assert!(!drive.press(&Editor::initial()).testing);
        let driving = drive.press(&Editor::upstream());
        assert!(driving.testing);
        assert!(!drive.press(&driving).testing);
    }
}
