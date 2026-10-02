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
    pub fn frame(&mut self, x: i32, y: i32, w: i32, h: i32, color: u32) {
        self.rect(x, y, w, 1, color);
        self.rect(x, y + h - 1, w, 1, color);
        self.rect(x, y, 1, h, color);
        self.rect(x + w - 1, y, 1, h, color);
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

/// Six groups of faces of a solid: front, back, left, right, top, bottom —
/// each with uv 0..1 (u left→right, v top→bottom as seen from outside; the
/// top is seen from above with the front at the bottom).
pub struct Block {
    pub faces: [Vec<f32>; 6],
    /// Highest point (walls map v = 1 - y / hmax).
    pub hmax: f32,
}

fn tri(p: [[f32; 3]; 3], uv: [[f32; 2]; 3], out: &mut Vec<f32>) {
    let u = [p[1][0] - p[0][0], p[1][1] - p[0][1], p[1][2] - p[0][2]];
    let v = [p[2][0] - p[0][0], p[2][1] - p[0][1], p[2][2] - p[0][2]];
    let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-6);
    let n = [n[0] / l, n[1] / l, n[2] / l];
    for i in 0..3 {
        out.extend_from_slice(&p[i]);
        out.extend_from_slice(&n);
        out.extend_from_slice(&uv[i]);
    }
}

/// Quad BL, BR, TR, TL (CCW from outside) with explicit uvs.
fn quad_uv(p: [[f32; 3]; 4], uv: [[f32; 2]; 4]) -> Vec<f32> {
    let mut out = Vec::with_capacity(48);
    tri([p[0], p[1], p[2]], [uv[0], uv[1], uv[2]], &mut out);
    tri([p[0], p[2], p[3]], [uv[0], uv[2], uv[3]], &mut out);
    out
}

fn quad(p: [[f32; 3]; 4]) -> Vec<f32> {
    quad_uv(p, [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]])
}

/// Tapered block: bottom rectangle `bw × bd` at `y0`, top `tw × td` at `y0 + h`.
#[allow(clippy::too_many_arguments)]
pub fn block(cx: f32, y0: f32, cz: f32, bw: f32, bd: f32, tw: f32, td: f32, h: f32) -> Block {
    let (bx, bz, tx, tz) = (bw / 2.0, bd / 2.0, tw / 2.0, td / 2.0);
    let y1 = y0 + h;
    let b = [[cx - bx, y0, cz - bz], [cx + bx, y0, cz - bz], [cx + bx, y0, cz + bz], [cx - bx, y0, cz + bz]];
    let t = [[cx - tx, y1, cz - tz], [cx + tx, y1, cz - tz], [cx + tx, y1, cz + tz], [cx - tx, y1, cz + tz]];
    Block {
        faces: [
            quad([b[3], b[2], t[2], t[3]]),
            quad([b[1], b[0], t[0], t[1]]),
            quad([b[0], b[3], t[3], t[0]]),
            quad([b[2], b[1], t[1], t[2]]),
            quad([t[3], t[2], t[1], t[0]]),
            quad([b[0], b[1], b[2], b[3]]),
        ],
        hmax: y1,
    }
}

