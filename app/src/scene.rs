//! Immersive 3D mode (opt-in): a carousel of floating consoles, then the
//! system's shelf of 3D boxes (cover-flow). Painted with OpenGL through the
//! same rendering notifier as the box viewer (`box3d`), inside the region the
//! Slint HUD leaves free. Slint owns the text (titles, hints, filter chips),
//! keyboard focus and clicks; this module owns layout, animation and drawing.
//!
//! Console representation, plug and play:
//! * `models/<system>.glb` (or `.gltf`) in the data dir → the real model,
//!   normalised (centred, scaled) at load time; optional `<system>.json`
//!   sidecar with `yaw`, `pitch`, `roll` (degrees) and `scale` tweaks.
//! * Otherwise a floating dark plaque with the system's white logo.

use crate::box3d::{bytemuck_cast, compile, cube, mul, perspective, rot_x, rot_y, translate, upload, Tex, FS, M4, VS};
use glow::HasContext;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;

pub struct SysEntry {
    pub id: String,
    pub name: String,
    pub accent: [f32; 3],
    pub count: usize,
}

pub struct BoxEntry {
    pub id: String,
    pub title: String,
    pub aspect: f32,
    pub depth: f32,
    pub glossy: bool,
    pub accent: [f32; 3],
    pub installed: bool,
    pub cover_path: Option<PathBuf>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Phase {
    Carousel,
    /// 0 → 1: consoles fly away, boxes rise.
    ToShelf(f32),
    Shelf,
    /// 0 → 1: boxes sink, consoles come back.
    ToCarousel(f32),
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Pick {
    None,
    System(usize),
    Box(usize),
}

pub struct SceneState {
    pub visible: bool,
    /// Region in physical pixels (x, y from top-left, w, h).
    pub region: (f32, f32, f32, f32),
    pub systems: Vec<SysEntry>,
    pub sys_target: usize,
    pub sys_pos: f32,
    pub boxes: Vec<BoxEntry>,
    pub box_target: usize,
    pub box_pos: f32,
    pub phase: Phase,
    /// Yaw of the focused console (slow showcase spin).
    pub spin: f32,
    pub t: f32,
    pub models_dir: PathBuf,
    /// Bump to make the renderer forget its models/logos (e.g. new files).
    pub assets_gen: u32,
    /// Set by `tick` when the HUD text should be refreshed.
    pub hud_dirty: bool,
}

impl Default for SceneState {
    fn default() -> Self {
        Self {
            visible: false,
            region: (0.0, 0.0, 0.0, 0.0),
            systems: Vec::new(),
            sys_target: 0,
            sys_pos: 0.0,
            boxes: Vec::new(),
            box_target: 0,
            box_pos: 0.0,
            phase: Phase::Carousel,
            spin: 0.0,
            t: 0.0,
            models_dir: PathBuf::new(),
            assets_gen: 0,
            hud_dirty: true,
        }
    }
}

pub type Shared = Rc<RefCell<SceneState>>;

const TRANSITION_SECS: f32 = 0.55;

impl SceneState {
    pub fn in_shelf(&self) -> bool {
        matches!(self.phase, Phase::Shelf | Phase::ToShelf(_))
    }

    /// Advances animation by `dt` seconds. Returns true while something moves
    /// (callers keep redrawing anyway; this is informational).
    pub fn tick(&mut self, dt: f32) -> bool {
        self.t += dt;
        match std::env::var("FREEPORT_DEBUG_SPIN").ok().and_then(|v| v.parse::<f32>().ok()) {
            Some(fixed) => self.spin = fixed, // dev aid: freeze the console angle
            None => self.spin += dt * 0.45,
        }
        let st = self.sys_target as f32;
        let bt = self.box_target as f32;
        let before = (self.sys_pos, self.box_pos, self.phase);
        self.sys_pos += (st - self.sys_pos) * (1.0 - (-dt * 9.0).exp());
        if (st - self.sys_pos).abs() < 0.002 {
            self.sys_pos = st;
        }
        self.box_pos += (bt - self.box_pos) * (1.0 - (-dt * 10.0).exp());
        if (bt - self.box_pos).abs() < 0.002 {
            self.box_pos = bt;
        }
        match self.phase {
            Phase::ToShelf(t) => {
                let t = t + dt / TRANSITION_SECS;
                self.phase = if t >= 1.0 { Phase::Shelf } else { Phase::ToShelf(t) };
            }
            Phase::ToCarousel(t) => {
                let t = t + dt / TRANSITION_SECS;
                self.phase = if t >= 1.0 { Phase::Carousel } else { Phase::ToCarousel(t) };
            }
            _ => {}
        }
        before != (self.sys_pos, self.box_pos, self.phase) || true
    }

    pub fn focus_system(&mut self, i: usize) {
        if self.systems.is_empty() {
            return;
        }
        let i = i.min(self.systems.len() - 1);
        if i != self.sys_target {
            self.sys_target = i;
            self.hud_dirty = true;
        }
    }

    pub fn move_system(&mut self, delta: i32) {
        let n = self.systems.len() as i32;
        if n == 0 {
            return;
        }
        let i = (self.sys_target as i32 + delta).clamp(0, n - 1);
        self.focus_system(i as usize);
    }

    pub fn focus_box(&mut self, i: usize) {
        if self.boxes.is_empty() {
            return;
        }
        let i = i.min(self.boxes.len() - 1);
        if i != self.box_target {
            self.box_target = i;
            self.hud_dirty = true;
        }
    }

    pub fn move_box(&mut self, delta: i32) {
        let n = self.boxes.len() as i32;
        if n == 0 {
            return;
        }
        let i = (self.box_target as i32 + delta).clamp(0, n - 1);
        self.focus_box(i as usize);
    }

    /// Starts the carousel → shelf transition (boxes must be filled already).
    pub fn enter_shelf(&mut self) {
        if matches!(self.phase, Phase::Carousel | Phase::ToCarousel(_)) {
            self.phase = Phase::ToShelf(0.0);
            self.box_pos = self.box_target as f32;
            self.hud_dirty = true;
        }
    }

    pub fn leave_shelf(&mut self) {
        if matches!(self.phase, Phase::Shelf | Phase::ToShelf(_)) {
            self.phase = Phase::ToCarousel(0.0);
            self.hud_dirty = true;
        }
    }

    pub fn current_system(&self) -> Option<&SysEntry> {
        self.systems.get(self.sys_target)
    }

    pub fn current_box(&self) -> Option<&BoxEntry> {
        self.boxes.get(self.box_target)
    }
}

// ───────────────────────── layout (shared by draw + pick) ─────────────────────────

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// How far the scene is into the shelf (0 = carousel, 1 = shelf).
fn shelf_mix(phase: Phase) -> f32 {
    match phase {
        Phase::Carousel => 0.0,
        Phase::Shelf => 1.0,
        Phase::ToShelf(t) => smooth(t),
        Phase::ToCarousel(t) => 1.0 - smooth(t),
    }
}

struct Placed {
    index: usize,
    /// Placement without the object's own yaw (see `yaw`/`focus`).
    model: M4,
    /// Brightness multiplier (focus/distance/transition fade).
    dim: f32,
    /// Overall transparency driver (0 = skip drawing).
    alpha: f32,
    /// Inward turn of unfocused items; the renderer adds the focused item's
    /// own motion (full spin for a model, gentle sway for a plaque).
    yaw: f32,
    /// 1 = in focus, 0 = neighbour.
    focus: f32,
}

const CAM_Z_CAROUSEL: f32 = 6.2;
const CAM_Z_SHELF: f32 = 4.4;

fn scale_m(s: f32) -> M4 {
    [s, 0.0, 0.0, 0.0, 0.0, s, 0.0, 0.0, 0.0, 0.0, s, 0.0, 0.0, 0.0, 0.0, 1.0]
}

fn scale3(x: f32, y: f32, z: f32) -> M4 {
    [x, 0.0, 0.0, 0.0, 0.0, y, 0.0, 0.0, 0.0, 0.0, z, 0.0, 0.0, 0.0, 0.0, 1.0]
}

/// Console placement on the carousel: focused one centred and spinning,
/// neighbours smaller, dimmer and turned slightly inwards.
fn layout_systems(st: &SceneState) -> Vec<Placed> {
    let mix = shelf_mix(st.phase);
    if mix >= 1.0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (i, _) in st.systems.iter().enumerate() {
        let d = i as f32 - st.sys_pos;
        if d.abs() > 2.8 {
            continue;
        }
        let ad = d.abs().min(1.0);
        let x = d * 2.5;
        let scale = 1.0 - 0.42 * ad;
        let yaw = -d.signum() * 0.45 * ad;
        let bob = (st.t * 1.1 + i as f32 * 1.7).sin() * 0.04;
        // Transition: the focused console grows towards the camera and fades;
        // the others slide away sideways.
        let fly_z = mix * 3.0 * (1.0 - ad);
        let fly_x = mix * d.signum() * 3.0 * ad;
        let model = mul(
            mul(translate(x + fly_x, bob + 0.05, fly_z), scale_m(scale * (1.0 + 0.3 * mix))),
            rot_x(0.42),
        );
        out.push(Placed { index: i, model, dim: (1.0 - 0.5 * ad) * (1.0 - mix), alpha: 1.0 - mix, yaw, focus: 1.0 - ad });
    }
    out
}

/// Cover-flow placement of the boxes of the current shelf.
fn layout_boxes(st: &SceneState) -> Vec<Placed> {
    let mix = shelf_mix(st.phase);
    if mix <= 0.0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (j, b) in st.boxes.iter().enumerate() {
        let d = j as f32 - st.box_pos;
        if d.abs() > 8.5 {
            continue;
        }
        let ad = d.abs();
        let side = d.signum();
        // Wide boxes (N64) need more room in the centre.
        let w = b.aspect.max(0.5);
        let gap = 0.7 + 0.7 * w; // focused ↔ first neighbour
        let step = 0.4 + 0.18 * w; // between turned neighbours
        let x = if ad < 1.0 { d * gap } else { side * (gap + (ad - 1.0) * step) };
        let z = -ad.min(1.0) * 1.1 - (ad - 1.0).max(0.0) * 0.08;
        let yaw = -side * ad.min(1.0) * 1.05;
        let rise = (1.0 - mix) * -2.6;
        let bob = if ad < 0.5 { (st.t * 1.3).sin() * 0.02 } else { 0.0 };
        let model = mul(translate(x, rise + bob, z), rot_y(yaw));
        out.push(Placed { index: j, model, dim: (1.0 - 0.45 * ad.min(1.0)) * mix, alpha: mix, yaw: 0.0, focus: 1.0 - ad.min(1.0) });
    }
    // Draw far-to-near so the focused box wins overlaps even without depth tricks.
    out.sort_by(|a, b| {
        let da = (a.index as f32 - st.box_pos).abs();
        let db = (b.index as f32 - st.box_pos).abs();
        db.partial_cmp(&da).unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

fn camera(st: &SceneState, aspect_vp: f32) -> (M4, M4) {
    let mix = shelf_mix(st.phase);
    let cam_z = CAM_Z_CAROUSEL + (CAM_Z_SHELF - CAM_Z_CAROUSEL) * mix;
    let proj = perspective(30f32.to_radians(), aspect_vp.max(0.2), 0.1, 40.0);
    let view = translate(0.0, 0.0, -cam_z);
    (proj, view)
}

fn project(mvp: &M4, p: [f32; 3]) -> Option<(f32, f32)> {
    let x = mvp[0] * p[0] + mvp[4] * p[1] + mvp[8] * p[2] + mvp[12];
    let y = mvp[1] * p[0] + mvp[5] * p[1] + mvp[9] * p[2] + mvp[13];
    let w = mvp[3] * p[0] + mvp[7] * p[1] + mvp[11] * p[2] + mvp[15];
    if w <= 0.0001 {
        return None;
    }
    Some((x / w, y / w))
}

/// Screen-space bounding box (NDC) of a unit-ish cuboid under `mvp`.
fn ndc_bounds(mvp: &M4, hw: f32, hh: f32, hd: f32) -> Option<(f32, f32, f32, f32)> {
    let mut bb = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for sx in [-1.0, 1.0] {
        for sy in [-1.0, 1.0] {
            for sz in [-1.0, 1.0] {
                let (x, y) = project(mvp, [sx * hw, sy * hh, sz * hd])?;
                bb.0 = bb.0.min(x);
                bb.1 = bb.1.min(y);
                bb.2 = bb.2.max(x);
                bb.3 = bb.3.max(y);
            }
        }
    }
    Some(bb)
}

/// What sits under a click at (`px`, `py`) physical pixels (window coords).
pub fn pick(st: &SceneState, px: f32, py: f32) -> Pick {
    let (rx, ry, rw, rh) = st.region;
    if rw < 4.0 || rh < 4.0 {
        return Pick::None;
    }
    let nx = (px - rx) / rw * 2.0 - 1.0;
    let ny = 1.0 - (py - ry) / rh * 2.0;
    let (proj, view) = camera(st, rw / rh);
    let pv = mul(proj, view);
    let inside = |bb: (f32, f32, f32, f32)| nx >= bb.0 && nx <= bb.2 && ny >= bb.1 && ny <= bb.3;
    if st.in_shelf() {
        // Nearest to the focus wins among overlapping cover-flow boxes.
        let mut best: Option<(f32, usize)> = None;
        for p in layout_boxes(st) {
            let b = &st.boxes[p.index];
            let mvp = mul(pv, p.model);
            if let Some(bb) = ndc_bounds(&mvp, b.aspect.max(0.2) / 2.0, 0.5, b.depth.max(0.02) / 2.0) {
                if inside(bb) {
                    let dist = (p.index as f32 - st.box_pos).abs();
                    if best.map(|(d, _)| dist < d).unwrap_or(true) {
                        best = Some((dist, p.index));
                    }
                }
            }
        }
        best.map(|(_, i)| Pick::Box(i)).unwrap_or(Pick::None)
    } else {
        let mut best: Option<(f32, usize)> = None;
        for p in layout_systems(st) {
            let mvp = mul(pv, mul(p.model, rot_y(p.yaw)));
            // Generous hit box around the console/plaque.
            if let Some(bb) = ndc_bounds(&mvp, 1.0, 0.7, 0.6) {
                if inside(bb) {
                    let dist = (p.index as f32 - st.sys_pos).abs();
                    if best.map(|(d, _)| dist < d).unwrap_or(true) {
                        best = Some((dist, p.index));
                    }
                }
            }
        }
        best.map(|(_, i)| Pick::System(i)).unwrap_or(Pick::None)
    }
}

// ───────────────────────── models (glTF) ─────────────────────────

struct Prim {
    vbo: glow::Buffer,
    count: i32,
    tex: Option<Tex>,
    base: [f32; 3],
    gloss: f32,
}

struct Model {
    prims: Vec<Prim>,
    /// Normalisation (centre + scale + user tweaks), applied before placement.
    fix: M4,
}

#[derive(Default, serde::Deserialize)]
struct Tweak {
    #[serde(default)]
    yaw: f32,
    #[serde(default)]
    pitch: f32,
    #[serde(default)]
    roll: f32,
    #[serde(default)]
    scale: Option<f32>,
}

fn rot_z(a: f32) -> M4 {
    let (s, c) = a.sin_cos();
    [c, s, 0.0, 0.0, -s, c, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]
}

fn xform(m: &[[f32; 4]; 4], p: [f32; 3]) -> [f32; 3] {
    let mut r = [0.0; 3];
    for i in 0..3 {
        r[i] = m[0][i] * p[0] + m[1][i] * p[1] + m[2][i] * p[2] + m[3][i];
    }
    r
}

fn xform_n(m: &[[f32; 4]; 4], n: [f32; 3]) -> [f32; 3] {
    let mut r = [0.0; 3];
    for i in 0..3 {
        r[i] = m[0][i] * n[0] + m[1][i] * n[1] + m[2][i] * n[2];
    }
    let l = (r[0] * r[0] + r[1] * r[1] + r[2] * r[2]).sqrt().max(1e-6);
    [r[0] / l, r[1] / l, r[2] / l]
}

fn mat_mul4(a: &[[f32; 4]; 4], b: &[[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut r = [[0.0; 4]; 4];
    for c in 0..4 {
        for rr in 0..4 {
            r[c][rr] = (0..4).map(|k| a[k][rr] * b[c][k]).sum();
        }
    }
    r
}

/// Flattened primitive data ready for upload: interleaved pos/nrm/uv.
struct RawPrim {
    verts: Vec<f32>,
    image: Option<usize>,
    base: [f32; 3],
}

fn load_gltf(path: &std::path::Path) -> Result<(Vec<RawPrim>, Vec<gltf::image::Data>, [f32; 3], f32), String> {
    let (doc, buffers, images) = gltf::import(path).map_err(|e| e.to_string())?;
    let mut prims = Vec::new();
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    let scene = doc.default_scene().or_else(|| doc.scenes().next()).ok_or("sin escena")?;
    let ident = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
    let mut stack: Vec<(gltf::Node, [[f32; 4]; 4])> = scene.nodes().map(|n| (n, ident)).collect();
    while let Some((node, parent)) = stack.pop() {
        let m = mat_mul4(&parent, &node.transform().matrix());
        for child in node.children() {
            stack.push((child, m));
        }
        let Some(mesh) = node.mesh() else { continue };
        for prim in mesh.primitives() {
            if prim.mode() != gltf::mesh::Mode::Triangles {
                continue;
            }
            let reader = prim.reader(|b| buffers.get(b.index()).map(|d| d.0.as_slice()));
            let Some(pos) = reader.read_positions() else { continue };
            let pos: Vec<[f32; 3]> = pos.collect();
            let nrm: Option<Vec<[f32; 3]>> = reader.read_normals().map(|n| n.collect());
            let uv: Option<Vec<[f32; 2]>> = reader.read_tex_coords(0).map(|t| t.into_f32().collect());
            let idx: Vec<u32> = match reader.read_indices() {
                Some(i) => i.into_u32().collect(),
                None => (0..pos.len() as u32).collect(),
            };
            let mat = prim.material();
            let pbr = mat.pbr_metallic_roughness();
            let bc = pbr.base_color_factor();
            let image = pbr.base_color_texture().map(|t| t.texture().source().index());
            let mut verts = Vec::with_capacity(idx.len() * 8);
            for tri in idx.chunks_exact(3) {
                let p: Vec<[f32; 3]> = tri.iter().map(|&i| xform(&m, pos[i as usize])).collect();
                let flat = {
                    let u = [p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]];
                    let v = [p[2][0] - p[0][0], p[2][1] - p[0][1], p[2][2] - p[0][2]];
                    let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
                    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-6);
                    [n[0] / l, n[1] / l, n[2] / l]
                };
                for (k, &i) in tri.iter().enumerate() {
                    let i = i as usize;
                    for c in 0..3 {
                        lo[c] = lo[c].min(p[k][c]);
                        hi[c] = hi[c].max(p[k][c]);
                    }
                    verts.extend_from_slice(&p[k]);
                    let n = nrm.as_ref().and_then(|n| n.get(i)).map(|n| xform_n(&m, *n)).unwrap_or(flat);
                    verts.extend_from_slice(&n);
                    let t = uv.as_ref().and_then(|u| u.get(i)).copied().unwrap_or([0.0, 0.0]);
                    verts.extend_from_slice(&t);
                }
            }
            prims.push(RawPrim { verts, image, base: [bc[0], bc[1], bc[2]] });
        }
    }
    if prims.is_empty() || lo[0] > hi[0] {
        return Err("sin geometría".into());
    }
    let center = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0];
    let extent = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(hi[2] - lo[2]).max(1e-4);
    Ok((prims, images, center, extent))
}

fn image_rgba(img: &gltf::image::Data) -> Option<(u32, u32, Vec<u8>)> {
    use gltf::image::Format::*;
    let (w, h) = (img.width, img.height);
    let px = match img.format {
        R8G8B8A8 => img.pixels.clone(),
        R8G8B8 => img.pixels.chunks_exact(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
        R8 => img.pixels.iter().flat_map(|&v| [v, v, v, 255]).collect(),
        R8G8 => img.pixels.chunks_exact(2).flat_map(|c| [c[0], c[0], c[0], 255]).collect(),
        R16G16B16A16 => img.pixels.chunks_exact(8).flat_map(|c| [c[1], c[3], c[5], c[7]]).collect(),
        R16G16B16 => img.pixels.chunks_exact(6).flat_map(|c| [c[1], c[3], c[5], 255]).collect(),
        _ => return None,
    };
    Some((w, h, px))
}

/// Texture upload without filtering or mipmaps (pixel art).
fn upload_nearest(gl: &glow::Context, w: u32, h: u32, rgba: &[u8]) -> Result<Tex, String> {
    unsafe {
        let t = gl.create_texture()?;
        gl.bind_texture(glow::TEXTURE_2D, Some(t));
        gl.tex_image_2d(glow::TEXTURE_2D, 0, glow::RGBA as i32, w as i32, h as i32, 0, glow::RGBA, glow::UNSIGNED_BYTE, glow::PixelUnpackData::Slice(Some(rgba)));
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::NEAREST as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::NEAREST as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
        Ok(Tex(t))
    }
}

// ───────────────────────── logo plaques ─────────────────────────

/// Dark rounded plate with the system's white logo centred: the fallback
/// representation when no model is installed for a system.
pub fn plaque_pixels(svg: &[u8], accent: [f32; 3]) -> Option<(u32, u32, Vec<u8>)> {
    use resvg::tiny_skia;
    use resvg::usvg;
    let (w, h) = (768u32, 384u32);
    let tree = usvg::Tree::from_data(svg, &usvg::Options::default()).ok()?;
    let mut pm = tiny_skia::Pixmap::new(w, h)?;
    pm.fill(tiny_skia::Color::from_rgba8(0x15, 0x18, 0x21, 255));
    // Accent strip along the bottom edge.
    let mut paint = tiny_skia::Paint::default();
    paint.set_color_rgba8((accent[0] * 255.0) as u8, (accent[1] * 255.0) as u8, (accent[2] * 255.0) as u8, 255);
    if let Some(r) = tiny_skia::Rect::from_xywh(0.0, h as f32 - 14.0, w as f32, 14.0) {
        pm.fill_rect(r, &paint, tiny_skia::Transform::identity(), None);
    }
    let size = tree.size();
    let s = (w as f32 * 0.72 / size.width()).min(h as f32 * 0.5 / size.height());
    let tx = (w as f32 - size.width() * s) / 2.0;
    let ty = (h as f32 - 14.0 - size.height() * s) / 2.0;
    resvg::render(&tree, tiny_skia::Transform::from_scale(s, s).post_translate(tx, ty), &mut pm.as_mut());
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for p in pm.pixels() {
        let c = p.demultiply();
        out.extend_from_slice(&[c.red(), c.green(), c.blue(), 255]);
    }
    Some((w, h, out))
}

// ───────────────────────── renderer ─────────────────────────

const BG_VS: &str = r#"
attribute vec2 a_pos; varying vec2 v_p;
void main() { v_p = a_pos; gl_Position = vec4(a_pos, 0.0, 1.0); }"#;

const BG_FS: &str = r#"
#ifdef GL_ES
precision mediump float;
#endif
varying vec2 v_p; uniform vec3 u_accent; uniform float u_glow; uniform vec2 u_center;
void main() {
  vec3 top = vec3(0.055, 0.062, 0.085);
  vec3 bottom = vec3(0.027, 0.031, 0.047);
  vec3 col = mix(bottom, top, (v_p.y + 1.0) * 0.5);
  float d = length((v_p - u_center) * vec2(1.0, 1.6));
  col += u_accent * smoothstep(1.2, 0.0, d) * u_glow;
  // Soft floor: slightly lighter band fading downwards, like a lit stage.
  float fl = smoothstep(-0.55, -0.62, v_p.y) * smoothstep(-1.0, -0.62, v_p.y);
  col += vec3(0.05, 0.055, 0.075) * fl;
  // Vignette.
  col *= 1.0 - 0.35 * smoothstep(0.6, 1.5, length(v_p));
  gl_FragColor = vec4(col, 1.0);
}"#;

pub struct SceneRenderer {
    gl: Rc<glow::Context>,
    prog: glow::Program,
    bg: glow::Program,
    bg_vbo: glow::Buffer,
    vbo: glow::Buffer,
    white: Tex,
    side: Tex,
    spine: Tex,
    covers: HashMap<String, Tex>,
    cover_missing: HashSet<String>,
    plaques: HashMap<String, Tex>,
    models: HashMap<String, Model>,
    model_tried: HashSet<String>,
    assets_gen: u32,
    logos: Vec<(String, Vec<u8>)>,
}

impl SceneRenderer {
    pub fn new(gl: Rc<glow::Context>, logos: &[(&str, &[u8])]) -> Result<Self, String> {
        unsafe {
            let prog = compile(&gl, VS, FS)?;
            let bg = compile(&gl, BG_VS, BG_FS)?;
            let bg_vbo = gl.create_buffer()?;
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(bg_vbo));
            let quad: [f32; 12] = [-1.0, -1.0, 1.0, -1.0, 1.0, 1.0, -1.0, -1.0, 1.0, 1.0, -1.0, 1.0];
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytemuck_cast(&quad), glow::STATIC_DRAW);
            let vbo = gl.create_buffer()?;
            let white = upload(&gl, 1, 1, &[255, 255, 255, 255])?;
            let side = upload(&gl, 1, 1, &[214, 216, 222, 255])?;
            let spine = {
                let (w, h) = (16u32, 128u32);
                let mut px = Vec::with_capacity((w * h * 4) as usize);
                for y in 0..h {
                    for x in 0..w {
                        let edge = if x == 0 || x == w - 1 { 0.75 } else { 1.0 };
                        let v = (0.78 + 0.22 * (1.0 - y as f32 / h as f32)) * edge;
                        let c = (v * 255.0) as u8;
                        px.extend_from_slice(&[c, c, c, 255]);
                    }
                }
                upload(&gl, w, h, &px)?
            };
            Ok(Self {
                gl,
                prog,
                bg,
                bg_vbo,
                vbo,
                white,
                side,
                spine,
                covers: HashMap::new(),
                cover_missing: HashSet::new(),
                plaques: HashMap::new(),
                models: HashMap::new(),
                model_tried: HashSet::new(),
                assets_gen: 0,
                logos: logos.iter().map(|(k, v)| (k.to_string(), v.to_vec())).collect(),
            })
        }
    }

    /// Number of systems with a real model loaded (for the settings text).
    #[allow(dead_code)]
    pub fn models_loaded(&self) -> usize {
        self.models.len()
    }

    fn ensure_model(&mut self, st: &SceneState, sys: &str) {
        if self.models.contains_key(sys) || self.model_tried.contains(sys) {
            return;
        }
        self.model_tried.insert(sys.to_string());
        let candidates = [
            st.models_dir.join(format!("{sys}.glb")),
            st.models_dir.join(format!("{sys}.gltf")),
            st.models_dir.join(sys).join("scene.gltf"), // Sketchfab zip layout
            st.models_dir.join(sys).join("scene.glb"),
        ];
        let Some(path) = candidates.iter().find(|p| p.exists()) else {
            // No file: the built-in low-poly console.
            if let Some(parts) = crate::consoles::build(sys) {
                let (lo, hi) = crate::consoles::bounds(&parts);
                let center = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0];
                let extent = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(hi[2] - lo[2]).max(1e-4);
                let fix = mul(scale_m(2.1 / extent), translate(-center[0], -center[1], -center[2]));
                let gl = &self.gl;
                let mut out = Vec::new();
                unsafe {
                    for p in parts {
                        let Ok(vbo) = gl.create_buffer() else { continue };
                        gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
                        gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytemuck_cast(&p.verts), glow::STATIC_DRAW);
                        // Pixel-art skins: nearest filtering so the texels stay crisp.
                        let tex = p.tex.as_ref().and_then(|c| upload_nearest(gl, c.w, c.h, &c.px).ok());
                        out.push(Prim { vbo, count: (p.verts.len() / 8) as i32, tex, base: p.color, gloss: p.gloss });
                    }
                }
                self.models.insert(sys.to_string(), Model { prims: out, fix });
            }
            return;
        };
        let started = std::time::Instant::now();
        match load_gltf(path) {
            Ok((prims, images, center, extent)) => {
                let tweak: Tweak = std::fs::read(st.models_dir.join(format!("{sys}.json")))
                    .ok()
                    .and_then(|b| serde_json::from_slice(&b).ok())
                    .unwrap_or_default();
                let s = tweak.scale.unwrap_or(1.0) * 1.7 / extent;
                let fix = mul(
                    mul(scale_m(s), mul(rot_y(tweak.yaw.to_radians()), mul(rot_x(tweak.pitch.to_radians()), rot_z(tweak.roll.to_radians())))),
                    translate(-center[0], -center[1], -center[2]),
                );
                let gl = &self.gl;
                let mut tex_cache: HashMap<usize, glow::Texture> = HashMap::new();
                let mut out = Vec::new();
                unsafe {
                    for p in prims {
                        let Ok(vbo) = gl.create_buffer() else { continue };
                        gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
                        gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytemuck_cast(&p.verts), glow::STATIC_DRAW);
                        let tex = p.image.and_then(|i| {
                            if let Some(t) = tex_cache.get(&i) {
                                return Some(Tex(*t));
                            }
                            let (w, h, px) = image_rgba(images.get(i)?)?;
                            let t = upload(gl, w, h, &px).ok()?;
                            tex_cache.insert(i, t.0);
                            Some(t)
                        });
                        out.push(Prim { vbo, count: (p.verts.len() / 8) as i32, tex, base: p.base, gloss: 0.35 });
                    }
                }
                eprintln!(
                    "[freeport] 3D: modelo {sys} cargado ({} prim., {:.0} ms)",
                    out.len(),
                    started.elapsed().as_secs_f32() * 1000.0
                );
                self.models.insert(sys.to_string(), Model { prims: out, fix });
            }
            Err(e) => eprintln!("[freeport] 3D: modelo {sys}: {e}"),
        }
    }

