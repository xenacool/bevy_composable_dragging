//! Documents described as data share what is equal: a particle effect in the schema of
//! `bevy_sprinkles` and a lava terrain that both name one material hold one node and one asset,
//! and a rewrite that makes two materials equal leaves one.

use std::collections::BTreeMap;

use bevy::asset::{Asset, Assets};
use bevy::reflect::TypePath;
use bevy_composable_dragging::graph::{Graph, Id, Instances, Node};
use serde::Deserialize;

/// A trimmed Magic Puff effect, with a `materials` table that emitters and the terrain name.
const PUFF: &str = r#"(
    sprinkles_version: "0.3",
    name: "Magic Puff",
    materials: {
        "lava": Standard((
            base_color: (1.0, 0.3, 0.0, 1.0),
            emissive: (8.0, 2.0, 0.0, 1.0),
            alpha_mode: Add,
        )),
    },
    emitters: [
        (
            name: "Shock",
            draw_pass: (
                mesh: Sphere(radius: 1.25, segments: 32, rings: 16),
                material: Standard((
                    base_color: (1.0, 1.0, 1.0, 1.0),
                    alpha_mode: Blend,
                    unlit: true,
                )),
            ),
            emission: (particles_amount: 1),
        ),
        (
            name: "Balls",
            draw_pass: (
                mesh: Quad(orientation: FaceZ, size: (0.3, 0.3)),
                material: Standard((
                    base_color: (0.0, 1.0, 1.0, 1.0),
                    base_color_texture: Some(Preset(Fire2)),
                    emissive: (0.0, 50.0, 50.0, 1.0),
                    alpha_mode: Add,
                )),
            ),
            emission: (particles_amount: 10),
        ),
        (
            name: "Sparks",
            draw_pass: (
                mesh: Quad(orientation: FaceZ, size: (1.0, 0.5)),
                material: Standard((
                    base_color: (0.0, 1.0, 1.0, 1.0),
                    base_color_texture: Some(Preset(Trace1)),
                    emissive: (0.0, 50.0, 50.0, 1.0),
                    alpha_mode: Add,
                )),
            ),
            emission: (particles_amount: 50),
        ),
        (
            name: "Lava Spit",
            draw_pass: (mesh: Sphere(radius: 0.1, segments: 8, rings: 4), material: Ref("lava")),
            emission: (particles_amount: 24),
        ),
    ],
    terrain: [(name: "Lava Flow", material: Ref("lava"))],
)"#;

#[derive(Deserialize)]
struct Effect {
    name: String,
    #[serde(default)]
    materials: BTreeMap<String, Defined>,
    emitters: Vec<EmitterSpec>,
    #[serde(default)]
    terrain: Vec<TerrainSpec>,
}

#[derive(Deserialize)]
struct EmitterSpec {
    name: String,
    draw_pass: DrawPass,
    emission: Emission,
}

#[derive(Deserialize)]
struct DrawPass {
    material: Material,
}

#[derive(Deserialize)]
struct Emission {
    particles_amount: u32,
}

#[derive(Deserialize)]
struct TerrainSpec {
    name: String,
    material: Material,
}

/// A material of the `materials` table.
#[derive(Deserialize)]
enum Defined {
    Standard(Spec),
}

/// A material written inline, or the name of one in the `materials` table.
#[derive(Deserialize)]
enum Material {
    Standard(Spec),
    Ref(String),
}

#[derive(Clone, Deserialize)]
struct Spec {
    base_color: [f32; 4],
    #[serde(default)]
    base_color_texture: Option<Texture>,
    #[serde(default)]
    emissive: [f32; 4],
    alpha_mode: Alpha,
    #[serde(default)]
    unlit: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Deserialize)]
enum Alpha {
    Opaque,
    Blend,
    Add,
}

#[derive(Clone, PartialEq, Eq, Hash, Debug, Deserialize)]
enum Texture {
    Preset(Preset),
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Deserialize)]
enum Preset {
    Fire2,
    Trace1,
}

/// What the graph stores. A float is its bits, so that equal materials are equal values.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
enum Part {
    Material {
        color: [u32; 4],
        texture: Option<Texture>,
        emissive: [u32; 4],
        alpha: Alpha,
        unlit: bool,
    },
    Emitter {
        name: String,
        material: Id,
        amount: u32,
    },
    Terrain {
        name: String,
        material: Id,
    },
    Effect {
        name: String,
        parts: Vec<Id>,
    },
}

impl Node for Part {
    fn children(&self) -> Vec<Id> {
        match self {
            Part::Material { .. } => vec![],
            Part::Emitter { material, .. } | Part::Terrain { material, .. } => vec![*material],
            Part::Effect { parts, .. } => parts.clone(),
        }
    }

    fn with_children(&self, children: &[Id]) -> Self {
        match self {
            Part::Material { .. } => self.clone(),
            Part::Emitter { name, amount, .. } => Part::Emitter {
                name: name.clone(),
                material: children[0],
                amount: *amount,
            },
            Part::Terrain { name, .. } => Part::Terrain {
                name: name.clone(),
                material: children[0],
            },
            Part::Effect { name, .. } => Part::Effect {
                name: name.clone(),
                parts: children.to_vec(),
            },
        }
    }
}

fn bits(color: [f32; 4]) -> [u32; 4] {
    color.map(f32::to_bits)
}