/// Relief shell: a footprint `w × d` (centred at the origin, `inside` says
/// which cells exist) with a height function sampled on a `cell`-sized grid.
/// Faceted (one flat normal per triangle) so the low-poly facets show. Walls
/// are emitted wherever a cell borders the outside and sorted into the four
/// side groups by their direction; the top gets uv over the whole footprint.
pub fn shell(w: f32, d: f32, cell: f32, inside: &dyn Fn(f32, f32) -> bool, height: &dyn Fn(f32, f32) -> f32) -> Block {
    let nx = (w / cell).ceil() as i32;
    let nz = (d / cell).ceil() as i32;
    let x_at = |i: i32| -w / 2.0 + i as f32 * cell;
    let z_at = |k: i32| -d / 2.0 + k as f32 * cell;
    let present = |i: i32, k: i32| -> bool {
        i >= 0 && k >= 0 && i < nx && k < nz && inside(x_at(i) + cell / 2.0, z_at(k) + cell / 2.0)
    };
    // Corner heights: sample the height function clamped into the footprint.
    let hc = |i: i32, k: i32| -> f32 {
        let x = x_at(i).clamp(-w / 2.0 + 0.01, w / 2.0 - 0.01);
        let z = z_at(k).clamp(-d / 2.0 + 0.01, d / 2.0 - 0.01);
        height(x, z).max(0.05)
    };
    let mut hmax = 0.0f32;
    for k in 0..=nz {
        for i in 0..=nx {
            hmax = hmax.max(hc(i, k));
        }
    }
    let mut faces: [Vec<f32>; 6] = Default::default();
    let uv_top = |i: i32, k: i32| [(x_at(i) + w / 2.0) / w, (z_at(k) + d / 2.0) / d];
    for k in 0..nz {
        for i in 0..nx {
            if !present(i, k) {
                continue;
            }
            let (x0, x1, z0, z1) = (x_at(i), x_at(i + 1), z_at(k), z_at(k + 1));
            let a = [x0, hc(i, k), z0]; // back-left
            let bb = [x1, hc(i + 1, k), z0]; // back-right
            let c = [x1, hc(i + 1, k + 1), z1]; // front-right
            let dd = [x0, hc(i, k + 1), z1]; // front-left
            let (ua, ub, uc, ud) = (uv_top(i, k), uv_top(i + 1, k), uv_top(i + 1, k + 1), uv_top(i, k + 1));
            tri([dd, c, bb], [ud, uc, ub], &mut faces[TOP]);
            tri([dd, bb, a], [ud, ub, ua], &mut faces[TOP]);
            // Bottom (flat).
            let f = |p: [f32; 3]| [p[0], 0.0, p[2]];
            faces[BOTTOM].extend(quad_uv([f(a), f(bb), f(c), f(dd)], [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]]));
            // Walls towards missing neighbours.
            let vmap = |h: f32| 1.0 - h / hmax;
            if !present(i, k + 1) {
                let (u0, u1) = ((x0 + w / 2.0) / w, (x1 + w / 2.0) / w);
                faces[FRONT].extend(quad_uv(
                    [f(dd), f(c), c, dd],
                    [[u0, 1.0], [u1, 1.0], [u1, vmap(c[1])], [u0, vmap(dd[1])]],
                ));
            }
            if !present(i, k - 1) {
                let (u0, u1) = ((w / 2.0 - x1) / w, (w / 2.0 - x0) / w);
                faces[BACK].extend(quad_uv(
                    [f(bb), f(a), a, bb],
                    [[u0, 1.0], [u1, 1.0], [u1, vmap(a[1])], [u0, vmap(bb[1])]],
                ));
            }
            if !present(i - 1, k) {
                let (u0, u1) = ((z0 + d / 2.0) / d, (z1 + d / 2.0) / d);
                faces[LEFT].extend(quad_uv(
                    [f(a), f(dd), dd, a],
                    [[u0, 1.0], [u1, 1.0], [u1, vmap(dd[1])], [u0, vmap(a[1])]],
                ));
            }
            if !present(i + 1, k) {
                let (u0, u1) = ((d / 2.0 - z1) / d, (d / 2.0 - z0) / d);
                faces[RIGHT].extend(quad_uv(
                    [f(c), f(bb), bb, c],
                    [[u0, 1.0], [u1, 1.0], [u1, vmap(bb[1])], [u0, vmap(c[1])]],
                ));
            }
        }
    }
    Block { faces, hmax }
}

/// Inside a rectangle `w × d` centred at the origin with corner radius `r`.
pub fn rounded_rect(x: f32, z: f32, w: f32, d: f32, r: f32) -> bool {
    rounded_rect4(x, z, w, d, [r, r, r, r])
}

/// Same with one radius per corner: back-left, back-right, front-right, front-left.
pub fn rounded_rect4(x: f32, z: f32, w: f32, d: f32, r: [f32; 4]) -> bool {
    let (hx, hz) = (w / 2.0, d / 2.0);
    if x.abs() > hx || z.abs() > hz {
        return false;
    }
    let ri = match (x < 0.0, z < 0.0) {
        (true, true) => r[0],
        (false, true) => r[1],
        (false, false) => r[2],
        (true, false) => r[3],
    };
    let dx = (x.abs() - (hx - ri)).max(0.0);
    let dz = (z.abs() - (hz - ri)).max(0.0);
    dx * dx + dz * dz <= ri * ri
}

pub fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Soft disc bump of height `h` and radius `r` centred at (cx, cz).
pub fn dome(x: f32, z: f32, cx: f32, cz: f32, r: f32, h: f32) -> f32 {
    let dist = ((x - cx).powi(2) + (z - cz).powi(2)).sqrt();
    h * (1.0 - smoothstep(r - 0.8, r + 0.2, dist))
}

/// Raised rectangle (buttons, bezels): `h` inside `x0..x1 × z0..z1`.
pub fn plate(x: f32, z: f32, x0: f32, z0: f32, x1: f32, z1: f32, h: f32) -> f32 {
    if x >= x0 && x <= x1 && z >= z0 && z <= z1 {
        h
    } else {
        0.0
    }
}

pub const FRONT: usize = 0;
pub const BACK: usize = 1;
pub const LEFT: usize = 2;
pub const RIGHT: usize = 3;
pub const TOP: usize = 4;
pub const BOTTOM: usize = 5;

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

/// Lays a handheld built flat (face up, front = bottom edge) upright facing
/// the camera: top → +z, front → -y.
fn stand_up(v: Vec<f32>) -> Vec<f32> {
    rotate_x_about(v, std::f32::consts::FRAC_PI_2, [0.0; 3])
}

fn translate(mut v: Vec<f32>, dx: f32, dy: f32, dz: f32) -> Vec<f32> {
    for i in (0..v.len()).step_by(8) {
        v[i] += dx;
        v[i + 1] += dy;
        v[i + 2] += dz;
    }
    v
}

