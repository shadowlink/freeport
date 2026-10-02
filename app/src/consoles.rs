//! Low-poly consoles for the immersive carousel, in the "low poly + pixel art
//! texture" style: hard-edged geometry with faithful silhouettes (tapered
//! blocks, humps, handles) and small procedurally painted textures (vents,
//! ports, buttons, logos) sampled without filtering so the pixels show.
//!
//! Coordinates: centimetres, y up, +z towards the viewer (front). `scene`
//! normalises the result like any other model. Texture scale: 3 texels/cm.

use std::f32::consts::TAU;

/// A painted pixel texture (RGBA8, row 0 = top).
pub struct Canvas {
    pub w: u32,
    pub h: u32,
    pub px: Vec<u8>,
}

fn rgb(hex: u32) -> [f32; 3] {
    [((hex >> 16) & 255) as f32 / 255.0, ((hex >> 8) & 255) as f32 / 255.0, (hex & 255) as f32 / 255.0]
}

impl Canvas {
    pub fn new(w: u32, h: u32, color: u32) -> Self {
        let mut c = Self { w, h, px: vec![0; (w * h * 4) as usize] };
        c.rect(0, 0, w as i32, h as i32, color);
        c
    }
    /// Canvas sized for a face of `w_cm × h_cm` at `TEXELS_PER_CM`.
    pub fn for_face(w_cm: f32, h_cm: f32, color: u32) -> Self {
        Self::new(((w_cm * TEXELS_PER_CM).round() as u32).max(2), ((h_cm * TEXELS_PER_CM).round() as u32).max(2), color)
    }
    pub fn px(&mut self, x: i32, y: i32, color: u32) {
        if x < 0 || y < 0 || x >= self.w as i32 || y >= self.h as i32 {
            return;
        }
        let i = ((y as u32 * self.w + x as u32) * 4) as usize;
        self.px[i] = ((color >> 16) & 255) as u8;
        self.px[i + 1] = ((color >> 8) & 255) as u8;
        self.px[i + 2] = (color & 255) as u8;
        self.px[i + 3] = 255;
    }
    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: u32) {
        for yy in y..y + h {
            for xx in x..x + w {
                self.px(xx, yy, color);
            }
        }
    }
    pub fn disc(&mut self, cx: i32, cy: i32, r: i32, color: u32) {
        for yy in -r..=r {
            for xx in -r..=r {
                if xx * xx + yy * yy <= r * r {
                    self.px(cx + xx, cy + yy, color);
                }
            }
        }
    }
    /// One-texel-wide circle outline.
    pub fn ring(&mut self, cx: i32, cy: i32, r: i32, color: u32) {
        let inner = (r - 1).max(0);
        for yy in -r..=r {
            for xx in -r..=r {
                let d = xx * xx + yy * yy;
                if d <= r * r && d > inner * inner {
                    self.px(cx + xx, cy + yy, color);
                }
            }
        }
    }
    /// Horizontal vent slots: `n` lines of `w` texels, `gap` rows apart.
    pub fn vents_h(&mut self, x: i32, y: i32, w: i32, n: i32, gap: i32, color: u32) {
        for i in 0..n {
            self.rect(x, y + i * gap, w, 1, color);
        }
    }
    /// Vertical vent slots.
    pub fn vents_v(&mut self, x: i32, y: i32, h: i32, n: i32, gap: i32, color: u32) {
        for i in 0..n {
            self.rect(x + i * gap, y, 1, h, color);
        }
    }
    /// Dotted grid (speaker / mesh).
    pub fn dots(&mut self, x: i32, y: i32, cols: i32, rows: i32, gap: i32, color: u32) {
        for r in 0..rows {
            for c in 0..cols {
                self.px(x + c * gap, y + r * gap, color);
            }
        }
    }
    /// Round controller port: dark hole with a lighter rim.
    pub fn port(&mut self, cx: i32, cy: i32, r: i32, rim: u32, hole: u32) {
        self.disc(cx, cy, r, rim);
        self.disc(cx, cy, r - 1, hole);
    }
    /// Framed slot (memory cards, discs, cartridges).
    pub fn slot(&mut self, x: i32, y: i32, w: i32, h: i32, frame: u32, inner: u32) {
        self.rect(x, y, w, h, frame);
        self.rect(x + 1, y + 1, w - 2, (h - 2).max(1), inner);
    }
}

pub struct Part {
    pub color: [f32; 3],
    /// 0 = matte plastic, 1 = glossy (screens, lacquer).
    pub gloss: f32,
    /// Interleaved pos3 / nrm3 / uv2.
    pub verts: Vec<f32>,
    /// Pixel texture mapped on this part (uv 0..1), or None for flat colour.
    pub tex: Option<Canvas>,
}

/// Six faces of a tapered block (bottom rectangle `bw × bd` at `y0`, top
/// rectangle `tw × td` at `y0 + h`, centred on `cx, cz`), each as its own
/// quad with a flat normal and uv 0..1 (u left→right, v top→bottom as seen
/// from outside; the top face is seen from above with the front at the bottom).
pub struct Block {
    pub faces: [Vec<f32>; 6], // front, back, left, right, top, bottom
}