fn material(spec: &Spec) -> Part {
    Part::Material {
        color: bits(spec.base_color),
        texture: spec.base_color_texture.clone(),
        emissive: bits(spec.emissive),
        alpha: spec.alpha_mode,
        unlit: spec.unlit,
    }
}

/// The effect in `source` as nodes of `graph`: the root of the effect.
fn load(graph: &mut Graph<Part>, source: &str) -> Id {
    let effect: Effect = ron::from_str(source).expect("the effect parses");
    let named: BTreeMap<&str, Id> = effect
        .materials
        .iter()
        .map(|(name, Defined::Standard(spec))| (name.as_str(), graph.intern(material(spec))))
        .collect();
    let resolve = |graph: &mut Graph<Part>, choice: &Material| match choice {
        Material::Standard(spec) => graph.intern(material(spec)),
        Material::Ref(name) => named[name.as_str()],
    };
    let mut parts = Vec::new();
    for emitter in &effect.emitters {
        let material = resolve(graph, &emitter.draw_pass.material);
        parts.push(graph.intern(Part::Emitter {
            name: emitter.name.clone(),
            material,
            amount: emitter.emission.particles_amount,
        }));
    }
    for terrain in &effect.terrain {
        let material = resolve(graph, &terrain.material);
        parts.push(graph.intern(Part::Terrain {
            name: terrain.name.clone(),
            material,
        }));
    }
    graph.intern(Part::Effect {
        name: effect.name,
        parts,
    })
}

/// The ids of the distinct materials that `root` reaches.
fn materials(graph: &Graph<Part>, root: Id) -> Vec<Id> {
    let mut found: Vec<Id> = graph
        .paths(root)
        .iter()
        .filter_map(|path| graph.at(root, path))
        .filter(|id| matches!(graph.node(*id), Part::Material { .. }))
        .collect();
    found.sort();
    found.dedup();
    found
}

#[derive(Asset, TypePath)]
struct Paint;

#[test]
fn a_material_named_by_a_terrain_and_an_emitter_is_one_node() {
    let mut graph = Graph::new();
    let root = load(&mut graph, PUFF);
    let Part::Effect { parts, .. } = graph.node(root).clone() else {
        panic!("the root is the effect");
    };
    let (spit, flow) = (parts[3], parts[4]);
    let (Part::Emitter { material: a, .. }, Part::Terrain { material: b, .. }) =
        (graph.node(spit), graph.node(flow))
    else {
        panic!("an emitter and a terrain");
    };
    assert_eq!(a, b);
    // Shock, Balls, Sparks and lava: four materials for five users.
    assert_eq!(materials(&graph, root).len(), 4);
    assert_eq!(graph.reachable(&[root]), 1 + 5 + 4);
}

#[test]
fn a_material_written_inline_equal_to_a_named_one_is_the_same_node() {
    let mut graph = Graph::new();
    let first = load(&mut graph, PUFF);
    let inline = PUFF.replace(
        r#"material: Ref("lava")"#,
        "material: Standard((base_color: (1.0, 0.3, 0.0, 1.0), emissive: (8.0, 2.0, 0.0, 1.0), alpha_mode: Add))",
    );
    let size = graph.len();
    let second = load(&mut graph, &inline);
    assert_eq!(first, second, "the same document is the same node");
    assert_eq!(graph.len(), size, "and loading it again stored nothing");
}

#[test]
fn a_rewrite_that_makes_two_materials_equal_leaves_one() {
    let mut graph = Graph::new();
    let root = load(&mut graph, PUFF);
    assert_eq!(materials(&graph, root).len(), 4);
    // Sparks take the texture of Balls: their materials are now the same material.
    let rewritten = graph.rewrite(root, &mut |_, part| match part {
        Part::Material {
            texture: Some(Texture::Preset(Preset::Trace1)),
            color,
            emissive,
            alpha,
            unlit,
        } => Part::Material {
            texture: Some(Texture::Preset(Preset::Fire2)),
            color,
            emissive,
            alpha,
            unlit,
        },
        other => other,
    });
    assert_eq!(materials(&graph, rewritten).len(), 3);
    assert_ne!(rewritten, root);
}

#[test]
fn every_use_of_a_material_has_a_path_and_replacing_one_leaves_the_other() {
    let mut graph = Graph::new();
    let root = load(&mut graph, PUFF);
    let lava = graph.at(root, &[3, 0]).unwrap();
    assert_eq!(graph.at(root, &[4, 0]), Some(lava));
    let shock = graph.at(root, &[0, 0]).unwrap();
    let edited = graph.replace_at(root, &[4, 0], shock).unwrap();
    assert_eq!(
        graph.at(edited, &[3, 0]),
        Some(lava),
        "the emitter keeps lava"
    );
    assert_eq!(
        graph.at(edited, &[4, 0]),
        Some(shock),
        "the terrain changed"
    );
}

#[test]
fn equal_materials_cost_one_asset_between_terrain_and_particles() {
    let mut graph = Graph::new();
    let root = load(&mut graph, PUFF);
    let mut assets = Assets::<Paint>::default();
    let mut instances = Instances::default();
    for path in graph.paths(root) {
        let id = graph.at(root, &path).unwrap();
        if matches!(graph.node(id), Part::Material { .. }) {
            instances.get_or_add(id, &mut assets, || Paint);
        }
    }
    assert_eq!(assets.len(), 4, "five uses of four materials");
}
