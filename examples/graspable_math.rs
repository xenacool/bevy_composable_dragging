//! Rearranging an equation by direct manipulation, after Graspable Math: drag a term across the
//! equals sign and its sign flips, drag a factor across and it divides the other side, drop a
//! term on a like term to combine them.
//!
//! The equation is a `shared-graph`: equal subexpressions are one node, so two terms are like
//! terms exactly when their ids are equal, and a rewrite that arrives at something that exists
//! already adds nothing. Every term and factor is one node of the view with a transparent box
//! over its glyphs, a factor's box inside its term's, so a press on a sign grabs the term and a
//! press on a number grabs the factor. A drag is `closest` of the equations it can lead to,
//! and where it is far from all of them the equation stays and the grabbed part follows the
//! pointer.

use bevy::prelude::*;
use bevy::text::FontSize;
use bevy_composable_dragging::graph::{Graph, Id, Node};
use bevy_composable_dragging::ops::{closest, drop_target, state};
use bevy_composable_dragging::{DragSpec, DraggablePlugin, NodeId, Rendering, View, ViewNode};
use bevy_gradmesh::{Shape, ShapeDetail};
use std::collections::HashMap;

mod common;
use common::screenshot::AutoScreenshotPlugin;
use common::script::ScriptedDragPlugin;

/// How far the pointer may be from every equation a drag can lead to before the part follows it.
const FAR: f32 = 1.5;
/// The height of a glyph.
const EM: f32 = 0.9;
const ADVANCE: f32 = 0.55 * EM;
/// The room of a sign between two terms, and of a minus before the first.
const SIGN: f32 = 1.2 * EM;
const MINUS: f32 = 0.7 * EM;
const PAREN: f32 = 0.4 * EM;
const ASCENT: f32 = 0.72 * EM;
const DESCENT: f32 = 0.25 * EM;
/// The height of the fraction bar above the baseline, and the gap around it.
const AXIS: f32 = 0.3 * EM;
const GAP: f32 = 0.12 * EM;
const PAD: f32 = 0.12;
/// How much nearer the camera a node is than the node it is in, so inner boxes win a press.
const STEP: f32 = 0.1;

#[derive(Clone, PartialEq, Eq, Hash)]
enum Expr {
    Num(i64),
    Var(char),
    Neg(Id),
    Sum(Vec<Id>),
    Mul(Vec<Id>),
    Frac(Id, Id),
}

impl Node for Expr {
    fn children(&self) -> Vec<Id> {
        match self {
            Expr::Num(_) | Expr::Var(_) => vec![],
            Expr::Neg(x) => vec![*x],
            Expr::Sum(parts) | Expr::Mul(parts) => parts.clone(),
            Expr::Frac(n, d) => vec![*n, *d],
        }
    }

    fn with_children(&self, children: &[Id]) -> Self {
        match self {
            Expr::Num(_) | Expr::Var(_) => self.clone(),
            Expr::Neg(_) => Expr::Neg(children[0]),
            Expr::Sum(_) => Expr::Sum(children.to_vec()),
            Expr::Mul(_) => Expr::Mul(children.to_vec()),
            Expr::Frac(..) => Expr::Frac(children[0], children[1]),
        }
    }
}

/// The two sides of an equation, in one graph.
#[derive(Clone)]
struct Equation {
    graph: Graph<Expr>,
    sides: [Id; 2],
}

/// Keeps an expression in normal form: no sum in a sum, no product in a product, no zero among
/// terms, no one among factors, no double minus and no fraction of a fraction.
fn normalize(graph: &mut Graph<Expr>, node: Expr) -> Expr {
    let get = |graph: &Graph<Expr>, id| graph.node(id).clone();
    match node {
        Expr::Sum(terms) => {
            let mut flat = Vec::new();
            for term in terms {
                match get(graph, term) {
                    Expr::Sum(inner) => flat.extend(inner),
                    Expr::Neg(x) => match get(graph, x) {
                        Expr::Sum(inner) => {
                            flat.extend(inner.into_iter().map(|u| negate(graph, u)));
                        }
                        _ => flat.push(term),
                    },
                    _ => flat.push(term),
                }
            }
            flat.retain(|term| *graph.node(*term) != Expr::Num(0));
            match flat[..] {
                [] => Expr::Num(0),
                [only] => get(graph, only),
                _ => Expr::Sum(flat),
            }
        }
        Expr::Mul(factors) => {
            let mut flat = Vec::new();
            for factor in factors {
                match get(graph, factor) {
                    Expr::Mul(inner) => flat.extend(inner),
                    _ => flat.push(factor),
                }
            }
            flat.retain(|factor| *graph.node(*factor) != Expr::Num(1));
            match flat[..] {
                [] => Expr::Num(1),
                [only] => get(graph, only),
                _ => Expr::Mul(flat),
            }
        }
        Expr::Neg(x) => match get(graph, x) {
            Expr::Neg(y) => get(graph, y),
            Expr::Num(0) => Expr::Num(0),
            _ => Expr::Neg(x),
        },
        Expr::Frac(n, d) => match (get(graph, n), get(graph, d)) {
            (_, Expr::Num(1)) => get(graph, n),
            (Expr::Frac(a, b), _) => {
                let d = graph.intern(Expr::Mul(vec![b, d]));
                Expr::Frac(a, d)
            }
            _ => Expr::Frac(n, d),
        },
        other => other,
    }
}

