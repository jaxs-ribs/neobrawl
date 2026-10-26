//! NeoBrawl's scene graph: GameObjects with active flags, tags and layers,
//! (Rect)Transforms, and the Unity components the game uses, loaded from the
//! JSON written by tools/unity4/extract5.py.

use crate::det::HashMap;

use serde_json::Value;
use crate::shared::math::{v3, Mat4, Quat, V3};

pub type Color = [f32; 4];

#[derive(Clone, Debug, Default)]
pub struct RectT {
    pub anchor_min: [f32; 2],
    pub anchor_max: [f32; 2],
    pub pos: [f32; 2],
    pub size: [f32; 2],
    pub pivot: [f32; 2],
}

#[derive(Clone, Debug)]
pub struct SpriteR {
    pub enabled: bool,
    pub sprite: Option<usize>,
    pub color: Color,
    pub order: i32,
}

#[derive(Clone, Debug)]
pub struct ImageC {
    pub enabled: bool,
    /// CanvasRenderer color (Button tint), multiplied with `color`
    pub tint: Color,
    pub sprite: Option<usize>,
    pub color: Color,
    pub kind: i32,
    pub fill: f32,
    pub fill_method: i32,
    pub fill_origin: i32,
    pub raycast: bool,
}

#[derive(Clone, Debug)]
pub struct TextC {
    /// LetterSpacing component: extra advance = spacing * fontSize / 100
    pub spacing: f32,
    pub enabled: bool,
    pub tint: Color,
    pub raycast: bool,
    pub text: String,
    pub size: f32,
    pub style: i32,
    pub align: i32,
    pub color: Color,
}

#[derive(Clone, Debug)]
pub struct LineR {
    pub enabled: bool,
    pub points: Vec<V3>,
    pub width: [f32; 2],
    pub colors: [Color; 2],
    pub world: bool,
}

#[derive(Clone, Debug)]
pub struct TrailR {
    pub enabled: bool,
    pub time: f32,
    pub width: [f32; 2],
    pub color: Color,
    pub points: Vec<(V3, f32)>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    Box { size: V3, center: V3 },
    Capsule { radius: f32, center: V3 },
    /// convex hull traced from the sprite (local units, CCW, n used)
    Hull { n: u8, pts: [[f32; 2]; 16] },
}

/// A sprite re-drawn as vectors (tools/vectorize.py): fill triangles,
/// outline rings (outer first, then holes) and a convex hull, all local.
#[derive(Clone, Debug, Default)]
pub struct VShape {
    pub tris: Vec<[f32; 6]>,
    pub outline: Vec<Vec<[f32; 2]>>,
    pub hull: Vec<[f32; 2]>,
    /// the bitmap's own colour, multiplied by the sprite tint
    pub ink: [f32; 3],
}

#[derive(Clone, Debug)]
pub struct Collider {
    pub shape: Shape,
    pub enabled: bool,
    pub trigger: bool,
}

#[derive(Clone, Debug)]
pub struct Body {
    pub mass: f32,
    pub drag: f32,
    pub constraints: i32,
    pub velocity: V3,
    pub spin: f32,
}

pub const FREEZE_ALL: i32 = 126;