fn quad(p: [[f32; 3]; 4]) -> Vec<f32> {
    // p: BL, BR, TR, TL (CCW from outside), uv BL(0,1) BR(1,1) TR(1,0) TL(0,0).
    let u = [p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]];
    let v = [p[3][0] - p[0][0], p[3][1] - p[0][1], p[3][2] - p[0][2]];
    let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-6);
    let n = [n[0] / l, n[1] / l, n[2] / l];
    let uv = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
    let mut out = Vec::with_capacity(48);
    for i in [0, 1, 2, 0, 2, 3] {
        out.extend_from_slice(&p[i]);
        out.extend_from_slice(&n);
        out.extend_from_slice(&uv[i]);
    }
    out
}

#[allow(clippy::too_many_arguments)]
pub fn block(cx: f32, y0: f32, cz: f32, bw: f32, bd: f32, tw: f32, td: f32, h: f32) -> Block {
    let (bx, bz, tx, tz) = (bw / 2.0, bd / 2.0, tw / 2.0, td / 2.0);
    let y1 = y0 + h;
    // Corners: [-x,-z], [+x,-z], [+x,+z], [-x,+z] at the bottom (b) and top (t).
    let b = [[cx - bx, y0, cz - bz], [cx + bx, y0, cz - bz], [cx + bx, y0, cz + bz], [cx - bx, y0, cz + bz]];
    let t = [[cx - tx, y1, cz - tz], [cx + tx, y1, cz - tz], [cx + tx, y1, cz + tz], [cx - tx, y1, cz + tz]];
    Block {
        faces: [
            quad([b[3], b[2], t[2], t[3]]), // front (+z)
            quad([b[1], b[0], t[0], t[1]]), // back (-z)
            quad([b[0], b[3], t[3], t[0]]), // left (-x), u towards +z
            quad([b[2], b[1], t[1], t[2]]), // right (+x), u towards -z
            quad([t[3], t[2], t[1], t[0]]), // top: BL = front-left, TL = back-left
            quad([b[0], b[1], b[2], b[3]]), // bottom
        ],
    }
}

pub const FRONT: usize = 0;
pub const BACK: usize = 1;
pub const LEFT: usize = 2;
pub const RIGHT: usize = 3;
pub const TOP: usize = 4;

#[derive(Clone, Copy)]
pub enum Axis {
    X,
    Y,
}

/// Closed cylinder along `axis` (flat-shaded facets), untextured.
pub fn cylinder(c: [f32; 3], r: f32, h: f32, axis: Axis, seg: usize) -> Vec<f32> {
    let mut v: Vec<f32> = Vec::with_capacity(seg * 12 * 8);
    let (y0, y1) = (-h / 2.0, h / 2.0);
    let mut push = |p: [f32; 3], n: [f32; 3]| {
        v.extend_from_slice(&p);
        v.extend_from_slice(&n);
        v.extend_from_slice(&[0.0, 0.0]);
    };
    for i in 0..seg {
        let a0 = i as f32 / seg as f32 * TAU;
        let a1 = (i + 1) as f32 / seg as f32 * TAU;
        let (s0, c0) = a0.sin_cos();
        let (s1, c1) = a1.sin_cos();
        let p00 = [r * c0, y0, -r * s0];
        let p01 = [r * c1, y0, -r * s1];
        let p10 = [r * c0, y1, -r * s0];
        let p11 = [r * c1, y1, -r * s1];
        let am = (a0 + a1) / 2.0;
        let n = [am.cos(), 0.0, -am.sin()];
        push(p00, n);
        push(p01, n);
        push(p11, n);
        push(p00, n);
        push(p11, n);
        push(p10, n);
        push([0.0, y1, 0.0], [0.0, 1.0, 0.0]);
        push(p10, [0.0, 1.0, 0.0]);
        push(p11, [0.0, 1.0, 0.0]);
        push([0.0, y0, 0.0], [0.0, -1.0, 0.0]);
        push(p01, [0.0, -1.0, 0.0]);
        push(p00, [0.0, -1.0, 0.0]);
    }
    for i in (0..v.len()).step_by(8) {
        let (x, y, z) = (v[i], v[i + 1], v[i + 2]);
        let (nx, ny, nz) = (v[i + 3], v[i + 4], v[i + 5]);
        let (p, n) = match axis {
            Axis::Y => ([x, y, z], [nx, ny, nz]),
            Axis::X => ([y, x, z], [ny, nx, nz]),
        };
        v[i] = p[0] + c[0];
        v[i + 1] = p[1] + c[1];
        v[i + 2] = p[2] + c[2];
        v[i + 3] = n[0];
        v[i + 4] = n[1];
        v[i + 5] = n[2];
    }
    if matches!(axis, Axis::X) {
        // The axis swap mirrors the geometry: flip winding so culling keeps the outside.
        for t in (0..v.len()).step_by(24) {
            let (a, b) = (t + 8, t + 16);
            for k in 0..8 {
                v.swap(a + k, b + k);
            }
        }
    }
    v
}