/// Works out a division of whole numbers that has no rest. It is not part of the normal form, so
/// that while a factor is dragged across the division is still there, with the factor in it.
fn evaluate(graph: &mut Graph<Expr>, node: Expr) -> Expr {
    if let Expr::Frac(n, d) = node
        && let (Expr::Num(a), Expr::Num(b)) = (graph.node(n), graph.node(d))
        && *b != 0
        && a % b == 0
    {
        return Expr::Num(a / b);
    }
    node
}

/// `term` with its sign flipped.
fn negate(graph: &mut Graph<Expr>, term: Id) -> Id {
    match *graph.node(term) {
        Expr::Neg(x) => x,
        _ => graph.intern(Expr::Neg(term)),
    }
}

/// The part of a term that stays when its sign flips.
fn body(graph: &Graph<Expr>, term: Id) -> Id {
    match graph.node(term) {
        Expr::Neg(x) => *x,
        _ => term,
    }
}

/// A term as a signed number times what it is a number of: `-2x` is `(-2, [x])`.
fn coefficient(graph: &Graph<Expr>, term: Id) -> (i64, Vec<Id>) {
    match graph.node(term) {
        Expr::Neg(x) => {
            let (number, rest) = coefficient(graph, *x);
            (-number, rest)
        }
        Expr::Num(n) => (*n, vec![]),
        Expr::Mul(factors) => match factors.split_first() {
            Some((&first, rest)) if matches!(graph.node(first), Expr::Num(_)) => {
                let Expr::Num(n) = graph.node(first) else {
                    unreachable!("matched a number")
                };
                (*n, rest.to_vec())
            }
            _ => (1, factors.clone()),
        },
        _ => (1, vec![term]),
    }
}

/// The term with the signed `number` times `rest`, or `None` when they cancel.
fn from_coefficient(graph: &mut Graph<Expr>, number: i64, mut rest: Vec<Id>) -> Option<Id> {
    if number == 0 {
        return None;
    }
    if number.abs() != 1 || rest.is_empty() {
        rest.insert(0, graph.intern(Expr::Num(number.abs())));
    }
    let product = graph.intern(Expr::Mul(rest));
    Some(if number < 0 {
        graph.intern(Expr::Neg(product))
    } else {
        product
    })
}

/// `a` and `b` added, if they are like terms: the same thing in a different amount. Equal things
/// are equal ids, so this is a comparison of ids.
fn combined(graph: &Graph<Expr>, a: Id, b: Id) -> Option<(i64, Vec<Id>)> {
    let ((x, rest), (y, other)) = (coefficient(graph, a), coefficient(graph, b));
    (rest == other).then_some((x + y, rest))
}

/// `items` with the item `at` removed.
fn without(items: &[Id], at: usize) -> Vec<Id> {
    let mut items = items.to_vec();
    items.remove(at);
    items
}

/// `items` with `item` inserted at `at`.
fn inserted(items: &[Id], at: usize, item: Id) -> Vec<Id> {
    let mut items = items.to_vec();
    items.insert(at, item);
    items
}

impl Equation {
    /// The equation after `change`, in normal form.
    fn edited(&self, change: impl FnOnce(&mut Graph<Expr>, &mut [Id; 2])) -> Self {
        let mut next = self.clone();
        change(&mut next.graph, &mut next.sides);
        for side in &mut next.sides {
            *side = next.graph.rewrite(*side, &mut normalize);
        }
        next
    }

    /// The equation once a drag is over: divisions that come out even are worked out, and only
    /// what the sides reach is kept, so that a long session does not pile up what is gone.
    fn settled(&self) -> Self {
        let mut graph = self.graph.clone();
        let sides = self.sides.map(|side| graph.rewrite(side, &mut evaluate));
        let (graph, roots) = graph.compact(&sides);
        Self {
            graph,
            sides: [roots[0], roots[1]],
        }
    }

    /// The terms of a side: its sum, or the side itself.
    fn terms(&self, side: usize) -> Vec<Id> {
        match self.graph.node(self.sides[side]) {
            Expr::Sum(terms) => terms.clone(),
            _ => vec![self.sides[side]],
        }
    }

    /// The factors of a side that can move across: a product's, or what a fraction divides by.
    fn factors(&self, side: usize) -> Vec<Id> {
        match self.graph.node(self.sides[side]) {
            Expr::Mul(factors) => factors.clone(),
            Expr::Frac(_, divisor) => vec![*divisor],
            _ => vec![],
        }
    }

    /// The names of the terms and of the factors, by side. A name is the node of the thing that
    /// stays when the term moves, so a term keeps its name when it flips its sign across the
    /// equals sign, and the same thing twice is told apart by how many came before it.
    fn names(&self) -> [[Vec<String>; 2]; 2] {
        let name = |kind: &str, parts: [Vec<Id>; 2]| {
            let mut seen: HashMap<Id, usize> = HashMap::new();
            parts.map(|ids| {
                ids.into_iter()
                    .map(|id| {
                        let before = seen.entry(id).or_default();
                        *before += 1;
                        format!("{kind}-{id}-{}", *before - 1)
                    })
                    .collect()
            })
        };
        [
            name(
                "term",
                [0, 1].map(|side| {
                    let terms = self.terms(side);
                    terms.iter().map(|&t| body(&self.graph, t)).collect()
                }),
            ),
            name("factor", [0, 1].map(|side| self.factors(side))),
        ]
    }
}