    fn ensure_plaque(&mut self, sys: &str, accent: [f32; 3]) {
        if self.plaques.contains_key(sys) {
            return;
        }
        let svg = self.logos.iter().find(|(k, _)| k == sys).map(|(_, v)| v.clone());
        let px = svg.and_then(|s| plaque_pixels(&s, accent)).or_else(|| {
            // No logo: plain plate.
            let (w, h) = (4u32, 2u32);
            Some((w, h, vec![0x15, 0x18, 0x21, 255].repeat((w * h) as usize)))
        });
        if let Some((w, h, px)) = px {
            if let Ok(t) = upload(&self.gl, w, h, &px) {
                self.plaques.insert(sys.to_string(), t);
            }
        }
    }

    /// Loads at most `budget` cover textures near the focus (disk → GPU).
    fn ensure_covers(&mut self, st: &SceneState, budget: usize) {
        let mut n = 0;
        let mut order: Vec<usize> = (0..st.boxes.len()).collect();
        order.sort_by_key(|&j| ((j as f32 - st.box_pos).abs() * 100.0) as i64);
        for j in order.into_iter().take(14) {
            let b = &st.boxes[j];
            if self.covers.contains_key(&b.id) || self.cover_missing.contains(&b.id) {
                continue;
            }
            let Some(path) = b.cover_path.as_ref() else {
                self.cover_missing.insert(b.id.clone());
                continue;
            };
            match freeport_core::thumbs::load_rgba(path, 512) {
                Some((w, h, px)) => {
                    if let Ok(t) = upload(&self.gl, w, h, &px) {
                        self.covers.insert(b.id.clone(), t);
                    }
                }
                None => {
                    self.cover_missing.insert(b.id.clone());
                }
            }
            n += 1;
            if n >= budget {
                break;
            }
        }
    }