/// Rotates `verts` around the X axis by `a` radians about `pivot`.
fn rotate_x_about(mut verts: Vec<f32>, a: f32, pivot: [f32; 3]) -> Vec<f32> {
    let (s, c) = a.sin_cos();
    for i in (0..verts.len()).step_by(8) {
        let y = verts[i + 1] - pivot[1];
        let z = verts[i + 2] - pivot[2];
        verts[i + 1] = y * c - z * s + pivot[1];
        verts[i + 2] = y * s + z * c + pivot[2];
        let ny = verts[i + 4];
        let nz = verts[i + 5];
        verts[i + 4] = ny * c - nz * s;
        verts[i + 5] = ny * s + nz * c;
    }
    verts
}

struct Builder {
    parts: Vec<Part>,
}

impl Builder {
    fn new() -> Self {
        Self { parts: Vec::new() }
    }
    fn add(&mut self, color: [f32; 3], gloss: f32, verts: Vec<f32>) -> &mut Self {
        self.parts.push(Part { color, gloss, verts, tex: None });
        self
    }
    fn add_tex(&mut self, verts: Vec<f32>, tex: Canvas, gloss: f32) -> &mut Self {
        self.parts.push(Part { color: [1.0, 1.0, 1.0], gloss, verts, tex: Some(tex) });
        self
    }
    /// Whole block in one colour.
    fn block(&mut self, color: u32, b: &Block) -> &mut Self {
        let mut v = Vec::new();
        for f in &b.faces {
            v.extend_from_slice(f);
        }
        self.add(rgb(color), 0.0, v)
    }
    /// Block in one colour except the faces given a painted canvas.
    fn block_skins(&mut self, color: u32, b: Block, skins: Vec<(usize, Canvas)>) -> &mut Self {
        let mut plain = Vec::new();
        let mut used = [false; 6];
        for (i, c) in skins {
            used[i] = true;
            self.add_tex(b.faces[i].clone(), c, 0.0);
        }
        for (i, f) in b.faces.iter().enumerate() {
            if !used[i] {
                plain.extend_from_slice(f);
            }
        }
        if !plain.is_empty() {
            self.add(rgb(color), 0.0, plain);
        }
        self
    }
    fn cyl(&mut self, color: u32, c: [f32; 3], r: f32, h: f32, axis: Axis) -> &mut Self {
        self.add(rgb(color), 0.0, cylinder(c, r, h, axis, 14))
    }
}

// Shared palette.
const HOLE: u32 = 0x101014;
const RIM: u32 = 0x6b6b74;
const LED_GREEN: u32 = 0x4fdc4f;
const LED_RED: u32 = 0xe04040;
const SCREEN: u32 = 0x1d2433;

const TEXELS_PER_CM: f32 = 3.0;

/// cm → texel.
fn t(cm: f32) -> i32 {
    (cm * TEXELS_PER_CM).round() as i32
}

/// Builds the console of `system`, or None for unknown ids.
pub fn build(system: &str) -> Option<Vec<Part>> {
    let mut b = Builder::new();
    match system {
        "n64" => n64(&mut b),
        "psx" => psx(&mut b),
        "ps2" => ps2(&mut b),
        "gc" => gc(&mut b),
        "wii" => wii(&mut b),
        "xbox" => xbox(&mut b),
        "x360" => x360(&mut b),
        "dc" => dc(&mut b),
        "gb" => gb(&mut b),
        "gba" => gba(&mut b),
        "nds" => ds(&mut b, false),
        "3ds" => ds(&mut b, true),
        "psp" => psp(&mut b),
        "ps5" => ps5(&mut b),
        "pc" => pc(&mut b),
        _ => return None,
    }
    Some(b.parts)
}