#[derive(Clone, Debug)]
pub struct Click {
    pub target: Option<usize>,
    pub method: String,
    pub arg: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct CameraC {
    pub fov: f32,
    pub near: f32,
    pub far: f32,
    pub bg: Color,
    pub ortho: bool,
    pub ortho_size: f32,
    pub depth: f32,
}

#[derive(Clone, Debug)]
pub struct Node {
    pub name: std::sync::Arc<str>,
    pub tag: std::sync::Arc<str>,
    pub layer: u32,
    pub active: bool,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub lpos: V3,
    pub lrot: Quat,
    pub lscale: V3,
    pub rect: Option<RectT>,
    pub sprite: Option<SpriteR>,
    pub mesh: Option<(usize, usize, bool)>,
    pub line: Option<LineR>,
    pub trail: Option<TrailR>,
    pub collider: Option<Collider>,
    pub body: Option<Body>,
    pub camera: Option<CameraC>,
    pub canvas: Option<(i32, Option<usize>, f32)>,
    pub scaler: Option<([f32; 2], f32, i32)>,
    pub image: Option<ImageC>,
    pub text: Option<TextC>,
    // static data, shared between snapshots (rollback clones the world)
    pub button: std::sync::Arc<Vec<Click>>,
    pub audio: Option<String>,
    pub scripts: std::sync::Arc<HashMap<String, Value>>,
}

#[derive(Clone, Debug)]
pub struct SpriteAsset {
    pub name: String,
    pub tex: usize,
    /// uv rect in the texture (0..1, origin bottom-left): x, y, w, h
    pub uv: [f32; 4],
    /// local quad in units: x0, y0, x1, y1
    pub quad: [f32; 4],
    /// size in pixels / ppu (for UI "native size")
    pub ppu: f32,
    pub border: f32,
    pub vector: Option<std::sync::Arc<VShape>>,
}

/// The scene's nodes, copy-on-write: a rollback snapshot shares every node
/// with the live world, and a node is copied only when written after it.
#[derive(Clone, Default)]
pub struct Nodes(Vec<std::rc::Rc<Node>>);

impl std::ops::Index<usize> for Nodes {
    type Output = Node;
    #[inline]
    fn index(&self, i: usize) -> &Node {
        &self.0[i]
    }
}

impl std::ops::IndexMut<usize> for Nodes {
    #[inline]
    fn index_mut(&mut self, i: usize) -> &mut Node {
        std::rc::Rc::make_mut(&mut self.0[i])
    }
}

impl Nodes {
    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn push(&mut self, n: Node) {
        self.0.push(std::rc::Rc::new(n));
    }
    pub fn iter(&self) -> impl Iterator<Item = &Node> + '_ {
        self.0.iter().map(|n| &**n)
    }
    pub fn get(&self, i: usize) -> Option<&Node> {
        self.0.get(i).map(|n| &**n)
    }
    /// How many nodes this world shares with others (diagnostics).
    pub fn shared(&self) -> usize {
        self.0.iter().filter(|n| std::rc::Rc::strong_count(n) > 1).count()
    }
}

#[derive(Clone)]
pub struct World {
    pub nodes: Nodes,
    // lookup tables and assets: static, shared between snapshots
    by_name: std::sync::Arc<HashMap<String, Vec<usize>>>,
    pub objects: std::sync::Arc<HashMap<String, usize>>,
    pub textures: std::sync::Arc<Vec<(String, String)>>,
    pub sprites: std::sync::Arc<Vec<SpriteAsset>>,
    pub sprite_keys: std::sync::Arc<HashMap<String, usize>>,
    pub meshes: std::sync::Arc<Vec<String>>,
    pub materials: std::sync::Arc<Vec<(String, Color, Option<usize>)>>,
    pub audio: std::sync::Arc<HashMap<String, String>>,
    pub font: Option<String>,
}

fn f(v: &Value) -> f32 {
    v.as_f64().unwrap_or(0.0) as f32
}

fn v3j(v: &Value) -> V3 {
    v3(f(&v[0]), f(&v[1]), f(&v[2]))
}

fn col(v: &Value) -> Color {
    [f(&v[0]), f(&v[1]), f(&v[2]), f(&v[3])]
}

fn v2(v: &Value) -> [f32; 2] {
    [f(&v[0]), f(&v[1])]
}

fn rgba32(c: u32) -> Color {
    [(c & 255) as f32 / 255.0, ((c >> 8) & 255) as f32 / 255.0, ((c >> 16) & 255) as f32 / 255.0, (c >> 24) as f32 / 255.0]
}