    /// Forget covers of a previous shelf (called when the shelf is refilled).
    pub fn drop_covers(&mut self) {
        unsafe {
            for (_, t) in self.covers.drain() {
                self.gl.delete_texture(t.0);
            }
        }
        self.cover_missing.clear();
    }

    pub fn render(&mut self, st: &mut SceneState, win_w: f32, win_h: f32) {
        if st.assets_gen != self.assets_gen {
            self.assets_gen = st.assets_gen;
            self.model_tried.clear();
            unsafe {
                for (_, m) in self.models.drain() {
                    for p in m.prims {
                        self.gl.delete_buffer(p.vbo);
                        if let Some(t) = p.tex {
                            self.gl.delete_texture(t.0);
                        }
                    }
                }
            }
        }
        let (rx, ry, rw, rh) = st.region;
        if rw < 4.0 || rh < 4.0 {
            return;
        }
        // Lazy assets near the focus.
        let near: Vec<(String, [f32; 3])> = st
            .systems
            .iter()
            .enumerate()
            .filter(|(i, _)| (*i as f32 - st.sys_pos).abs() < 2.8)
            .map(|(_, s)| (s.id.clone(), s.accent))
            .collect();
        for (id, accent) in &near {
            self.ensure_model(st, id);
            self.ensure_plaque(id, *accent);
        }
        if st.in_shelf() || matches!(st.phase, Phase::ToCarousel(_)) {
            self.ensure_covers(st, 2);
        }

        let gl = self.gl.clone();
        let accent = st.current_system().map(|s| s.accent).unwrap_or([1.0, 0.7, 0.24]);
        let mix = shelf_mix(st.phase);
        unsafe {
            gl.viewport(rx as i32, (win_h - ry - rh) as i32, rw as i32, rh as i32);
            gl.enable(glow::SCISSOR_TEST);
            gl.scissor(rx as i32, (win_h - ry - rh) as i32, rw as i32, rh as i32);
            gl.disable(glow::DEPTH_TEST);
            gl.disable(glow::BLEND);
            gl.disable(glow::CULL_FACE);

            // Background gradient + accent glow behind the focus.
            gl.use_program(Some(self.bg));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.bg_vbo));
            let a = gl.get_attrib_location(self.bg, "a_pos").unwrap_or(0);
            gl.enable_vertex_attrib_array(a);
            gl.vertex_attrib_pointer_f32(a, 2, glow::FLOAT, false, 8, 0);
            gl.uniform_3_f32(gl.get_uniform_location(self.bg, "u_accent").as_ref(), accent[0], accent[1], accent[2]);
            gl.uniform_1_f32(gl.get_uniform_location(self.bg, "u_glow").as_ref(), 0.22 + 0.08 * mix);
            gl.uniform_2_f32(gl.get_uniform_location(self.bg, "u_center").as_ref(), 0.0, -0.05 + 0.15 * mix);
            gl.draw_arrays(glow::TRIANGLES, 0, 6);
            gl.disable_vertex_attrib_array(a);