fn n64(b: &mut Builder) {
    let body = 0x3d3d45;
    let dark = 0x2c2c33;
    let light = 0x55555e;
    // Base: wide tapered block (26 × 19 cm footprint, 4 cm tall).
    let base = block(0.0, 0.0, 0.0, 26.0, 19.0, 23.6, 17.2, 4.0);
    // Top of the base: vents on both wings, power switch + reset at the front-left.
    let mut top = Canvas::for_face(23.6, 17.2, body);
    top.vents_h(t(1.2), t(2.0), t(5.5), 9, 2, dark);
    top.vents_h(t(23.6 - 6.7), t(2.0), t(5.5), 9, 2, dark);
    top.slot(t(1.6), t(11.5), t(3.2), t(1.4), light, dark); // power switch
    top.slot(t(1.6), t(14.0), t(2.4), t(1.2), light, dark); // reset
    top.px(t(5.6), t(12.0), LED_RED);
    // Front of the base: four controller ports + the four-colour logo.
    let mut front = Canvas::for_face(26.0, 4.0, body);
    for x in [-8.0f32, -3.0, 3.0, 8.0] {
        front.port(t(13.0 + x), t(2.1), t(1.3), RIM, HOLE);
    }
    front.rect(t(13.0) - 2, t(1.0), 2, 2, 0xe04040);
    front.rect(t(13.0), t(1.0), 2, 2, 0x4fa64f);
    front.rect(t(13.0) - 2, t(1.0) + 2, 2, 2, 0x3f6fe0);
    front.rect(t(13.0), t(1.0) + 2, 2, 2, 0xf0c040);
    let mut side = Canvas::for_face(19.0, 4.0, body);
    side.vents_v(t(3.0), t(1.0), t(2.0), 10, 2, dark);
    let mut side2 = Canvas::for_face(19.0, 4.0, body);
    side2.vents_v(t(19.0 - 3.0 - 10.0), t(1.0), t(2.0), 10, 2, dark);
    b.block_skins(body, base, vec![(TOP, top), (FRONT, front), (LEFT, side), (RIGHT, side2)]);
    // Hump with the cartridge slot, tapering towards the top.
    let hump = block(0.0, 4.0, -1.0, 14.5, 15.5, 11.0, 12.5, 3.3);
    let mut htop = Canvas::for_face(11.0, 12.5, body);
    htop.slot(t(1.6), t(3.6), t(7.8), t(1.6), light, HOLE); // cartridge slot
    htop.vents_h(t(1.5), t(7.0), t(8.0), 5, 2, dark);
    let mut hfront = Canvas::for_face(14.5, 3.3, body);
    hfront.rect(t(5.0), t(1.0), t(4.5), 2, light); // logo plate
    b.block_skins(body, hump, vec![(TOP, htop), (FRONT, hfront)]);
    // Cartridge peeking out of the slot.
    let cart = block(0.0, 7.3, -1.0, 7.4, 1.5, 7.0, 1.3, 2.4);
    let mut cfront = Canvas::for_face(7.4, 2.4, 0x8d8d93);
    cfront.rect(1, 1, t(7.4) - 2, t(2.4) - 2, 0xc9392f); // label
    cfront.rect(2, 2, t(7.4) - 4, 1, 0xf0e6c0);
    b.block_skins(0x8d8d93, cart, vec![(FRONT, cfront)]);
}

fn psx(b: &mut Builder) {
    let body = 0xd4d1c8;
    let shade = 0xb9b6ad;
    let dark = 0x6f6d66;
    let base = block(0.0, 0.0, 0.0, 27.0, 19.0, 26.4, 18.4, 6.0);
    let mut top = Canvas::for_face(26.4, 18.4, body);
    // Disc lid: big circle offset to the right, with a seam.
    top.disc(t(16.0), t(8.6), t(7.4), 0xdedbd2);
    top.ring(t(16.0), t(8.6), t(7.4), shade);
    top.disc(t(16.0), t(8.6), t(1.2), shade);
    // PlayStation logo colours (tiny) on the lid.
    top.rect(t(16.0) - 3, t(13.5), 2, 2, 0xd94040);
    top.rect(t(16.0) - 1, t(13.5), 2, 2, 0xe0b030);
    top.rect(t(16.0) + 1, t(13.5), 2, 2, 0x40a060);
    top.rect(t(16.0) + 3, t(13.5), 2, 2, 0x4060d0);
    // Buttons on the left: power, reset, open.
    top.slot(t(1.6), t(10.5), t(3.4), t(1.4), shade, dark);
    top.slot(t(1.6), t(13.0), t(3.4), t(1.4), shade, dark);
    top.slot(t(1.6), t(15.5), t(3.4), t(1.4), shade, dark);
    top.px(t(5.6), t(11.0), LED_GREEN);
    top.vents_h(t(1.5), t(1.5), t(6.0), 6, 2, shade);
    let mut front = Canvas::for_face(27.0, 6.0, body);
    for x in [3.0f32, 8.5] {
        front.slot(t(x), t(3.6), t(4.4), t(1.8), dark, HOLE); // controller ports
        front.slot(t(x), t(1.4), t(4.4), t(1.0), dark, HOLE); // memory cards
    }
    front.vents_v(t(18.0), t(1.5), t(3.0), 10, 2, shade);
    let mut back = Canvas::for_face(27.0, 6.0, body);
    back.vents_v(t(2.0), t(1.2), t(3.6), 18, 2, shade);
    b.block_skins(body, base, vec![(TOP, top), (FRONT, front), (BACK, back)]);
}

fn ps2(b: &mut Builder) {
    let body = 0x17171d;
    let groove = 0x26262e;
    let base = block(0.0, 0.0, 0.0, 30.0, 18.0, 30.0, 18.0, 7.8);
    let mut front = Canvas::for_face(30.0, 7.8, body);
    front.rect(0, 0, t(1.6), t(7.8), 0x2244cc); // blue edge
    front.vents_h(t(1.6), t(1.0), t(28.4), 4, 4, groove);
    front.slot(t(4.0), t(5.4), t(3.6), t(1.6), 0x3a3a44, HOLE);
    front.slot(t(8.5), t(5.4), t(3.6), t(1.6), 0x3a3a44, HOLE);
    front.slot(t(4.0), t(3.6), t(3.6), t(0.9), 0x3a3a44, HOLE);
    front.slot(t(8.5), t(3.6), t(3.6), t(0.9), 0x3a3a44, HOLE);
    front.slot(t(13.5), t(1.2), t(14.0), t(1.6), 0x3a3a44, 0x1e1e26); // tray
    front.disc(t(26.0), t(4.6), 2, 0x3a3a44); // reset
    front.disc(t(28.0), t(4.6), 2, 0x3a3a44); // eject
    front.px(t(26.0), t(6.4), LED_GREEN);
    let mut top = Canvas::for_face(30.0, 18.0, body);
    top.vents_h(0, t(2.0), t(30.0), 4, 8, groove);
    top.rect(t(22.0), t(10.0), t(5.0), 2, 0x5c5c66); // logo bar
    b.block_skins(body, base, vec![(FRONT, front), (TOP, top)]);
}