impl World {
    pub fn load(scene_json: &str, assets_json: &str) -> World {
        let assets: Value = serde_json::from_str(assets_json).expect("assets json");
        let scene: Value = serde_json::from_str(scene_json).expect("scene json");

        let mut tex_keys: Vec<String> = assets["textures"].as_object().unwrap().keys().cloned().collect();
        tex_keys.sort();
        let textures = tex_keys
            .iter()
            .map(|k| (k.clone(), assets["textures"][k]["file"].as_str().unwrap().to_string()))
            .collect::<Vec<_>>();
        let tex_index = |k: &str| tex_keys.iter().position(|x| x == k);

        let mut sprite_keys_sorted: Vec<String> = assets["sprites"].as_object().unwrap().keys().cloned().collect();
        sprite_keys_sorted.sort();
        let mut sprites = Vec::new();
        let mut sprite_keys = HashMap::default();
        for k in &sprite_keys_sorted {
            let s = &assets["sprites"][k];
            let tk = s["tex"].as_str().unwrap_or("");
            let Some(ti) = tex_index(tk) else { continue };
            let tw = assets["textures"][tk]["w"].as_f64().unwrap() as f32;
            let th = assets["textures"][tk]["h"].as_f64().unwrap() as f32;
            let rect = [f(&s["rect"][0]), f(&s["rect"][1]), f(&s["rect"][2]), f(&s["rect"][3])];
            let tr = [f(&s["texRect"][0]), f(&s["texRect"][1]), f(&s["texRect"][2]), f(&s["texRect"][3])];
            let tro = v2(&s["texRectOffset"]);
            let off = v2(&s["offset"]);
            let ppu = f(&s["ppu"]);
            // pivot = rect center + offset (pixels, relative to the rect origin)
            let pivot = [rect[2] / 2.0 + off[0], rect[3] / 2.0 + off[1]];
            let x0 = (tro[0] - pivot[0]) / ppu;
            let y0 = (tro[1] - pivot[1]) / ppu;
            sprite_keys.insert(k.clone(), sprites.len());
            sprites.push(SpriteAsset {
                name: s["name"].as_str().unwrap_or("").to_string(),
                tex: ti,
                uv: [tr[0] / tw, tr[1] / th, tr[2] / tw, tr[3] / th],
                quad: [x0, y0, x0 + tr[2] / ppu, y0 + tr[3] / ppu],
                ppu,
                border: 0.0,
                vector: assets.get("shapes").and_then(|sh| sh.get(s["name"].as_str().unwrap_or(""))).map(|v| {
                    let pt = |p: &Value| [f(&p[0]), f(&p[1])];
                    std::sync::Arc::new(VShape {
                        tris: v["tris"].as_array().unwrap().iter().map(|t| {
                            let a = t.as_array().unwrap();
                            [f(&a[0]), f(&a[1]), f(&a[2]), f(&a[3]), f(&a[4]), f(&a[5])]
                        }).collect(),
                        outline: v["outline"].as_array().unwrap().iter().map(|r| r.as_array().unwrap().iter().map(pt).collect()).collect(),
                        hull: v["hull"].as_array().unwrap().iter().map(pt).collect(),
                        ink: [f(&v["ink"][0]), f(&v["ink"][1]), f(&v["ink"][2])],
                    })
                }),
            });
        }
        let mut mesh_keys: Vec<String> = assets["meshes"].as_object().unwrap().keys().cloned().collect();
        mesh_keys.sort();
        let mut mat_keys: Vec<String> = assets["materials"].as_object().unwrap().keys().cloned().collect();
        mat_keys.sort();
        let materials = mat_keys
            .iter()
            .map(|k| {
                let m = &assets["materials"][k];
                let c = if m["color"].is_array() { col(&m["color"]) } else { [1.0; 4] };
                (k.clone(), c, m["tex"].as_str().and_then(|t| tex_index(t)))
            })
            .collect::<Vec<_>>();
        let audio = assets["audio"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v["name"].as_str().unwrap().to_string()))
            .collect();
        let font = assets["fonts"].as_object().unwrap().values().next().map(|v| v["name"].as_str().unwrap().to_string());

        let objects: HashMap<String, usize> = scene["objects"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (format!("obj:{k}"), v.as_u64().unwrap() as usize))
            .collect();
        let obj = |v: &Value| v.as_str().and_then(|s| objects.get(s).copied());