/// The equations that dragging term `index` of `side` can lead to without combining it: put in
/// another place of its side, or moved across the equals sign with its sign flipped.
fn term_moves(eq: &Equation, side: usize, index: usize) -> Vec<Equation> {
    let (terms, other) = (eq.terms(side), 1 - side);
    let (term, rest, there) = (terms[index], without(&terms, index), eq.terms(other));
    let reordered = (0..=rest.len()).map(|at| {
        eq.edited(|graph, sides| {
            sides[side] = graph.intern(Expr::Sum(inserted(&rest, at, term)));
        })
    });
    let across = (0..=there.len()).map(|at| {
        eq.edited(|graph, sides| {
            let flipped = negate(graph, term);
            sides[side] = graph.intern(Expr::Sum(rest.clone()));
            sides[other] = graph.intern(Expr::Sum(inserted(&there, at, flipped)));
        })
    });
    reordered.chain(across).collect()
}

/// The equations that dropping term `index` of `side` on a like term of its side leads to, with
/// the index of the term it is dropped on.
fn term_merges(eq: &Equation, side: usize, index: usize) -> Vec<(usize, Equation)> {
    let terms = eq.terms(side);
    (0..terms.len())
        .filter(|&at| at != index)
        .filter_map(|at| {
            let (number, rest) = combined(&eq.graph, terms[index], terms[at])?;
            let merged = eq.edited(|graph, sides| {
                let mut kept = terms.clone();
                match from_coefficient(graph, number, rest) {
                    Some(sum) => kept[at] = sum,
                    None => {
                        kept.remove(at);
                    }
                }
                sides[side] = graph.intern(Expr::Sum(without(&kept, index)));
            });
            Some((at, merged))
        })
        .collect()
}

/// The equations that dragging factor `index` of `side` can lead to without combining it: put in
/// another place of a product, or moved across the equals sign, where a factor divides the other
/// side and what a fraction divides by multiplies it. Nothing divides by zero.
fn factor_moves(eq: &Equation, side: usize, index: usize) -> Vec<Equation> {
    let (factors, other) = (eq.factors(side), 1 - side);
    let factor = factors[index];
    match eq.graph.node(eq.sides[side]).clone() {
        Expr::Mul(_) => {
            let rest = without(&factors, index);
            let reordered = (0..=rest.len()).map(|at| {
                eq.edited(|graph, sides| {
                    sides[side] = graph.intern(Expr::Mul(inserted(&rest, at, factor)));
                })
            });
            let across = (*eq.graph.node(factor) != Expr::Num(0)).then(|| {
                eq.edited(|graph, sides| {
                    sides[other] = graph.intern(Expr::Frac(sides[other], factor));
                    sides[side] = graph.intern(Expr::Mul(rest.clone()));
                })
            });
            reordered.chain(across).collect()
        }
        Expr::Frac(numerator, divisor) => vec![eq.edited(|graph, sides| {
            sides[other] = graph.intern(Expr::Mul(vec![sides[other], divisor]));
            sides[side] = numerator;
        })],
        _ => vec![],
    }
}

/// The equations that dropping factor `index` of `side` on another number of its product leads to
/// when it is a number too, with the index of the factor it is dropped on.
fn factor_merges(eq: &Equation, side: usize, index: usize) -> Vec<(usize, Equation)> {
    let factors = eq.factors(side);
    let Expr::Num(a) = *eq.graph.node(factors[index]) else {
        return vec![];
    };
    (0..factors.len())
        .filter(|&at| at != index)
        .filter_map(|at| {
            let Expr::Num(b) = *eq.graph.node(factors[at]) else {
                return None;
            };
            let merged = eq.edited(|graph, sides| {
                let mut kept = factors.clone();
                kept[at] = graph.intern(Expr::Num(a * b));
                sides[side] = graph.intern(Expr::Mul(without(&kept, index)));
            });
            Some((at, merged))
        })
        .collect()
}

/// A drag of a part: it snaps to the nearest of the equations it can lead to, is dropped on a
/// like part to combine with it, and stays where it is far from all of them while the part
/// follows the pointer.
fn part_spec(
    eq: &Equation,
    moves: Vec<Equation>,
    merges: Vec<(String, Equation)>,
) -> DragSpec<Equation> {
    let targets = moves.into_iter().map(state).chain(
        merges
            .into_iter()
            .map(|(on, merged)| drop_target(format!("box-{on}"), merged)),
    );
    closest(targets)
        .when_far(state(eq.clone()), FAR)
        .with_floating()
        .on_drop_with(Equation::settled)
}

fn term_spec(eq: &Equation, side: usize, index: usize) -> DragSpec<Equation> {
    let names = &eq.names()[0][side];
    let merges = term_merges(eq, side, index)
        .into_iter()
        .map(|(at, merged)| (names[at].clone(), merged))
        .collect();
    part_spec(eq, term_moves(eq, side, index), merges)
}