fn gc(b: &mut Builder) {
    let body = 0x5048a0;
    let shade = 0x3e3880;
    let light = 0x6a62b8;
    let base = block(0.0, 0.0, 0.0, 15.0, 16.0, 15.0, 16.0, 11.0);
    let mut front = Canvas::for_face(15.0, 11.0, body);
    for x in [3.0f32, 6.0, 9.0, 12.0] {
        front.port(t(x), t(5.2), t(1.2), 0x8a84c4, HOLE);
    }
    front.slot(t(2.0), t(8.6), t(4.6), t(1.4), shade, HOLE);
    front.slot(t(8.4), t(8.6), t(4.6), t(1.4), shade, HOLE);
    front.rect(t(1.0), t(1.6), t(13.0), 1, shade);
    let mut top = Canvas::for_face(15.0, 16.0, body);
    top.disc(t(7.5), t(7.5), t(5.8), light);
    top.ring(t(7.5), t(7.5), t(5.8), shade);
    top.ring(t(7.5), t(7.5), t(2.2), shade); // logo ring
    top.rect(t(7.5) - 1, t(7.5) - 1, 3, 3, shade);
    top.disc(t(13.3), t(13.5), 2, 0x9a94d0); // power
    top.disc(t(13.3), t(11.0), 1, 0x9a94d0); // reset
    top.vents_h(t(1.0), t(13.0), t(4.0), 4, 2, shade);
    let mut right = Canvas::for_face(16.0, 11.0, body);
    right.dots(t(1.5), t(2.0), 12, 10, 2, shade); // vent grid
    let mut left = Canvas::for_face(16.0, 11.0, body);
    left.dots(t(16.0 - 1.5 - 11.0), t(2.0), 12, 10, 2, shade);
    b.block_skins(body, base, vec![(FRONT, front), (TOP, top), (RIGHT, right), (LEFT, left)]);
    // Handle at the back.
    b.block(shade, &block(-4.5, 2.0, -8.9, 1.6, 1.6, 1.6, 1.6, 6.0));
    b.block(shade, &block(4.5, 2.0, -8.9, 1.6, 1.6, 1.6, 1.6, 6.0));
    b.block(shade, &block(0.0, 8.0, -8.9, 10.6, 1.6, 10.6, 1.6, 1.6));
}

fn wii(b: &mut Builder) {
    let body = 0xeef0f3;
    let shade = 0xcfd3d8;
    let base = block(0.0, 0.0, 0.0, 4.4, 15.7, 4.4, 15.7, 21.5);
    let mut front = Canvas::for_face(4.4, 21.5, body);
    front.rect(t(2.0), t(2.0), 1, t(12.0), 0x7fb8ff); // disc slot light
    front.rect(t(2.0) + 1, t(2.0), 1, t(12.0), HOLE);
    front.slot(t(0.8), t(15.5), t(2.8), t(2.6), shade, 0xe2e5ea); // SD door
    front.disc(t(2.2), t(19.4), 1, 0x9aa0a8); // power
    front.px(t(1.0), t(19.4), 0x7fb8ff);
    let mut left = Canvas::for_face(15.7, 21.5, body);
    left.vents_h(t(10.0), t(16.0), t(4.0), 8, 2, shade);
    left.rect(t(1.0), t(1.0), t(13.7), 1, shade);
    b.block_skins(body, base, vec![(FRONT, front), (LEFT, left)]);
    // Stand.
    b.block(shade, &block(0.0, -1.3, 0.0, 10.0, 12.0, 8.0, 11.0, 1.3));
}

fn xbox(b: &mut Builder) {
    let body = 0x121214;
    let edge = 0x26262a;
    let base = block(0.0, 0.0, 0.0, 32.0, 26.0, 30.6, 24.8, 10.0);
    let mut top = Canvas::for_face(30.6, 24.8, body);
    // Big X grooves and the green jewel.
    for i in 0..t(24.8) {
        let x0 = (i as f32 / t(24.8) as f32 * t(30.6) as f32) as i32;
        top.px(x0, i, edge);
        top.px(x0 + 1, i, edge);
        top.px(t(30.6) - 1 - x0, i, edge);
        top.px(t(30.6) - 2 - x0, i, edge);
    }
    top.disc(t(15.3), t(11.5), t(5.0), 0x1c1c20);
    top.disc(t(15.3), t(11.5), t(4.2), 0x5cc230);
    top.disc(t(15.3), t(11.5), t(1.6), 0x3f8e22);
    let mut front = Canvas::for_face(32.0, 10.0, body);
    for x in [5.0f32, 11.0, 21.0, 27.0] {
        front.port(t(x), t(6.6), t(1.6), 0x3a3a40, HOLE);
    }
    front.slot(t(6.0), t(1.6), t(20.0), t(2.0), 0x2a2a30, 0x1a1a1e); // tray
    front.disc(t(29.0), t(2.6), 2, 0x5cc230); // power
    front.disc(t(3.0), t(2.6), 2, 0x3a3a40); // eject
    b.block_skins(body, base, vec![(TOP, top), (FRONT, front)]);
}