/// Centre-relative cm from canvas-style (top-left origin) cm on a `w × d` top.
fn cx(w: f32, x_from_left: f32) -> f32 {
    x_from_left - w / 2.0
}
fn cz(d: f32, y_from_top: f32) -> f32 {
    y_from_top - d / 2.0
}

fn n64(b: &mut Builder) {
    // From the reference photo: a flat lower tray that sticks out at the
    // front (four grey ports + logo on its face), and a set-back upper shell
    // with a very broad hump, the cartridge slot at the back, the Expansion
    // Pak lid at the front centre, power slider left and reset right.
    let (w, d) = (26.0f32, 19.0f32);
    let body = 0x3a3a40;
    let dark = 0x26262b;
    let light = 0x8e8e94;
    let inside = |x: f32, z: f32| rounded_rect(x, z, w, d, 2.6);
    // Upper shell footprint: 1 cm in from the sides/back, 3.5 cm from the front.
    let shell_in = |x: f32, z: f32| rounded_rect(x, z + 1.25, 24.0, 16.5, 3.0);
    let core_in = |x: f32, z: f32| rounded_rect(x, z + 1.25, 22.4, 14.9, 2.4);
    let height = |x: f32, z: f32| {
        let mut h = 2.6; // tray
        if shell_in(x, z) {
            h += if core_in(x, z) { 2.4 } else { 1.6 }; // shell with a chamfered rim
            let (nx, nz) = (x / 10.5, (z + 2.0) / 8.0); // broad hump, slightly back
            let r = (nx * nx + nz * nz).sqrt();
            h += 2.4 * (1.0 - smoothstep(0.5, 1.05, r));
            h += plate(x, z, -4.0, 1.0, 4.0, 5.5, 0.25); // Expansion Pak lid
            if x.abs() < 4.3 && z > -5.6 && z < -3.6 {
                h -= 1.5; // cartridge slot
            }
            h += plate(x, z, -10.5, -1.0, -7.5, 1.5, -0.3); // power slider track
            h += plate(x, z, 7.5, 1.0, 10.0, 3.5, 0.3); // reset button
        }
        h
    };
    let sh = shell(w, d, 0.5, &inside, &height);
    let hmax = sh.hmax;
    let mut top = Canvas::for_face(w, d, body);
    top.slot(t(8.4), t(3.9), t(9.2), t(2.0), light, HOLE); // cartridge slot with grey frame
    top.rect(t(9.0), t(10.5), t(8.0), t(4.5), 0x404046); // Expansion Pak lid
    top.frame(t(9.0), t(10.5), t(8.0), t(4.5), dark);
    top.rect(t(10.5), t(12.4), t(5.0), 1, light); // "Nintendo 64" on the lid
    top.rect(t(2.5), t(8.5), t(3.0), t(2.5), dark); // power track
    top.rect(t(3.0), t(9.0), t(2.0), t(1.1), light); // slider
    top.rect(t(20.5), t(10.5), t(2.5), t(2.5), dark); // reset
    top.frame(t(20.5), t(10.5), t(2.5), t(2.5), 0x55555c);
    top.px(t(6.2), t(9.0), LED_RED);
    let mut front = Canvas::for_face(w, hmax, body);
    let py = t(hmax - 1.3);
    for x in [4.5f32, 8.2, 17.8, 21.5] {
        front.disc(t(x), py, t(1.1), light);
        front.disc(t(x), py, t(0.7), 0x6a6a70);
        front.px(t(x) - 1, py, HOLE);
        front.px(t(x) + 1, py, HOLE);
        front.px(t(x), py + 1, HOLE);
    }
    front.rect(t(11.0), t(hmax - 2.2), t(4.0), 1, light); // NINTENDO 64
    let (lx, ly) = (t(13.0) - 2, t(hmax - 1.6));
    front.rect(lx, ly, 2, 2, 0xe04040);
    front.rect(lx + 2, ly, 2, 2, 0x4fa64f);
    front.rect(lx, ly + 2, 2, 2, 0x3f6fe0);
    front.rect(lx + 2, ly + 2, 2, 2, 0xf0c040);
    let mut back = Canvas::for_face(w, hmax, body);
    back.vents_v(t(3.0), t(hmax - 2.3), t(1.6), 30, 2, dark);
    b.block_skins(body, sh, vec![(TOP, top), (FRONT, front), (BACK, back)]);
    // Cartridge sitting in the slot.
    let cart = block(0.0, 6.0, -4.6, 7.4, 1.6, 7.0, 1.4, 2.6);
    let mut cfront = Canvas::for_face(7.4, 2.6, 0x8d8d93);
    cfront.rect(1, 1, t(7.4) - 2, t(2.6) - 2, 0xc9392f);
    cfront.rect(2, 2, t(7.4) - 4, 1, 0xf0e6c0);
    b.block_skins(0x8d8d93, cart, vec![(FRONT, cfront)]);
}