fn factor_spec(eq: &Equation, side: usize, index: usize) -> DragSpec<Equation> {
    let names = &eq.names()[1][side];
    let merges = factor_merges(eq, side, index)
        .into_iter()
        .map(|(at, merged)| (names[at].clone(), merged))
        .collect();
    part_spec(eq, factor_moves(eq, side, index), merges)
}

// ---------------------------------------------------------------------------------------------
// View
// ---------------------------------------------------------------------------------------------

/// How big an expression is: its width and how far it reaches above and below its baseline.
#[derive(Clone, Copy)]
struct Size {
    w: f32,
    asc: f32,
    desc: f32,
}

impl Size {
    fn text(chars: usize) -> Self {
        Self {
            w: chars as f32 * ADVANCE,
            asc: ASCENT,
            desc: DESCENT,
        }
    }

    fn beside(parts: impl IntoIterator<Item = Size>) -> Self {
        parts.into_iter().fold(Self::text(0), |all, part| Self {
            w: all.w + part.w,
            asc: all.asc.max(part.asc),
            desc: all.desc.max(part.desc),
        })
    }
}

/// The sign written before a term, and the room it takes.
fn lead(graph: &Graph<Expr>, term: Id, first: bool) -> (&'static str, f32) {
    match (matches!(graph.node(term), Expr::Neg(_)), first) {
        (false, true) => ("", 0.0),
        (false, false) => ("+", SIGN),
        (true, true) => ("-", MINUS),
        (true, false) => ("-", SIGN),
    }
}

/// Whether a factor is written in parentheses.
fn parenthesized(graph: &Graph<Expr>, factor: Id) -> bool {
    matches!(graph.node(factor), Expr::Sum(_) | Expr::Neg(_))
}

fn term_size(graph: &Graph<Expr>, term: Id, first: bool) -> Size {
    let inner = measure(graph, body(graph, term));
    Size {
        w: lead(graph, term, first).1 + inner.w,
        ..inner
    }
}

fn factor_size(graph: &Graph<Expr>, factor: Id) -> Size {
    let inner = measure(graph, factor);
    let w = inner.w
        + if parenthesized(graph, factor) {
            2.0 * PAREN
        } else {
            0.0
        };
    Size { w, ..inner }
}

fn measure(graph: &Graph<Expr>, id: Id) -> Size {
    match graph.node(id) {
        Expr::Num(n) => Size::text(n.to_string().len()),
        Expr::Var(_) => Size::text(1),
        Expr::Neg(x) => Size::beside([Size::text(1), measure(graph, *x)]),
        Expr::Sum(terms) => Size::beside(
            terms
                .iter()
                .enumerate()
                .map(|(i, &t)| term_size(graph, t, i == 0)),
        ),
        Expr::Mul(factors) => Size::beside(factors.iter().map(|&f| factor_size(graph, f))),
        Expr::Frac(n, d) => {
            let (n, d) = (measure(graph, *n), measure(graph, *d));
            Size {
                w: n.w.max(d.w) + 2.0 * GAP,
                asc: (AXIS + GAP + n.desc + n.asc).max(ASCENT),
                desc: (d.asc + d.desc + GAP - AXIS).max(DESCENT),
            }
        }
    }
}

/// Where the nodes being laid out hang: the node they are children of, and where it is.
#[derive(Clone)]
struct Spot {
    parent: Option<NodeId>,
    origin: Vec2,
}

impl Spot {
    /// `node` as a child of what the spot hangs from.
    fn hang(&self, node: ViewNode<Equation>) -> ViewNode<Equation> {
        match &self.parent {
            Some(parent) => node.with_parent(parent.clone()),
            None => node,
        }
    }
}

struct Pen<'a> {
    eq: &'a Equation,
    out: Vec<ViewNode<Equation>>,
    tile: Shape,
    /// How many glyphs and bars each part has so far.
    drawn: HashMap<Option<NodeId>, usize>,
}