fn x360(b: &mut Builder) {
    let body = 0xe9e9ec;
    let shade = 0xc9cbd0;
    let base = block(0.0, 0.0, 0.0, 31.0, 26.0, 30.0, 25.0, 8.3);
    let mut front = Canvas::for_face(31.0, 8.3, shade);
    front.rect(0, 0, t(31.0), 1, body);
    front.slot(t(2.0), t(5.4), t(17.0), t(1.6), 0x3a3a40, 0x1a1a1e); // tray
    front.disc(t(25.5), t(4.1), t(2.3), body);
    front.disc(t(25.5), t(4.1), t(1.5), 0x58c843); // ring of light
    front.disc(t(25.5), t(4.1), 1, body);
    front.slot(t(2.0), t(1.4), t(2.6), t(1.6), 0x9fa2a8, HOLE); // memory units
    front.slot(t(5.4), t(1.4), t(2.6), t(1.6), 0x9fa2a8, HOLE);
    let mut top = Canvas::for_face(30.0, 25.0, body);
    top.dots(t(2.0), t(2.0), 30, 6, 2, shade);
    top.dots(t(2.0), t(19.0), 30, 6, 2, shade);
    top.rect(t(26.0), t(11.0), t(2.0), t(2.0), shade); // sticker
    let mut back = Canvas::for_face(31.0, 8.3, body);
    back.dots(t(3.0), t(1.5), 48, 6, 2, shade);
    b.block_skins(body, base, vec![(FRONT, front), (TOP, top), (BACK, back)]);
}

fn dc(b: &mut Builder) {
    let body = 0xeeeeee;
    let shade = 0xd0d0d0;
    let base = block(0.0, 0.0, 0.0, 19.0, 19.5, 18.4, 18.9, 7.6);
    let mut top = Canvas::for_face(18.4, 18.9, body);
    top.disc(t(9.2), t(8.6), t(7.4), 0xf6f6f6);
    top.ring(t(9.2), t(8.6), t(7.4), shade);
    top.disc(t(9.2), t(8.6), t(1.2), 0xf27a1a); // swirl
    top.px(t(9.2) + 2, t(8.6) - 2, body);
    top.slot(t(1.2), t(15.6), t(3.0), t(1.4), shade, 0xbdbdbd); // power
    top.slot(t(14.2), t(15.6), t(3.0), t(1.4), shade, 0xbdbdbd); // open
    top.px(t(5.0), t(16.3), 0xf27a1a);
    top.vents_h(t(1.0), t(1.0), t(5.0), 5, 2, shade);
    let mut front = Canvas::for_face(19.0, 7.6, body);
    for x in [3.5f32, 7.5, 11.5, 15.5] {
        front.port(t(x), t(4.4), t(1.2), 0xa8a8a8, HOLE);
    }
    b.block_skins(body, base, vec![(TOP, top), (FRONT, front)]);
}

fn gb(b: &mut Builder) {
    let body = 0xc7c6c0;
    let base = block(0.0, 0.0, 0.0, 9.0, 3.2, 9.0, 3.2, 14.8);
    let mut front = Canvas::for_face(9.0, 14.8, body);
    front.rect(t(0.7), t(1.0), t(7.6), t(5.8), 0x3c3c48); // bezel
    front.rect(t(0.7), t(1.3), t(7.6), 1, 0x6e1f3f); // purple line
    front.rect(t(0.7), t(1.6), t(7.6), 1, 0x2f3bb3); // blue line
    front.rect(t(1.6), t(2.0), t(4.6), t(4.1), 0x8fa44a); // screen
    front.px(t(1.0), t(4.0), LED_RED);
    front.rect(t(1.0), t(7.6), t(4.0), 1, 0x4a4a52); // "Nintendo GAME BOY"
    front.rect(t(1.3), t(9.3), t(2.6), t(0.9), 0x2a2a30); // d-pad
    front.rect(t(2.15), t(8.45), t(0.9), t(2.6), 0x2a2a30);
    front.disc(t(6.0), t(10.6), t(0.6), 0xa8356b); // B
    front.disc(t(7.6), t(9.8), t(0.6), 0xa8356b); // A
    front.rect(t(2.5), t(12.6), t(1.4), 1, 0x7a7a80); // select
    front.rect(t(4.4), t(12.6), t(1.4), 1, 0x7a7a80); // start
    for i in 0..6 {
        front.rect(t(6.0) + i * 2, t(12.0) + i, 1, t(1.6), 0x9a9a94); // speaker
    }
    let mut back = Canvas::for_face(9.0, 14.8, body);
    back.rect(t(1.5), t(2.0), t(6.0), t(4.0), 0xb3b2ac); // battery cover
    b.block_skins(body, base, vec![(FRONT, front), (BACK, back)]);
}