fn psx(b: &mut Builder) {
    let (w, d) = (27.0f32, 19.0f32);
    let body = 0xd4d1c8;
    let shade = 0xb9b6ad;
    let dark = 0x6f6d66;
    let inside = |x: f32, z: f32| rounded_rect(x, z, w, d, 2.2);
    let height = |x: f32, z: f32| {
        let zn = (z + d / 2.0) / d;
        6.0 - 0.5 * smoothstep(0.85, 1.0, zn) + dome(x, z, 3.0, -1.0, 7.5, 0.55)
    };
    let sh = shell(w, d, 0.5, &inside, &height);
    let hmax = sh.hmax;
    let mut top = Canvas::for_face(w, d, body);
    let (lcx, lcz) = (t(w / 2.0 + 3.0), t(d / 2.0 - 1.0));
    top.disc(lcx, lcz, t(7.5), 0xdedbd2);
    top.ring(lcx, lcz, t(7.5), shade);
    top.disc(lcx, lcz, t(1.2), shade);
    top.rect(lcx - 4, lcz + t(4.6), 2, 2, 0xd94040); // PlayStation colours
    top.rect(lcx - 2, lcz + t(4.6), 2, 2, 0xe0b030);
    top.rect(lcx, lcz + t(4.6), 2, 2, 0x40a060);
    top.rect(lcx + 2, lcz + t(4.6), 2, 2, 0x4060d0);
    top.slot(t(1.8), t(10.5), t(3.4), t(1.4), shade, dark); // power
    top.slot(t(1.8), t(13.0), t(3.4), t(1.4), shade, dark); // reset
    top.slot(t(1.8), t(15.5), t(3.4), t(1.4), shade, dark); // open
    top.px(t(5.8), t(11.1), LED_GREEN);
    top.vents_h(t(1.5), t(1.5), t(6.0), 6, 2, shade);
    let mut front = Canvas::for_face(w, hmax, body);
    for x in [3.0f32, 8.5] {
        front.slot(t(x), t(hmax - 2.6), t(4.4), t(1.8), dark, HOLE); // controller ports
        front.slot(t(x), t(hmax - 4.6), t(4.4), t(1.0), dark, HOLE); // memory cards
    }
    front.vents_v(t(18.0), t(hmax - 4.0), t(3.0), 10, 2, shade);
    let mut back = Canvas::for_face(w, hmax, body);
    back.vents_v(t(2.0), t(hmax - 4.5), t(3.6), 18, 2, shade);
    b.block_skins(body, sh, vec![(TOP, top), (FRONT, front), (BACK, back)]);
}

fn ps2(b: &mut Builder) {
    let (w, d) = (30.0f32, 18.0f32);
    let body = 0x17171d;
    let groove = 0x26262e;
    let inside = |x: f32, z: f32| rounded_rect(x, z, w, d, 0.7);
    let height = |_x: f32, _z: f32| 7.8;
    let sh = shell(w, d, 0.5, &inside, &height);
    let mut front = Canvas::for_face(w, 7.8, body);
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
    let mut top = Canvas::for_face(w, d, body);
    top.vents_h(0, t(2.0), t(w), 4, 8, groove);
    top.disc(t(24.0), t(10.0), t(1.6), 0x2e2e38); // logo disc
    b.block_skins(body, sh, vec![(FRONT, front), (TOP, top)]);
}

fn gc(b: &mut Builder) {
    let (w, d) = (15.0f32, 16.0f32);
    let body = 0x5048a0;
    let shade = 0x3e3880;
    let light = 0x6a62b8;
    let inside = |x: f32, z: f32| rounded_rect(x, z, w, d, 1.8);
    let height = |x: f32, z: f32| {
        let edge = (x.abs() / (w / 2.0)).max(z.abs() / (d / 2.0));
        11.0 - 0.35 * smoothstep(0.86, 1.0, edge) + dome(x, z, 0.0, 0.5, 5.6, 0.45)
    };
    let sh = shell(w, d, 0.5, &inside, &height);
    let hmax = sh.hmax;
    let mut front = Canvas::for_face(w, hmax, body);
    for x in [3.0f32, 6.0, 9.0, 12.0] {
        front.port(t(x), t(hmax - 5.6), t(1.2), 0x8a84c4, HOLE);
    }
    front.slot(t(2.0), t(hmax - 2.6), t(4.6), t(1.4), shade, HOLE);
    front.slot(t(8.4), t(hmax - 2.6), t(4.6), t(1.4), shade, HOLE);
    front.rect(t(1.0), t(hmax - 9.4), t(13.0), 1, shade);
    let mut top = Canvas::for_face(w, d, body);
    top.disc(t(7.5), t(8.5), t(5.6), light);
    top.ring(t(7.5), t(8.5), t(5.6), shade);
    top.ring(t(7.5), t(8.5), t(2.2), shade); // logo ring
    top.rect(t(7.5) - 1, t(8.5) - 1, 3, 3, shade);
    top.disc(t(13.3), t(14.0), 2, 0x9a94d0); // power
    top.disc(t(13.3), t(11.5), 1, 0x9a94d0); // reset
    top.vents_h(t(1.0), t(13.5), t(4.0), 4, 2, shade);
    let mut right = Canvas::for_face(d, hmax, body);
    right.dots(t(1.5), t(2.0), 12, 10, 2, shade); // vent grid
    let mut left = Canvas::for_face(d, hmax, body);
    left.dots(t(d - 1.5 - 11.0), t(2.0), 12, 10, 2, shade);
    b.block_skins(body, sh, vec![(FRONT, front), (TOP, top), (RIGHT, right), (LEFT, left)]);
    b.block(shade, &block(-4.5, 2.0, -8.9, 1.6, 1.6, 1.6, 1.6, 6.0)); // handle
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
    b.block(shade, &block(0.0, -1.3, 0.0, 10.0, 12.0, 8.0, 11.0, 1.3)); // stand
}