        let raw = scene["nodes"].as_array().unwrap();
        let mut tints: Vec<(Option<usize>, Color)> = Vec::new();
        let mut nodes: Vec<Node> = raw
            .iter()
            .map(|n| {
                let parent = n["parent"].as_i64().unwrap();
                let mut scripts: HashMap<String, Value> = HashMap::default();
                let mut image = None;
                let mut text = None;
                let mut button = Vec::new();
                let mut scaler = None;
                for (name, s) in n["scripts"].as_object().unwrap() {
                    match name.as_str() {
                        "Image" => {
                            image = Some(ImageC {
                                enabled: s["enabled"].as_bool().unwrap_or(true),
                                tint: [1.0; 4],
                                sprite: s["sprite"].as_str().and_then(|k| sprite_keys.get(k).copied()),
                                color: col(&s["color"]),
                                kind: s["type"].as_i64().unwrap_or(0) as i32,
                                fill: f(&s["fillAmount"]),
                                fill_method: s["fillMethod"].as_i64().unwrap_or(0) as i32,
                                fill_origin: s["fillOrigin"].as_i64().unwrap_or(0) as i32,
                                raycast: s["raycast"].as_bool().unwrap_or(true),
                            })
                        }
                        "Text" => {
                            text = Some(TextC {
                                spacing: f(&n["scripts"]["LetterSpacing"]["spacing"]),
                                enabled: s["enabled"].as_bool().unwrap_or(true),
                                tint: [1.0; 4],
                                raycast: s["raycast"].as_bool().unwrap_or(true),
                                text: s["text"].as_str().unwrap_or("").to_string(),
                                size: f(&s["fontSize"]),
                                style: s["fontStyle"].as_i64().unwrap_or(0) as i32,
                                align: s["alignment"].as_i64().unwrap_or(0) as i32,
                                color: col(&s["color"]),
                            })
                        }
                        "Button" => {
                            if s["transition"].as_i64() == Some(1) {
                                tints.push((obj(&s["targetGraphic"]), col(&s["tint"])));
                            }
                            for c in s["onClick"].as_array().unwrap() {
                                button.push(Click {
                                    target: obj(&c["target"]),
                                    method: c["method"].as_str().unwrap_or("").to_string(),
                                    arg: obj(&c["obj"]),
                                });
                            }
                        }
                        "CanvasScaler" => {
                            scaler = Some((v2(&s["refResolution"]), f(&s["match"]), s["matchMode"].as_i64().unwrap_or(0) as i32))
                        }
                        _ => {}
                    }
                    let fields = s.get("fields").cloned().unwrap_or(Value::Null);
                    let mut fields = fields;
                    if let Value::Object(m) = &mut fields {
                        m.insert("__enabled".into(), s["enabled"].clone());
                    }
                    scripts.insert(name.clone(), fields);
                }
                let shape = if let Some(b) = n.get("box") {
                    Some(Collider {
                        shape: Shape::Box { size: v3j(&b["size"]), center: v3j(&b["center"]) },
                        enabled: b["enabled"].as_bool().unwrap(),
                        trigger: b["trigger"].as_bool().unwrap(),
                    })
                } else {
                    n.get("capsule").map(|c| Collider {
                        shape: Shape::Capsule { radius: f(&c["radius"]), center: v3j(&c["center"]) },
                        enabled: c["enabled"].as_bool().unwrap(),
                        trigger: c["trigger"].as_bool().unwrap(),
                    })
                };
                Node {
                    name: n["name"].as_str().unwrap().into(),
                    tag: n["tag"].as_str().unwrap_or("").into(),
                    layer: n["layer"].as_u64().unwrap_or(0) as u32,
                    active: n["active"].as_bool().unwrap(),
                    parent: if parent < 0 { None } else { Some(parent as usize) },
                    children: Vec::new(),
                    lpos: v3j(&n["pos"]),
                    lrot: Quat::from_slice(&[f(&n["rot"][0]), f(&n["rot"][1]), f(&n["rot"][2]), f(&n["rot"][3])]),
                    lscale: v3j(&n["scale"]),
                    rect: n.get("rect").map(|r| RectT {
                        anchor_min: v2(&r["anchorMin"]),
                        anchor_max: v2(&r["anchorMax"]),
                        pos: v2(&r["pos"]),
                        size: v2(&r["size"]),
                        pivot: v2(&r["pivot"]),
                    }),
                    sprite: n.get("sprite").map(|s| SpriteR {
                        enabled: s["enabled"].as_bool().unwrap(),
                        sprite: s["sprite"].as_str().and_then(|k| sprite_keys.get(k).copied()),
                        color: col(&s["color"]),
                        order: s["order"].as_i64().unwrap_or(0) as i32,
                    }),
                    mesh: match (n.get("mesh"), n.get("meshRenderer")) {
                        (Some(m), Some(r)) => {
                            let mi = m.as_str().and_then(|k| mesh_keys.iter().position(|x| x == k));
                            let mat = r["materials"][0].as_str().and_then(|k| mat_keys.iter().position(|x| x == k));
                            match (mi, mat) {
                                (Some(a), Some(b)) => Some((a, b, r["enabled"].as_bool().unwrap())),
                                _ => None,
                            }
                        }
                        _ => None,
                    },
                    line: n.get("line").map(|l| LineR {
                        enabled: l["enabled"].as_bool().unwrap(),
                        points: l["points"].as_array().unwrap().iter().map(v3j).collect(),
                        width: v2(&l["width"]),
                        colors: [rgba32(l["colors"][0].as_u64().unwrap() as u32), rgba32(l["colors"][1].as_u64().unwrap() as u32)],
                        world: l["world"].as_bool().unwrap(),
                    }),
                    trail: n.get("trail").map(|t| TrailR {
                        enabled: t["enabled"].as_bool().unwrap(),
                        time: f(&t["time"]),
                        width: v2(&t["width"]),
                        color: t["materials"][0]
                            .as_str()
                            .and_then(|k| assets["materials"].get(k))
                            .map(|m| col(&m["color"]))
                            .unwrap_or([0.0, 0.0, 0.0, 1.0]),
                        points: Vec::new(),
                    }),
                    collider: shape,
                    body: n.get("rigidbody").map(|b| Body {
                        mass: f(&b["mass"]),
                        drag: f(&b["drag"]),
                        constraints: b["constraints"].as_i64().unwrap() as i32,
                        velocity: V3::ZERO,
                        spin: 0.0,
                    }),
                    camera: n.get("camera").map(|c| CameraC {
                        fov: f(&c["fov"]),
                        near: f(&c["near"]),
                        far: f(&c["far"]),
                        bg: col(&c["bg"]),
                        ortho: c["ortho"].as_bool().unwrap(),
                        ortho_size: f(&c["orthoSize"]),
                        depth: f(&c["depth"]),
                    }),
                    canvas: n.get("canvas").map(|c| {
                        (c["mode"].as_i64().unwrap() as i32, c["camera"].as_str().and_then(|s| objects.get(s).copied()), f(&c["planeDistance"]))
                    }),
                    scaler,
                    image,
                    text,
                    button: button.into(),
                    audio: n.get("audioSource").and_then(|a| a["clip"].as_str()).and_then(|k| assets["audio"].get(k)).map(|a| a["name"].as_str().unwrap().to_string()),
                    scripts: scripts.into(),
                }
            })
            .collect();
        for i in 0..nodes.len() {
            if let Some(p) = nodes[i].parent {
                nodes[p].children.push(i);
            }
        }
        // Button ColorTint: the target graphic's CanvasRenderer color is the
        // normal color (invisible hit areas use alpha 0)
        for (target, tint) in tints {
            if let Some(t) = target {
                if let Some(img) = nodes[t].image.as_mut() {
                    img.tint = tint;
                }
                if let Some(txt) = nodes[t].text.as_mut() {
                    txt.tint = tint;
                }
            }
        }
        // Unity keeps children in sibling order; the extractor emits them in
        // file order, which matches for prefab instances but not always, so
        // restore order from the Transform child lists where available.
        let order = scene.get("childOrder");
        if let Some(Value::Object(m)) = order {
            for (k, v) in m {
                if let (Ok(p), Some(arr)) = (k.parse::<usize>(), v.as_array()) {
                    nodes[p].children = arr.iter().filter_map(|x| x.as_u64().map(|x| x as usize)).collect();
                }
            }
        }
        // Box colliders on traced sprites become the sprite's own hull, so
        // what you see is exactly what collides (the originals were rough
        // rectangles: a box's was 61% of its drawn size).
        for n in nodes.iter_mut() {
            let hull = n.sprite.as_ref().and_then(|s| s.sprite).and_then(|k| sprites[k].vector.clone()).map(|v| v.hull.clone());
            if let (Some(h), Some(c)) = (hull, n.collider.as_mut()) {
                if matches!(c.shape, Shape::Box { .. }) && h.len() >= 3 {
                    let mut pts = [[0.0f32; 2]; 16];
                    for (k, p) in h.iter().take(16).enumerate() {
                        pts[k] = *p;
                    }
                    c.shape = Shape::Hull { n: h.len().min(16) as u8, pts };
                }
            }
        }
        let mut by_name: HashMap<String, Vec<usize>> = HashMap::default();
        for (i, n) in nodes.iter().enumerate() {
            by_name.entry(n.name.to_string()).or_default().push(i);
        }
        use std::sync::Arc;
        World { nodes: Nodes(nodes.into_iter().map(std::rc::Rc::new).collect()), by_name: Arc::new(by_name), objects: Arc::new(objects), textures: Arc::new(textures), sprites: Arc::new(sprites), sprite_keys: Arc::new(sprite_keys), meshes: Arc::new(mesh_keys), materials: Arc::new(materials), audio: Arc::new(audio), font }
    }

    pub fn obj(&self, key: &Value) -> Option<usize> {
        key.as_str().and_then(|s| self.objects.get(s).copied())
    }

    pub fn active_in_hierarchy(&self, i: usize) -> bool {
        let mut c = Some(i);
        while let Some(n) = c {
            if !self.nodes[n].active {
                return false;
            }
            c = self.nodes[n].parent;
        }
        true
    }

    pub fn set_active(&mut self, i: usize, b: bool) {
        self.nodes[i].active = b;
    }

    /// GameObject.Find: first active object with that name.
    /// Copy a node and its descendants (minus `skip` and below) under the
    /// same parent; returns the new root. Scripts are not copied: the copy
    /// is plain geometry, colliders and bodies.
    pub fn clone_subtree(&mut self, root: usize, skip: &[usize]) -> usize {
        fn go(w: &mut World, i: usize, parent: Option<usize>, skip: &[usize]) -> usize {
            let mut n = w.nodes[i].clone();
            n.parent = parent;
            n.scripts = Default::default();
            n.button = Default::default();
            let kids = std::mem::take(&mut n.children);
            let id = w.nodes.len();
            std::sync::Arc::make_mut(&mut w.by_name).entry(n.name.to_string()).or_default().push(id);
            w.nodes.push(n);
            for c in kids {
                if skip.contains(&c) {
                    continue;
                }
                let cc = go(w, c, Some(id), skip);
                w.nodes[id].children.push(cc);
            }
            id
        }
        let parent = self.nodes[root].parent;
        let id = go(self, root, parent, skip);
        if let Some(p) = parent {
            self.nodes[p].children.push(id);
        }
        id
    }

    /// Move a node under a new parent, keeping where it is in the world.
    pub fn reparent(&mut self, i: usize, parent: usize) {
        let at = self.position(i);
        let rot = self.rotation(i);
        if let Some(old) = self.nodes[i].parent {
            self.nodes[old].children.retain(|&c| c != i);
        }
        self.nodes[i].parent = Some(parent);
        self.nodes[parent].children.push(i);
        self.set_position(i, at);
        self.set_rotation(i, rot);
    }

    pub fn find(&self, name: &str) -> Option<usize> {
        self.by_name.get(name)?.iter().copied().find(|&i| self.active_in_hierarchy(i))
    }

    pub fn find_any(&self, name: &str) -> Vec<usize> {
        self.by_name.get(name).cloned().unwrap_or_default()
    }

    pub fn with_tag(&self, tag: &str) -> Option<usize> {
        (0..self.nodes.len()).find(|&i| &*self.nodes[i].tag == tag && self.active_in_hierarchy(i))
    }

    pub fn child(&self, i: usize, k: usize) -> usize {
        self.nodes[i].children[k]
    }

    pub fn child_named(&self, i: usize, name: &str) -> Option<usize> {
        self.nodes[i].children.iter().copied().find(|&c| &*self.nodes[c].name == name)
    }

    pub fn descendants(&self, i: usize, out: &mut Vec<usize>) {
        out.push(i);
        for k in 0..self.nodes[i].children.len() {
            let c = self.nodes[i].children[k];
            self.descendants(c, out);
        }
    }

    pub fn under(&self, i: usize, ancestor: usize) -> bool {
        let mut c = Some(i);
        while let Some(n) = c {
            if n == ancestor {
                return true;
            }
            c = self.nodes[n].parent;
        }
        false
    }

    pub fn local_matrix(&self, i: usize) -> Mat4 {
        let n = &self.nodes[i];
        Mat4::trs(n.lpos, n.lrot, n.lscale)
    }

    /// Every node's world matrix and active-in-hierarchy flag in one top-down
    /// pass (`matrix` and `active_in_hierarchy` walk the parent chain per call).
    pub fn world_cache(&self) -> (Vec<Mat4>, Vec<bool>) {
        let n = self.nodes.len();
        let mut m = vec![Mat4::IDENTITY; n];
        let mut act = vec![false; n];
        let mut seen = vec![false; n];
        // inactive branches are skipped entirely (their matrices stay identity:
        // callers only use matrices of active nodes)
        let mut stack: Vec<usize> = (0..n).filter(|&i| self.nodes[i].parent.is_none()).collect();
        for &r in &stack {
            seen[r] = true;
            act[r] = self.nodes[r].active;
            if act[r] {
                m[r] = self.local_matrix(r);
            }
        }
        stack.retain(|&r| act[r]);
        while let Some(p) = stack.pop() {
            for &c in &self.nodes[p].children {
                if c >= n || seen[c] || self.nodes[c].parent != Some(p) {
                    continue;
                }
                seen[c] = true;
                if !self.nodes[c].active {
                    continue;
                }
                m[c] = m[p].mul(&self.local_matrix(c));
                act[c] = true;
                stack.push(c);
            }
        }
        // anything the child lists missed (descendants of skipped inactive
        // branches are simply inactive)
        for i in 0..n {
            if !seen[i] && self.active_in_hierarchy(i) {
                m[i] = self.matrix(i);
                act[i] = true;
            }
        }
        (m, act)
    }

    pub fn matrix(&self, i: usize) -> Mat4 {
        let local = self.local_matrix(i);
        match self.nodes[i].parent {
            Some(p) => self.matrix(p).mul(&local),
            None => local,
        }
    }

    pub fn position(&self, i: usize) -> V3 {
        match self.nodes[i].parent {
            Some(p) => self.matrix(p).point(self.nodes[i].lpos),
            None => self.nodes[i].lpos,
        }
    }

    pub fn rotation(&self, i: usize) -> Quat {
        match self.nodes[i].parent {
            Some(p) => self.rotation(p) * self.nodes[i].lrot,
            None => self.nodes[i].lrot,
        }
    }

    pub fn lossy_scale(&self, i: usize) -> V3 {
        let s = self.nodes[i].lscale;
        match self.nodes[i].parent {
            Some(p) => self.lossy_scale(p).mul_elem(s),
            None => s,
        }
    }

    pub fn set_position(&mut self, i: usize, p: V3) {
        let local = match self.nodes[i].parent {
            Some(par) => self.matrix(par).inverse_affine().point(p),
            None => p,
        };
        self.nodes[i].lpos = local;
    }

    pub fn set_rotation(&mut self, i: usize, q: Quat) {
        let local = match self.nodes[i].parent {
            Some(par) => self.rotation(par).inverse() * q,
            None => q,
        };
        self.nodes[i].lrot = local.normalized();
    }

    pub fn translate_world(&mut self, i: usize, d: V3) {
        let p = self.position(i) + d;
        self.set_position(i, p);
    }

    /// Transform.Rotate(x, y, z) in local space.
    pub fn rotate_self(&mut self, i: usize, x: f32, y: f32, z: f32) {
        let n = &mut self.nodes[i];
        n.lrot = (n.lrot * Quat::euler(x, y, z)).normalized();
    }

    /// eulerAngles setter (world).
    pub fn set_euler(&mut self, i: usize, x: f32, y: f32, z: f32) {
        self.set_rotation(i, Quat::euler(x, y, z));
    }

    /// Z euler angle of the world rotation (2D heading).
    pub fn angle_z(&self, i: usize) -> f32 {
        let r = self.rotation(i);
        let x = r.rotate(v3(1.0, 0.0, 0.0));
        x.y.atan2(x.x).to_degrees()
    }
}