impl Pen<'_> {
    /// A name for the next glyph or bar in a part, which stays the same while the layout around
    /// the part changes, so that a glyph is not made again for every state of a drag.
    fn drawn(&mut self, spot: &Spot, what: &str) -> String {
        let count = self.drawn.entry(spot.parent.clone()).or_default();
        *count += 1;
        let part = spot.parent.as_deref().unwrap_or("equation");
        format!("{part}/{}-{what}", *count - 1)
    }

    fn text(&mut self, spot: &Spot, text: &str, left: f32, base: f32) {
        let width = text.chars().count() as f32 * ADVANCE;
        let at = Vec2::new(left + width / 2.0, base + 0.3 * EM) - spot.origin;
        let content = text.to_owned();
        let name = self.drawn(spot, text);
        let glyph = ViewNode::group(name)
            .at(at.extend(STEP / 2.0))
            .scaled(Vec3::splat(EM / 100.0))
            .with_scene(move || {
                let content = content.clone();
                bevy::scene::bsn! {
                    Text2d::new(content.clone())
                    TextFont { font_size: FontSize::Px(100.0) }
                    TextColor(Color::srgb(0.1, 0.12, 0.2))
                }
            });
        self.out.push(spot.hang(glyph));
    }

    /// A part that can be grabbed: a node that moves everything in it, with a box over it.
    fn part(
        &mut self,
        spot: &Spot,
        name: &str,
        left: f32,
        size: Size,
        base: f32,
        drag: impl Fn() -> DragSpec<Equation> + Send + Sync + 'static,
    ) -> Spot {
        let center = Vec2::new(left + size.w / 2.0, base + (size.asc - size.desc) / 2.0);
        let node = ViewNode::group(name.to_owned())
            .flat()
            .at((center - spot.origin).extend(STEP))
            .draggable(drag);
        self.out.push(spot.hang(node));
        self.out.push(
            ViewNode::grad(format!("box-{name}"), self.tile.clone())
                .with_parent(NodeId::from(name.to_owned()))
                .scaled_xy(Vec2::new(size.w, size.asc + size.desc + 2.0 * PAD))
                .with_color(Color::srgb(0.25, 0.5, 0.95))
                .with_opacity(0.12),
        );
        Spot {
            parent: Some(NodeId::from(name.to_owned())),
            origin: center,
        }
    }

    fn expr(&mut self, spot: &Spot, id: Id, left: f32, base: f32, side: Option<usize>) {
        let graph = &self.eq.graph;
        match graph.node(id).clone() {
            Expr::Num(n) => self.text(spot, &n.to_string(), left, base),
            Expr::Var(c) => self.text(spot, &c.to_string(), left, base),
            Expr::Neg(x) => {
                self.text(spot, "-", left, base);
                self.expr(spot, x, left + ADVANCE, base, None);
            }
            Expr::Sum(terms) => {
                let mut x = left;
                for (i, term) in terms.into_iter().enumerate() {
                    x = self.term(spot, term, x, base, i == 0);
                }
            }
            Expr::Mul(factors) => {
                let mut x = left;
                for (i, factor) in factors.into_iter().enumerate() {
                    x += self.factor(spot, factor, x, base, side.map(|s| (s, i)));
                }
            }
            Expr::Frac(numerator, divisor) => {
                let (n, d) = (measure(graph, numerator), measure(graph, divisor));
                let w = n.w.max(d.w) + 2.0 * GAP;
                let name = self.drawn(spot, "bar");
                let bar = ViewNode::rectangle(name, Vec2::new(w, 0.05))
                    .at((Vec2::new(left + w / 2.0, base + AXIS) - spot.origin).extend(STEP / 2.0))
                    .with_color(Color::srgb(0.1, 0.12, 0.2))
                    .pickable(false)
                    .flat();
                self.out.push(spot.hang(bar));
                self.expr(
                    spot,
                    numerator,
                    left + (w - n.w) / 2.0,
                    base + AXIS + GAP + n.desc,
                    None,
                );
                self.factor(
                    spot,
                    divisor,
                    left + (w - d.w) / 2.0,
                    base + AXIS - GAP - d.asc,
                    side.map(|s| (s, 0)),
                );
            }
        }
    }

    /// A factor, grabbable if it is `grab`, the side and index of a factor that can move; the
    /// width it takes.
    fn factor(
        &mut self,
        spot: &Spot,
        factor: Id,
        left: f32,
        base: f32,
        grab: Option<(usize, usize)>,
    ) -> f32 {
        let size = factor_size(&self.eq.graph, factor);
        let spot = match grab {
            Some((side, index)) => {
                let eq = self.eq.clone();
                let name = eq.names()[1][side][index].clone();
                self.part(spot, &name, left, size, base, move || {
                    factor_spec(&eq, side, index)
                })
            }
            None => spot.clone(),
        };
        let wrap = parenthesized(&self.eq.graph, factor);
        if wrap {
            self.text(&spot, "(", left, base);
            self.text(&spot, ")", left + size.w - PAREN, base);
        }
        let inner = left + if wrap { PAREN } else { 0.0 };
        self.expr(&spot, factor, inner, base, None);
        size.w
    }

    /// A term of a sum that is not a side, with its sign; the right end of it.
    fn term(&mut self, spot: &Spot, term: Id, left: f32, base: f32, first: bool) -> f32 {
        let graph = &self.eq.graph;
        let (sign, room) = lead(graph, term, first);
        if !sign.is_empty() {
            self.text(spot, sign, left + (room - ADVANCE) / 2.0, base);
        }
        self.expr(spot, body(graph, term), left + room, base, None);
        left + term_size(graph, term, first).w
    }

    /// The terms of a side, each one a part that can be grabbed.
    fn side(&mut self, spot: &Spot, side: usize, left: f32, base: f32) {
        let eq = self.eq;
        let terms = eq.terms(side);
        let names = &eq.names()[0][side];
        let alone = terms.len() == 1 && lead(&eq.graph, terms[0], true).0.is_empty();
        let mut x = left;
        for (index, &term) in terms.iter().enumerate() {
            let size = term_size(&eq.graph, term, index == 0);
            let snapshot = eq.clone();
            let inner = self.part(spot, &names[index], x, size, base, move || {
                term_spec(&snapshot, side, index)
            });
            let (sign, room) = lead(&eq.graph, term, index == 0);
            if !sign.is_empty() {
                self.text(&inner, sign, x + (room - ADVANCE) / 2.0, base);
            }
            let content = body(&eq.graph, term);
            self.expr(&inner, content, x + room, base, alone.then_some(side));
            x += size.w;
        }
    }
}