fn xbox(b: &mut Builder) {
    let (w, d) = (32.0f32, 26.0f32);
    let body = 0x121214;
    let edge = 0x26262a;
    let inside = |x: f32, z: f32| rounded_rect(x, z, w, d, 3.0);
    let height = |x: f32, z: f32| {
        let e = (x.abs() / (w / 2.0)).max(z.abs() / (d / 2.0));
        let mut h = 10.0 - 0.7 * smoothstep(0.8, 1.0, e) + dome(x, z, 0.0, -2.0, 5.2, 0.6);
        // The two diagonal grooves of the X.
        let k = (d / 2.0) / (w / 2.0);
        let d1 = (z + 2.0 - x * k).abs() / (1.0 + k * k).sqrt();
        let d2 = (z + 2.0 + x * k).abs() / (1.0 + k * k).sqrt();
        let jewel = ((x * x) + (z + 2.0) * (z + 2.0)).sqrt() < 5.4;
        if d1.min(d2) < 0.9 && !jewel {
            h -= 0.4;
        }
        h
    };
    let sh = shell(w, d, 0.5, &inside, &height);
    let hmax = sh.hmax;
    let mut top = Canvas::for_face(w, d, body);
    for i in 0..t(d) {
        let x0 = (i as f32 / t(d) as f32 * t(w) as f32) as i32;
        top.px(x0, i, edge);
        top.px(x0 + 1, i, edge);
        top.px(t(w) - 1 - x0, i, edge);
        top.px(t(w) - 2 - x0, i, edge);
    }
    top.disc(t(w / 2.0), t(d / 2.0 - 2.0), t(5.0), 0x1c1c20);
    top.disc(t(w / 2.0), t(d / 2.0 - 2.0), t(4.2), 0x5cc230);
    top.disc(t(w / 2.0), t(d / 2.0 - 2.0), t(1.6), 0x3f8e22);
    let mut front = Canvas::for_face(w, hmax, body);
    for x in [5.0f32, 11.0, 21.0, 27.0] {
        front.port(t(x), t(hmax - 3.4), t(1.6), 0x3a3a40, HOLE);
    }
    front.slot(t(6.0), t(hmax - 8.0), t(20.0), t(2.0), 0x2a2a30, 0x1a1a1e); // tray
    front.disc(t(29.0), t(hmax - 7.0), 2, 0x5cc230); // power
    front.disc(t(3.0), t(hmax - 7.0), 2, 0x3a3a40); // eject
    b.block_skins(body, sh, vec![(TOP, top), (FRONT, front)]);
}

fn x360(b: &mut Builder) {
    let (w, d) = (31.0f32, 26.0f32);
    let body = 0xe9e9ec;
    let shade = 0xc9cbd0;
    // Concave sides: the waist of the 360.
    let inside = |x: f32, z: f32| {
        let waist = 1.7 * (1.0 - (z / (d / 2.0)).powi(2));
        x.abs() <= w / 2.0 - waist && rounded_rect(x, z, w, d, 2.0)
    };
    let height = |x: f32, _z: f32| 8.3 - 0.7 * (1.0 - (x / (w / 2.0)).powi(2));
    let sh = shell(w, d, 0.5, &inside, &height);
    let hmax = sh.hmax;
    let mut front = Canvas::for_face(w, hmax, shade);
    front.rect(0, 0, t(w), 1, body);
    front.slot(t(2.0), t(hmax - 2.9), t(17.0), t(1.6), 0x3a3a40, 0x1a1a1e); // tray
    front.disc(t(25.5), t(hmax - 4.2), t(2.3), body);
    front.disc(t(25.5), t(hmax - 4.2), t(1.5), 0x58c843); // ring of light
    front.disc(t(25.5), t(hmax - 4.2), 1, body);
    front.slot(t(2.0), t(hmax - 6.9), t(2.6), t(1.6), 0x9fa2a8, HOLE); // memory units
    front.slot(t(5.4), t(hmax - 6.9), t(2.6), t(1.6), 0x9fa2a8, HOLE);
    let mut top = Canvas::for_face(w, d, body);
    top.dots(t(2.5), t(2.0), 30, 6, 2, shade);
    top.dots(t(2.5), t(19.0), 30, 6, 2, shade);
    top.rect(t(26.0), t(11.0), t(2.0), t(2.0), shade); // sticker
    let mut back = Canvas::for_face(w, hmax, body);
    back.dots(t(3.0), t(1.5), 48, 6, 2, shade);
    b.block_skins(body, sh, vec![(FRONT, front), (TOP, top), (BACK, back)]);
}