            // 3D content.
            gl.enable(glow::DEPTH_TEST);
            gl.depth_func(glow::LEQUAL);
            gl.clear(glow::DEPTH_BUFFER_BIT);
            gl.enable(glow::CULL_FACE);
            gl.cull_face(glow::BACK);
            gl.use_program(Some(self.prog));
            gl.uniform_1_i32(gl.get_uniform_location(self.prog, "u_tex").as_ref(), 0);
            gl.active_texture(glow::TEXTURE0);
            let (proj, view) = camera(st, rw / rh);
            let pv = mul(proj, view);

            // Consoles.
            for p in layout_systems(st) {
                if p.alpha <= 0.02 {
                    continue;
                }
                let sys = &st.systems[p.index];
                if let Some(m) = self.models.get(&sys.id) {
                    // Real model: slow full turn while in focus.
                    let model = mul(mul(p.model, rot_y(p.yaw + p.focus * st.spin)), m.fix);
                    for prim in &m.prims {
                        self.set_mats(&pv, &model, prim.gloss);
                        gl.bind_buffer(glow::ARRAY_BUFFER, Some(prim.vbo));
                        self.attribs(true);
                        gl.bind_texture(glow::TEXTURE_2D, Some(prim.tex.as_ref().map(|t| t.0).unwrap_or(self.white.0)));
                        self.tint(prim.base, p.dim);
                        gl.draw_arrays(glow::TRIANGLES, 0, prim.count);
                    }
                } else {
                    // Plaque: wide thin slab, logo on the front, dark elsewhere.
                    // It sways instead of spinning so the logo stays readable.
                    let sway = (st.spin * 1.6).sin() * 0.28;
                    let model = mul(mul(p.model, rot_y(p.yaw + p.focus * sway)), scale_m(0.95));
                    self.set_mats(&pv, &model, 0.6);
                    let verts = cube(2.0, 0.09);
                    gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vbo));
                    gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytemuck_cast(&verts), glow::DYNAMIC_DRAW);
                    self.attribs(true);
                    let plaque = self.plaques.get(&sys.id).map(|t| t.0).unwrap_or(self.side.0);
                    for face in 0..6 {
                        if face == 0 {
                            gl.bind_texture(glow::TEXTURE_2D, Some(plaque));
                            self.tint([1.0, 1.0, 1.0], p.dim);
                        } else {
                            gl.bind_texture(glow::TEXTURE_2D, Some(self.white.0));
                            self.tint([0.11, 0.12, 0.16], p.dim);
                        }
                        gl.draw_arrays(glow::TRIANGLES, (face * 6) as i32, 6);
                    }
                }
            }

            // Boxes (cover-flow) + their faint reflection.
            let placed = layout_boxes(st);
            if !placed.is_empty() {
                gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vbo));
                for pass in 0..2 {
                    let reflect = pass == 0;
                    if reflect {
                        gl.cull_face(glow::FRONT);
                    } else {
                        gl.cull_face(glow::BACK);
                    }
                    for p in &placed {
                        if p.alpha <= 0.02 {
                            continue;
                        }
                        let b = &st.boxes[p.index];
                        let model = if reflect {
                            mul(mul(p.model, translate(0.0, -1.08, 0.0)), scale3(1.0, -1.0, 1.0))
                        } else {
                            p.model
                        };
                        if reflect && (p.index as f32 - st.box_pos).abs() > 4.5 {
                            continue;
                        }
                        let dim = if reflect { p.dim * 0.09 } else { p.dim };
                        self.set_mats(&pv, &model, if b.glossy { 1.0 } else { 0.0 });
                        let verts = cube(b.aspect.max(0.2), b.depth.max(0.02));
                        gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytemuck_cast(&verts), glow::DYNAMIC_DRAW);
                        self.attribs(true);
                        let cover = self.covers.get(&b.id).map(|t| t.0);
                        let ac = b.accent;
                        let faces: [(glow::Texture, [f32; 3], f32); 6] = [
                            (cover.unwrap_or(self.side.0), if cover.is_some() { [1.0, 1.0, 1.0] } else { [0.35, 0.37, 0.42] }, 1.0),
                            (cover.unwrap_or(self.side.0), [0.55, 0.55, 0.6], 0.8),
                            (self.spine.0, ac, 1.0),
                            (self.spine.0, ac, 1.0),
                            (self.side.0, [1.0, 1.0, 1.0], 0.9),
                            (self.side.0, [1.0, 1.0, 1.0], 0.7),
                        ];
                        for (i, (tex, tint, fdim)) in faces.iter().enumerate() {
                            gl.bind_texture(glow::TEXTURE_2D, Some(*tex));
                            self.tint(*tint, dim * fdim);
                            gl.draw_arrays(glow::TRIANGLES, (i * 6) as i32, 6);
                        }
                    }
                }
            }
            self.attribs(false);

            // Back to the state femtovg expects.
            gl.disable(glow::CULL_FACE);
            gl.disable(glow::DEPTH_TEST);
            gl.disable(glow::SCISSOR_TEST);
            gl.enable(glow::BLEND);
            gl.blend_func(glow::ONE, glow::ONE_MINUS_SRC_ALPHA);
            gl.bind_texture(glow::TEXTURE_2D, None);
            gl.bind_buffer(glow::ARRAY_BUFFER, None);
            gl.use_program(None);
            gl.viewport(0, 0, win_w as i32, win_h as i32);
        }
    }

    unsafe fn set_mats(&self, pv: &M4, model: &M4, gloss: f32) {
        let gl = &self.gl;
        let mvp = mul(*pv, *model);
        gl.uniform_matrix_4_f32_slice(gl.get_uniform_location(self.prog, "u_mvp").as_ref(), false, &mvp);
        gl.uniform_matrix_4_f32_slice(gl.get_uniform_location(self.prog, "u_model").as_ref(), false, model);
        gl.uniform_1_f32(gl.get_uniform_location(self.prog, "u_gloss").as_ref(), gloss);
    }

    unsafe fn tint(&self, c: [f32; 3], dim: f32) {
        let gl = &self.gl;
        gl.uniform_3_f32(gl.get_uniform_location(self.prog, "u_tint").as_ref(), c[0], c[1], c[2]);
        gl.uniform_1_f32(gl.get_uniform_location(self.prog, "u_dim").as_ref(), dim);
    }

    unsafe fn attribs(&self, on: bool) {
        let gl = &self.gl;
        let stride = 8 * 4;
        for (name, size, off) in [("a_pos", 3, 0), ("a_nrm", 3, 12), ("a_uv", 2, 24)] {
            if let Some(loc) = gl.get_attrib_location(self.prog, name) {
                if on {
                    gl.enable_vertex_attrib_array(loc);
                    gl.vertex_attrib_pointer_f32(loc, size, glow::FLOAT, false, stride, off);
                } else {
                    gl.disable_vertex_attrib_array(loc);
                }
            }
        }
    }
}