fn side_size(eq: &Equation, side: usize) -> Size {
    Size::beside(
        eq.terms(side)
            .into_iter()
            .enumerate()
            .map(|(i, t)| term_size(&eq.graph, t, i == 0)),
    )
}

fn view(eq: &Equation) -> View<Equation> {
    let sizes = [side_size(eq, 0), side_size(eq, 1)];
    let room = 2.0 * EM;
    let mut left = -(sizes[0].w + room + sizes[1].w) / 2.0;
    let tile = Shape::arbitrary_polygon(
        Shape::rounded_rectangle(Vec2::ONE, 0.2, 4),
        ShapeDetail::Low,
    );
    let mut pen = Pen {
        eq,
        out: vec![
            ViewNode::rectangle("paper", Vec2::new(16.0, 5.0))
                .with_z(-1.0)
                .with_color(Color::srgb(0.97, 0.97, 0.98))
                .pickable(false)
                .flat(),
        ],
        tile,
        drawn: HashMap::new(),
    };
    let top = Spot {
        parent: None,
        origin: Vec2::ZERO,
    };
    for (side, &size) in sizes.iter().enumerate() {
        pen.side(&top, side, left, 0.0);
        left += size.w;
        if side == 0 {
            pen.text(&top, "=", left + (room - ADVANCE) / 2.0, 0.0);
            left += room;
        }
    }
    pen.out.into_iter().fold(View::new(), View::with)
}

fn initial() -> Equation {
    let mut graph = Graph::new();
    let mut num = |n| graph.intern(Expr::Num(n));
    let (two, four, twelve) = (num(2), num(4), num(12));
    let x = graph.intern(Expr::Var('x'));
    let two_x = graph.intern(Expr::Mul(vec![two, x]));
    let minus_two_x = graph.intern(Expr::Neg(two_x));
    let lhs = graph.intern(Expr::Sum(vec![two_x, four]));
    let rhs = graph.intern(Expr::Sum(vec![twelve, minus_two_x]));
    Equation {
        graph,
        sides: [lhs, rhs],
    }
}

/// Run this example. Called by the WASM runner.
pub fn run(app: &mut App) {
    let eq = initial();
    let laid_out = view(&eq);
    let names = eq.names();
    let at = |name: &str| {
        laid_out
            .world_position(name)
            .expect("the name is laid out")
            .truncate()
    };
    let (from, to) = (
        at(&names[0][0][1]),
        at(&names[0][1][1]) + Vec2::X * 2.0 * EM,
    );
    app.add_plugins(common::default_plugins("graspable_math"))
        .add_plugins(DraggablePlugin::new(eq, view).with_rendering(Rendering::Flat))
        .add_plugins(ScriptedDragPlugin::straight(from, to))
        .add_plugins(
            AutoScreenshotPlugin::new("screenshot_graspable_math.gif")
                .with_title("graspable_math")
                .with_description("shared-graph: a term crosses the equals sign and flips its sign")
                .animation(600),
        )
        .add_systems(Startup, |mut commands: Commands| {
            common::spawn_cameras_2d(&mut commands, 5.0);
        })
        .add_systems(
            Update,
            |glyphs: Query<(&Name, Option<&Text2d>, &Transform)>, mut n: Local<u32>| {
                *n += 1;
                if n.is_multiple_of(20) {
                    let mut lines: Vec<String> = glyphs
                        .iter()
                        .filter(|(name, _, _)| name.contains('/'))
                        .map(|(name, text, t)| {
                            format!(
                                "{name} text={:?} scale={}",
                                text.map(|t| t.0.clone()),
                                t.scale.x
                            )
                        })
                        .collect();
                    lines.sort();
                    info!("DEBUG tick {}:\n{}", *n, lines.join("\n"));
                }
            },
        );
}