fn dc(b: &mut Builder) {
    let (w, d) = (19.0f32, 19.5f32);
    let body = 0xeeeeee;
    let shade = 0xd0d0d0;
    let inside = |x: f32, z: f32| rounded_rect(x, z, w, d, 4.5);
    let height = |x: f32, z: f32| {
        let zn = (z + d / 2.0) / d;
        7.6 - 0.6 * smoothstep(0.82, 1.0, zn) + dome(x, z, 0.0, -0.5, 7.6, 0.5) + dome(x, z, 0.0, -0.5, 1.6, 0.3)
    };
    let sh = shell(w, d, 0.5, &inside, &height);
    let hmax = sh.hmax;
    let mut top = Canvas::for_face(w, d, body);
    let (lcx, lcz) = (t(w / 2.0), t(d / 2.0 - 0.5));
    top.disc(lcx, lcz, t(7.6), 0xf6f6f6);
    top.ring(lcx, lcz, t(7.6), shade);
    top.disc(lcx, lcz, t(1.2), 0xf27a1a); // swirl
    top.px(lcx + 2, lcz - 2, body);
    top.slot(t(1.2), t(15.6), t(3.0), t(1.4), shade, 0xbdbdbd); // power
    top.slot(t(14.8), t(15.6), t(3.0), t(1.4), shade, 0xbdbdbd); // open
    top.px(t(5.0), t(16.3), 0xf27a1a);
    top.vents_h(t(1.0), t(1.0), t(5.0), 5, 2, shade);
    let mut front = Canvas::for_face(w, hmax, body);
    for x in [3.5f32, 7.5, 11.5, 15.5] {
        front.port(t(x), t(hmax - 3.2), t(1.2), 0xa8a8a8, HOLE);
    }
    b.block_skins(body, sh, vec![(TOP, top), (FRONT, front)]);
}

fn gb(b: &mut Builder) {
    let (w, d) = (9.0f32, 14.8f32); // face up: d runs top (back) → bottom (front)
    let body = 0xc7c6c0;
    let inside = |x: f32, z: f32| rounded_rect4(x, z, w, d, [0.8, 0.8, 2.8, 0.8]); // big bottom-right corner
    let height = |x: f32, z: f32| {
        let mut h = 3.2;
        h += plate(x, z, cx(w, 0.7), cz(d, 1.0), cx(w, 8.3), cz(d, 6.8), -0.2); // screen bezel inset
        h += plate(x, z, cx(w, 1.3), cz(d, 8.85), cx(w, 3.9), cz(d, 9.75), 0.35); // d-pad
        h += plate(x, z, cx(w, 2.15), cz(d, 8.0), cx(w, 3.05), cz(d, 10.6), 0.35);
        h += dome(x, z, cx(w, 6.0), cz(d, 10.6), 0.9, 0.4); // B
        h += dome(x, z, cx(w, 7.6), cz(d, 9.8), 0.9, 0.4); // A
        h
    };
    let sh = shell(w, d, 0.25, &inside, &height);
    let mut face = Canvas::for_face(w, d, body);
    face.rect(t(0.7), t(1.0), t(7.6), t(5.8), 0x3c3c48); // bezel
    face.rect(t(0.7), t(1.3), t(7.6), 1, 0x6e1f3f); // purple line
    face.rect(t(0.7), t(1.6), t(7.6), 1, 0x2f3bb3); // blue line
    face.rect(t(1.6), t(2.0), t(4.6), t(4.1), 0x8fa44a); // screen
    face.px(t(1.0), t(4.0), LED_RED);
    face.rect(t(1.0), t(7.6), t(4.0), 1, 0x4a4a52); // "Nintendo GAME BOY"
    face.rect(t(1.3), t(8.85), t(2.6), t(0.9), 0x2a2a30); // d-pad
    face.rect(t(2.15), t(8.0), t(0.9), t(2.6), 0x2a2a30);
    face.disc(t(6.0), t(10.6), t(0.6), 0xa8356b); // B
    face.disc(t(7.6), t(9.8), t(0.6), 0xa8356b); // A
    face.rect(t(2.5), t(12.6), t(1.4), 1, 0x7a7a80); // select
    face.rect(t(4.4), t(12.6), t(1.4), 1, 0x7a7a80); // start
    for i in 0..6 {
        face.rect(t(6.0) + i * 2, t(12.0) + i, 1, t(1.6), 0x9a9a94); // speaker
    }
    let mut back = Canvas::for_face(w, d, body);
    back.rect(t(1.5), t(2.0), t(6.0), t(4.0), 0xb3b2ac); // battery cover
    let mut hb = Builder::new();
    hb.block_skins(body, sh, vec![(TOP, face), (BOTTOM, back)]);
    for mut p in hb.parts {
        p.verts = stand_up(p.verts);
        b.parts.push(p);
    }
}