fn gba(b: &mut Builder) {
    let body = 0x5a4fa8;
    let base = block(0.0, 0.0, 0.0, 14.5, 2.5, 14.5, 2.5, 8.2);
    let mut front = Canvas::for_face(14.5, 8.2, body);
    front.rect(t(3.6), t(1.2), t(7.3), t(5.5), 0x2a2a35); // bezel
    front.rect(t(4.3), t(1.9), t(5.9), t(4.0), 0x9aa3b8); // screen
    front.rect(t(0.9), t(3.5), t(2.4), t(0.8), 0x2a2a30); // d-pad
    front.rect(t(1.7), t(2.7), t(0.8), t(2.4), 0x2a2a30);
    front.disc(t(12.0), t(4.0), t(0.5), 0xc7c3e3); // B
    front.disc(t(13.3), t(3.0), t(0.5), 0xc7c3e3); // A
    front.rect(t(1.2), t(6.6), t(1.3), 1, 0xc7c3e3); // select
    front.rect(t(1.2), t(7.3), t(1.3), 1, 0xc7c3e3); // start
    front.px(t(3.8), t(1.6), LED_GREEN);
    for i in 0..5 {
        front.rect(t(12.0) + i, t(6.4) + i, t(0.6), 1, 0x8a82c8); // speaker
    }
    b.block_skins(body, base, vec![(FRONT, front)]);
    b.block(0x3f3870, &block(-5.4, 8.2, -0.3, 4.0, 1.6, 4.0, 1.6, 0.5)); // L
    b.block(0x3f3870, &block(5.4, 8.2, -0.3, 4.0, 1.6, 4.0, 1.6, 0.5)); // R
}

fn ds(b: &mut Builder, is3: bool) {
    let shell = if is3 { 0xb7202e } else { 0xf2f2f4 };
    let inner = if is3 { 0x202024 } else { 0xf7f7f9 };
    let detail = if is3 { 0x3a3a40 } else { 0xb9bfc8 };
    let (w, d, lid_h) = (13.4f32, 7.4f32, 7.4f32);
    // Bottom half: painted top face (screen, d-pad, buttons).
    let bottom = block(0.0, 0.0, 0.0, w, d, w, d, 1.1);
    let mut top = Canvas::for_face(w, d, inner);
    top.rect(t(3.9), t(1.3), t(5.6), t(4.2), SCREEN); // touch screen
    top.rect(t(0.8), t(3.0), t(2.2), t(0.7), detail); // d-pad
    top.rect(t(1.55), t(2.25), t(0.7), t(2.2), detail);
    for (x, y) in [(10.8f32, 3.3f32), (11.8, 2.3), (11.8, 4.3), (12.8, 3.3)] {
        top.disc(t(x), t(y), 1, detail);
    }
    if is3 {
        top.disc(t(1.9), t(1.4), t(0.8), 0x5a5a62); // circle pad
    }
    top.rect(t(10.0), t(6.0), t(1.0), 1, detail); // start
    top.rect(t(11.5), t(6.0), t(1.0), 1, detail); // select
    let mut front = Canvas::for_face(w, 1.1, shell);
    front.rect(t(4.0), 1, t(5.0), 1, detail); // cartridge slot
    b.block_skins(shell, bottom, vec![(TOP, top), (FRONT, front)]);
    // Lid, hinged at the back, leaning back ~25°.
    let hinge = [0.0, 0.55, -d / 2.0];
    let tilt = -0.42f32;
    let lid = block(0.0, 0.55, -d / 2.0 + 0.55, w, 1.1, w, 1.1, lid_h);
    let mut face = Canvas::for_face(w, lid_h, inner); // inner side (faces the player)
    face.rect(t(if is3 { 2.9 } else { 3.9 }), t(1.3), t(if is3 { 7.7 } else { 5.6 }), t(4.3), SCREEN);
    for i in 0..4 {
        face.px(t(1.2) + i * 2, t(3.5), detail); // speaker dots
        face.px(t(w - 1.2) - i * 2, t(3.5), detail);
    }
    let mut outer = Canvas::for_face(w, lid_h, shell);
    outer.disc(t(w / 2.0), t(lid_h / 2.0), 2, if is3 { 0x8a1822 } else { 0xd8d8dc }); // logo
    outer.px(t(w - 1.0), t(1.0), LED_GREEN);
    let mut parts = Builder::new();
    parts.block_skins(shell, lid, vec![(FRONT, face), (BACK, outer)]);
    for mut p in parts.parts {
        p.verts = rotate_x_about(p.verts, tilt, hinge);
        b.parts.push(p);
    }
    b.cyl(shell, hinge, 0.6, w - 1.0, Axis::X);
}