fn main() {
    let mut app = App::new();
    run(&mut app);
    app.run();
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use bevy_composable_dragging::{DragContext, Frame, Pointer};

    use super::*;

    fn text(graph: &Graph<Expr>, id: Id) -> String {
        match graph.node(id) {
            Expr::Num(n) => n.to_string(),
            Expr::Var(c) => c.to_string(),
            Expr::Neg(x) => format!("-{}", text(graph, *x)),
            Expr::Sum(terms) => terms
                .iter()
                .enumerate()
                .map(|(i, &term)| {
                    let sign = match (matches!(graph.node(term), Expr::Neg(_)), i) {
                        (true, _) => "-",
                        (false, 0) => "",
                        (false, _) => "+",
                    };
                    let body = text(graph, body(graph, term));
                    if sign.is_empty() {
                        body
                    } else {
                        format!("{sign}{body}")
                    }
                })
                .collect(),
            Expr::Mul(factors) => factors
                .iter()
                .map(|&f| {
                    let t = text(graph, f);
                    if parenthesized(graph, f) {
                        format!("({t})")
                    } else {
                        t
                    }
                })
                .collect(),
            Expr::Frac(n, d) => format!("{}/{}", text(graph, *n), text(graph, *d)),
        }
    }

    /// The equation as written: `2x+4=12-2x`.
    fn show(eq: &Equation) -> String {
        format!(
            "{}={}",
            text(&eq.graph, eq.sides[0]),
            text(&eq.graph, eq.sides[1])
        )
    }

    fn pick(equations: impl IntoIterator<Item = Equation>, shown: &str) -> Equation {
        equations
            .into_iter()
            .find(|eq| show(eq) == shown)
            .unwrap_or_else(|| panic!("no move leads to {shown}"))
    }

    fn merged(merges: Vec<(usize, Equation)>) -> impl Iterator<Item = Equation> {
        merges.into_iter().map(|(_, eq)| eq)
    }

    /// Solving `2x+4=12-2x` with nothing but the moves of the drags.
    fn steps() -> Vec<Equation> {
        let start = initial();
        let across = pick(term_moves(&start, 0, 1), "2x=12-2x-4");
        let combined = pick(merged(term_merges(&across, 1, 2)), "2x=8-2x");
        let both = pick(term_moves(&combined, 1, 1), "2x+2x=8");
        let like = pick(merged(term_merges(&both, 0, 1)), "4x=8");
        let solved = pick(factor_moves(&like, 0, 0), "x=8/4").settled();
        vec![start, across, combined, both, like, solved]
    }

    fn drag(eq: &Equation, name: &str, spec: DragSpec<Equation>, pointer: Vec2) -> Frame<Equation> {
        spec.compile(&DragContext::new(view, name.to_owned().into(), eq.clone()))(Pointer::from(
            pointer,
        ))
    }

    fn down_at(at: Vec2) -> Ray3d {
        Ray3d::new(at.extend(10.0), Dir3::NEG_Z)
    }

    #[test]
    fn the_equation_starts_as_written() {
        assert_eq!(show(&initial()), "2x+4=12-2x");
    }

    #[test]
    fn dragging_solves_the_equation() {
        let shown: Vec<_> = steps().iter().map(show).collect();
        assert_eq!(
            shown,
            [
                "2x+4=12-2x",
                "2x=12-2x-4",
                "2x=8-2x",
                "2x+2x=8",
                "4x=8",
                "x=2"
            ]
        );
    }

    #[test]
    fn a_term_keeps_its_name_when_it_flips_its_sign_across() {
        let steps = steps();
        let (before, after) = (steps[0].names(), steps[1].names());
        assert_eq!(before[0][0][1], after[0][1][2], "the 4 and the -4");
    }

    #[test]
    fn the_same_term_twice_has_two_names() {
        let both = &steps()[3];
        let names = &both.names()[0][0];
        assert_eq!(names.len(), 2);
        assert_ne!(names[0], names[1]);
    }

    #[test]
    fn every_node_of_the_view_has_its_own_name() {
        for eq in steps() {
            let view = view(&eq);
            let names: HashSet<_> = view.nodes().iter().map(|node| node.id.clone()).collect();
            assert_eq!(names.len(), view.len(), "{}", show(&eq));
        }
    }

    #[test]
    fn settling_keeps_only_what_the_equation_reaches() {
        for eq in steps() {
            let compact = eq.settled();
            assert_eq!(show(&compact), show(&eq));
            assert_eq!(
                compact.graph.len(),
                compact.graph.reachable(&compact.sides),
                "{}",
                show(&eq)
            );
            assert!(compact.graph.len() <= eq.graph.len());
        }
    }

    #[test]
    fn nothing_divides_by_zero() {
        let mut graph = Graph::new();
        let (zero, x, three) = (
            graph.intern(Expr::Num(0)),
            graph.intern(Expr::Var('x')),
            graph.intern(Expr::Num(3)),
        );
        let product = graph.intern(Expr::Mul(vec![zero, x]));
        let eq = Equation {
            graph,
            sides: [product, three],
        };
        let moves = factor_moves(&eq, 0, 0);
        assert!(!moves.is_empty());
        assert!(moves.iter().all(|eq| show(eq).ends_with("=3")));
    }

    #[test]
    fn a_far_drag_leaves_the_equation_and_the_part_follows_the_pointer() {
        let eq = initial();
        let name = &eq.names()[0][0][1];
        let pointer = Vec2::new(0.0, -2.2);
        let frame = drag(&eq, name, term_spec(&eq, 0, 1), pointer);
        assert_eq!(show(&frame.drop_state), show(&eq));
        let at = frame.view.world_position(name).unwrap().truncate();
        assert!(at.distance(pointer) < 0.3, "{at}");
    }

    #[test]
    fn dropping_a_term_on_a_like_term_combines_them() {
        let eq = &steps()[1];
        let names = &eq.names()[0][1];
        let on = view(eq).world_position(&names[0]).unwrap().truncate();
        let frame = drag(eq, &names[2], term_spec(eq, 1, 2), on);
        assert_eq!(show(&frame.drop_state), "2x=8-2x");
    }

    #[test]
    fn dragging_a_term_next_to_the_other_side_moves_it_across() {
        let eq = initial();
        let name = &eq.names()[0][0][1];
        let end = view(&eq)
            .world_position(&eq.names()[0][1][1])
            .unwrap()
            .truncate()
            + Vec2::X * EM;
        let frame = drag(&eq, name, term_spec(&eq, 0, 1), end);
        assert_eq!(show(&frame.drop_state), "2x=12-2x-4");
    }

    #[test]
    fn dragging_a_factor_across_divides_the_other_side() {
        let eq = &steps()[4];
        let four = eq.names()[1][0][0].clone();
        let end = view(eq)
            .world_position(&eq.names()[0][1][0])
            .unwrap()
            .truncate()
            + Vec2::X * EM;
        let frame = drag(eq, &four, factor_spec(eq, 0, 0), end);
        assert_eq!(show(&frame.drop_state), "x=2");
    }

    #[test]
    fn a_press_on_a_sign_grabs_the_term_and_on_a_number_the_factor() {
        let eq = &steps()[4];
        let laid_out = view(eq);
        let names = eq.names();
        let grabbed = |at: Vec2| {
            let hit = laid_out.hit_test(down_at(at)).expect("a press lands");
            laid_out.draggable_ancestor(&hit.id).unwrap().id.clone()
        };
        let four = laid_out.world_position(&names[1][0][0]).unwrap().truncate();
        assert_eq!(grabbed(four), names[1][0][0]);
        let eight = laid_out.world_position(&names[0][1][0]).unwrap().truncate();
        assert_eq!(grabbed(eight), names[0][1][0]);
    }

    #[test]
    fn the_scripted_pointer_presses_the_term_it_moves() {
        let eq = initial();
        let laid_out = view(&eq);
        let name = &eq.names()[0][0][1];
        let from = laid_out.world_position(name).unwrap().truncate();
        let hit = laid_out.hit_test(down_at(from)).unwrap();
        assert_eq!(&laid_out.draggable_ancestor(&hit.id).unwrap().id, name);
    }

    #[test]
    fn drag_across_center_view_always_has_text() {
        // Simulate dragging a term from its start position across the center to the other side,
        // step by step, and verify that every frame has text glyphs (none are missing).
        let eq = initial();
        let name = &eq.names()[0][0][1];
        let spec = term_spec(&eq, 0, 1);

        let from = view(&eq).world_position(name).unwrap().truncate();
        let to = view(&eq)
            .world_position(&eq.names()[0][1][1])
            .unwrap()
            .truncate()
            + Vec2::X * 2.0 * EM;

        for step in 0..=30 {
            let t = step as f32 / 30.0;
            let pointer_pos = from.lerp(to, t);
            let frame = drag(&eq, name, spec.clone(), pointer_pos);
            let glyph_count = frame
                .view
                .nodes()
                .iter()
                .filter(|n| n.id.contains('/'))
                .count();
            assert!(
                glyph_count > 0,
                "step {step}/30 (t={t}): no text glyphs in frame, expected some"
            );
        }
    }

    #[test]
    fn proptest_drag_across_center_has_text() {
        use proptest::prelude::*;

        proptest!(|(t in 0.0..=1.0_f32)| {
            let eq = initial();
            let name = &eq.names()[0][0][1];
            let spec = term_spec(&eq, 0, 1);

            let from = view(&eq)
                .world_position(name)
                .unwrap()
                .truncate();
            let to = view(&eq)
                .world_position(&eq.names()[0][1][1])
                .unwrap()
                .truncate()
                + Vec2::X * 2.0 * EM;

            let pointer_pos = from.lerp(to, t);
            let frame = drag(&eq, name, spec.clone(), pointer_pos);

            let glyph_count = frame
                .view
                .nodes()
                .iter()
                .filter(|n| n.id.contains('/'))
                .count();

            prop_assert!(
                glyph_count > 0,
                "t={}: no text glyphs in frame",
                t
            );
        });
    }

    #[test]
    fn proptest_drag_all_terms_has_text() {
        use proptest::prelude::*;

        proptest!(|(side in 0usize..2, t in 0.0..=1.0_f32)| {
            let eq = initial();
            let terms = eq.terms(side);
            if terms.is_empty() {
                return Ok(());
            }
            let index = 0;
            let name = &eq.names()[0][side][index];
            let spec = term_spec(&eq, side, index);

            let from = view(&eq)
                .world_position(name)
                .unwrap()
                .truncate();
            // Drag toward the other side
            let other = 1 - side;
            let other_terms = eq.terms(other);
            let to = if other_terms.is_empty() {
                from + Vec2::X * (if side == 0 { 1.0 } else { -1.0 }) * 3.0 * EM
            } else {
                view(&eq)
                    .world_position(&eq.names()[0][other][0])
                    .unwrap()
                    .truncate()
                    + Vec2::X * (if side == 0 { 1.0 } else { -1.0 }) * EM
            };

            let pointer_pos = from.lerp(to, t);
            let frame = drag(&eq, name, spec.clone(), pointer_pos);

            let glyph_count = frame
                .view
                .nodes()
                .iter()
                .filter(|n| n.id.contains('/'))
                .count();

            prop_assert!(
                glyph_count > 0,
                "side={} t={}: no text glyphs in frame",
                side,
                t
            );
        });
    }
}