fn gba(b: &mut Builder) {
    let (w, d) = (14.5f32, 8.2f32);
    let body = 0x5a4fa8;
    let inside = |x: f32, z: f32| rounded_rect(x, z, w, d, 3.4);
    let height = |x: f32, z: f32| {
        let mut h = 2.5;
        h += plate(x, z, cx(w, 3.6), cz(d, 1.2), cx(w, 10.9), cz(d, 6.7), -0.15); // bezel inset
        h += plate(x, z, cx(w, 0.9), cz(d, 3.5), cx(w, 3.3), cz(d, 4.3), 0.3); // d-pad
        h += plate(x, z, cx(w, 1.7), cz(d, 2.7), cx(w, 2.5), cz(d, 5.1), 0.3);
        h += dome(x, z, cx(w, 12.0), cz(d, 4.0), 0.8, 0.3); // B
        h += dome(x, z, cx(w, 13.3), cz(d, 3.0), 0.8, 0.3); // A
        h
    };
    let sh = shell(w, d, 0.25, &inside, &height);
    let mut face = Canvas::for_face(w, d, body);
    face.rect(t(3.6), t(1.2), t(7.3), t(5.5), 0x2a2a35); // bezel
    face.rect(t(4.3), t(1.9), t(5.9), t(4.0), 0x9aa3b8); // screen
    face.rect(t(0.9), t(3.5), t(2.4), t(0.8), 0x2a2a30); // d-pad
    face.rect(t(1.7), t(2.7), t(0.8), t(2.4), 0x2a2a30);
    face.disc(t(12.0), t(4.0), t(0.5), 0xc7c3e3); // B
    face.disc(t(13.3), t(3.0), t(0.5), 0xc7c3e3); // A
    face.rect(t(1.2), t(6.6), t(1.3), 1, 0xc7c3e3); // select
    face.rect(t(1.2), t(7.3), t(1.3), 1, 0xc7c3e3); // start
    face.px(t(3.8), t(1.6), LED_GREEN);
    for i in 0..5 {
        face.rect(t(12.0) + i, t(6.4) + i, t(0.6), 1, 0x8a82c8); // speaker
    }
    let mut hb = Builder::new();
    hb.block_skins(body, sh, vec![(TOP, face)]);
    hb.block(0x3f3870, &block(-5.4, 0.4, -4.4, 4.0, 1.4, 4.0, 1.2, 1.7)); // L (at the back = top once upright)
    hb.block(0x3f3870, &block(5.4, 0.4, -4.4, 4.0, 1.4, 4.0, 1.2, 1.7)); // R
    for mut p in hb.parts {
        p.verts = stand_up(p.verts);
        b.parts.push(p);
    }
}

fn ds(b: &mut Builder, is3: bool) {
    let shell_c = if is3 { 0xb7202e } else { 0xf2f2f4 };
    let inner = if is3 { 0x202024 } else { 0xf7f7f9 };
    let detail = if is3 { 0x3a3a40 } else { 0xb9bfc8 };
    let (w, d) = (13.4f32, 7.4f32);
    let inside = |x: f32, z: f32| rounded_rect(x, z, w, d, 0.9);
    // Bottom half: screen inset, raised d-pad and buttons.
    let height = |x: f32, z: f32| {
        let mut h = 1.1;
        h += plate(x, z, cx(w, 3.9), cz(d, 1.3), cx(w, 9.5), cz(d, 5.5), -0.1);
        h += plate(x, z, cx(w, 0.8), cz(d, 3.0), cx(w, 3.0), cz(d, 3.7), 0.2);
        h += plate(x, z, cx(w, 1.55), cz(d, 2.25), cx(w, 2.25), cz(d, 4.45), 0.2);
        for (bx, bz) in [(10.8f32, 3.3f32), (11.8, 2.3), (11.8, 4.3), (12.8, 3.3)] {
            h += dome(x, z, cx(w, bx), cz(d, bz), 0.6, 0.2);
        }
        if is3 {
            h += dome(x, z, cx(w, 1.9), cz(d, 1.4), 1.0, 0.25);
        }
        h
    };
    let sh = shell(w, d, 0.25, &inside, &height);
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
    let mut front = Canvas::for_face(w, 1.1, shell_c);
    front.rect(t(4.0), 1, t(5.0), 1, detail); // cartridge slot
    let mut sides = Canvas::for_face(d, 1.1, shell_c);
    sides.rect(t(1.0), 1, t(2.0), 1, detail); // volume slider
    b.block_skins(shell_c, sh, vec![(TOP, top), (FRONT, front), (LEFT, sides)]);
    // Lid: built flat (inner face up), stood up, moved onto the hinge and tilted back.
    let lid_h = |x: f32, z: f32| {
        1.1 + plate(x, z, cx(w, if is3 { 2.9 } else { 3.9 }), cz(d, 1.3), cx(w, if is3 { 10.6 } else { 9.5 }), cz(d, 5.6), -0.1)
    };
    let lid = shell(w, d, 0.25, &inside, &lid_h);
    let mut face = Canvas::for_face(w, d, inner);
    face.rect(t(if is3 { 2.9 } else { 3.9 }), t(1.3), t(if is3 { 7.7 } else { 5.6 }), t(4.3), SCREEN);
    for i in 0..4 {
        face.px(t(1.2) + i * 2, t(3.5), detail); // speaker dots
        face.px(t(w - 1.2) - i * 2, t(3.5), detail);
    }
    let mut outer = Canvas::for_face(w, d, shell_c);
    outer.disc(t(w / 2.0), t(d / 2.0), 2, if is3 { 0x8a1822 } else { 0xd8d8dc }); // logo
    outer.px(t(w - 1.0), t(1.0), LED_GREEN);
    let hinge = [0.0, 0.55, -d / 2.0];
    let mut lb = Builder::new();
    lb.block_skins(shell_c, lid, vec![(TOP, face), (BOTTOM, outer)]);
    for mut p in lb.parts {
        let v = stand_up(p.verts);
        let v = translate(v, 0.0, d / 2.0 + 0.55, -d / 2.0 - 0.55);
        p.verts = rotate_x_about(v, -0.42, hinge);
        b.parts.push(p);
    }
    b.cyl(shell_c, hinge, 0.6, w - 1.0, Axis::X);
}