fn psp(b: &mut Builder) {
    let body = 0x15151a;
    let base = block(0.0, 0.0, 0.0, 17.0, 2.3, 17.0, 2.3, 7.4);
    let mut front = Canvas::for_face(17.0, 7.4, body);
    front.rect(t(3.7), t(1.0), t(9.6), t(5.4), 0x2a3350); // screen
    front.rect(t(3.9), t(1.2), t(9.2), 1, 0x3a4668); // glare line
    front.rect(t(0.8), t(2.9), t(2.2), t(0.7), 0x2e2e36); // d-pad
    front.rect(t(1.55), t(2.15), t(0.7), t(2.2), 0x2e2e36);
    front.disc(t(1.9), t(6.0), t(0.7), 0x55555c); // analog nub
    for (x, y) in [(14.4f32, 2.2f32), (13.3, 3.3), (15.5, 3.3), (14.4, 4.4)] {
        front.disc(t(x), t(y), 1, 0xb9b9c0);
    }
    front.rect(t(13.6), t(6.3), t(1.2), 1, 0x8a8a92); // select
    front.rect(t(15.2), t(6.3), t(1.2), 1, 0x8a8a92); // start
    front.rect(t(7.0), t(6.6), t(3.0), 1, 0x8a8a92); // "PSP" bar
    front.px(t(15.8), t(0.6), LED_GREEN);
    b.block_skins(body, base, vec![(FRONT, front)]);
    b.block(0x2a2a30, &block(-6.5, 7.4, 0.2, 3.5, 1.4, 3.5, 1.4, 0.3)); // L
    b.block(0x2a2a30, &block(6.5, 7.4, 0.2, 3.5, 1.4, 3.5, 1.4, 0.3)); // R
}

fn ps5(b: &mut Builder) {
    let plate = 0xf4f4f6;
    let core = block(0.0, 0.0, 0.0, 8.0, 24.0, 8.0, 24.0, 38.0);
    let mut front = Canvas::for_face(8.0, 38.0, 0x111115);
    front.rect(t(3.6), t(6.0), 2, t(14.0), 0x2a2a30); // disc slot
    front.disc(t(4.0), t(26.0), 1, 0x8a8a92); // power
    front.disc(t(4.0), t(28.5), 1, 0x8a8a92); // eject
    front.rect(t(2.6), t(31.0), t(2.8), 1, 0x2a2a30); // USB
    b.block_skins(0x111115, core, vec![(FRONT, front)]);
    // White plates flaring out towards the top.
    b.block(plate, &block(-4.8, -1.0, 0.0, 1.6, 26.0, 2.6, 26.0, 40.0));
    b.block(plate, &block(4.8, -1.0, 0.0, 1.6, 26.0, 2.6, 26.0, 40.0));
    let strip = block(0.0, 38.0, 0.0, 7.6, 22.0, 7.6, 22.0, 0.5);
    let mut v = Vec::new();
    for f in &strip.faces {
        v.extend_from_slice(f);
    }
    b.add(rgb(0x4f8cff), 1.0, v); // light strip
    b.cyl(0x1a1a1f, [0.0, -1.6, 0.0], 7.0, 1.0, Axis::Y); // stand
}

fn pc(b: &mut Builder) {
    let body = 0x1c1d22;
    let base = block(0.0, 0.0, 0.0, 21.0, 45.0, 21.0, 45.0, 45.0);
    let mut front = Canvas::for_face(21.0, 45.0, 0x26272d);
    front.dots(t(2.0), t(2.0), 17, 41, 2, 0x15161a); // mesh
    for y in [9.0f32, 22.5, 36.0] {
        front.disc(t(10.5), t(y), t(5.6), 0x1c1d22);
        front.ring(t(10.5), t(y), t(5.6), 0x3ad6c8);
        front.ring(t(10.5), t(y), t(5.6) - 1, 0x3ad6c8);
        front.disc(t(10.5), t(y), t(1.8), 0x2a2b31);
    }
    front.px(t(18.5), t(1.5), 0x3ad6c8); // power led
    let mut right = Canvas::for_face(45.0, 45.0, body);
    right.rect(t(2.0), t(2.0), t(41.0), t(41.0), 0x2a3a5a); // tempered glass
    right.rect(t(4.0), t(4.0), t(14.0), t(10.0), 0x24344f); // motherboard shadow
    right.rect(t(20.0), t(28.0), t(18.0), t(6.0), 0x3a2a5a); // GPU glow
    right.rect(t(20.0), t(27.0), t(18.0), 1, 0x9a5cff);
    let mut top = Canvas::for_face(21.0, 45.0, body);
    top.dots(t(3.0), t(3.0), 15, 39, 2, 0x15161a);
    b.block_skins(body, base, vec![(FRONT, front), (RIGHT, right), (TOP, top)]);
    for (x, z) in [(-8.0f32, -18.0f32), (8.0, -18.0), (-8.0, 18.0), (8.0, 18.0)] {
        b.block(0x111114, &block(x, -1.2, z, 3.0, 4.0, 3.0, 4.0, 1.2)); // feet
    }
}

/// Axis-aligned bounds of a set of parts (for normalisation).
pub fn bounds(parts: &[Part]) -> ([f32; 3], [f32; 3]) {
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    for p in parts {
        for i in (0..p.verts.len()).step_by(8) {
            for c in 0..3 {
                lo[c] = lo[c].min(p.verts[i + c]);
                hi[c] = hi[c].max(p.verts[i + c]);
            }
        }
    }
    (lo, hi)
}