fn psp(b: &mut Builder) {
    let (w, d) = (17.0f32, 7.4f32);
    let body = 0x15151a;
    let inside = |x: f32, z: f32| rounded_rect(x, z, w, d, 3.6);
    let height = |x: f32, z: f32| {
        let mut h = 2.3;
        h += plate(x, z, cx(w, 3.7), cz(d, 1.0), cx(w, 13.3), cz(d, 6.4), -0.15); // screen
        h += plate(x, z, cx(w, 0.8), cz(d, 2.9), cx(w, 3.0), cz(d, 3.6), 0.25); // d-pad
        h += plate(x, z, cx(w, 1.55), cz(d, 2.15), cx(w, 2.25), cz(d, 4.35), 0.25);
        h += dome(x, z, cx(w, 1.9), cz(d, 6.0), 0.8, 0.35); // analog nub
        for (bx, bz) in [(14.4f32, 2.2f32), (13.3, 3.3), (15.5, 3.3), (14.4, 4.4)] {
            h += dome(x, z, cx(w, bx), cz(d, bz), 0.6, 0.25);
        }
        h
    };
    let sh = shell(w, d, 0.25, &inside, &height);
    let mut face = Canvas::for_face(w, d, body);
    face.rect(t(3.7), t(1.0), t(9.6), t(5.4), 0x2a3350); // screen
    face.rect(t(3.9), t(1.2), t(9.2), 1, 0x3a4668); // glare line
    face.rect(t(0.8), t(2.9), t(2.2), t(0.7), 0x2e2e36); // d-pad
    face.rect(t(1.55), t(2.15), t(0.7), t(2.2), 0x2e2e36);
    face.disc(t(1.9), t(6.0), t(0.7), 0x55555c); // analog nub
    for (x, y) in [(14.4f32, 2.2f32), (13.3, 3.3), (15.5, 3.3), (14.4, 4.4)] {
        face.disc(t(x), t(y), 1, 0xb9b9c0);
    }
    face.rect(t(13.6), t(6.3), t(1.2), 1, 0x8a8a92); // select
    face.rect(t(15.2), t(6.3), t(1.2), 1, 0x8a8a92); // start
    face.rect(t(7.0), t(6.6), t(3.0), 1, 0x8a8a92); // "PSP" bar
    face.px(t(15.8), t(0.6), LED_GREEN);
    let mut hb = Builder::new();
    hb.block_skins(body, sh, vec![(TOP, face)]);
    hb.block(0x2a2a30, &block(-6.5, 0.2, -3.85, 3.5, 1.0, 3.5, 0.9, 1.6)); // L
    hb.block(0x2a2a30, &block(6.5, 0.2, -3.85, 3.5, 1.0, 3.5, 0.9, 1.6)); // R
    for mut p in hb.parts {
        p.verts = stand_up(p.verts);
        b.parts.push(p);
    }
}

fn ps5(b: &mut Builder) {
    let plate_c = 0xf4f4f6;
    let core = block(0.0, 0.0, 0.0, 8.0, 24.0, 8.0, 24.0, 38.0);
    let mut front = Canvas::for_face(8.0, 38.0, 0x111115);
    front.rect(t(3.6), t(6.0), 2, t(14.0), 0x2a2a30); // disc slot
    front.disc(t(4.0), t(26.0), 1, 0x8a8a92); // power
    front.disc(t(4.0), t(28.5), 1, 0x8a8a92); // eject
    front.rect(t(2.6), t(31.0), t(2.8), 1, 0x2a2a30); // USB
    b.block_skins(0x111115, core, vec![(FRONT, front)]);
    b.block(plate_c, &block(-4.8, -1.0, 0.0, 1.6, 26.0, 2.6, 26.0, 40.0)); // plates, flared
    b.block(plate_c, &block(4.8, -1.0, 0.0, 1.6, 26.0, 2.6, 26.0, 40.0));
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
    front.dots(t(2.0), t(2.0), 26, 62, 2, 0x15161a); // mesh
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
    top.dots(t(3.0), t(3.0), 22, 58, 2, 0x15161a);
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
