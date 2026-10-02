//! Procedural low-poly consoles for the immersive carousel: every system is
//! built from a handful of cuboids and cylinders at (roughly) real-world
//! proportions, in a flat, toy-like style that stays coherent across the
//! whole set, with rounded edges and smooth shading (no paper-box look). No external assets needed; a glTF in the models dir still wins.
//!
//! Coordinates: centimetres, y up, +z towards the viewer (the "front" of the
//! console). `scene` normalises the result like any other model.

use crate::box3d::cube;

pub struct Part {
    pub color: [f32; 3],
    /// 0 = matte plastic, 1 = glossy (screens, lacquer).
    pub gloss: f32,
    /// Interleaved pos3 / nrm3 / uv2.
    pub verts: Vec<f32>,
}

fn rgb(hex: u32) -> [f32; 3] {
    [((hex >> 16) & 255) as f32 / 255.0, ((hex >> 8) & 255) as f32 / 255.0, (hex & 255) as f32 / 255.0]
}

/// Axis-aligned cuboid centred at `c` with full size `s`.
fn cuboid(c: [f32; 3], s: [f32; 3]) -> Vec<f32> {
    let mut v = cube(1.0, 1.0); // unit cube, height 1, centred
    for i in (0..v.len()).step_by(8) {
        v[i] = v[i] * s[0] + c[0];
        v[i + 1] = v[i + 1] * s[1] + c[1];
        v[i + 2] = v[i + 2] * s[2] + c[2];
    }
    v
}

/// Cuboid with rounded edges and corners (radius `r`), smooth-shaded: every
/// face is a grid whose points are projected onto the rounded-box surface.
fn rounded_box(c: [f32; 3], s: [f32; 3], r: f32, n: usize) -> Vec<f32> {
    let h = [s[0] / 2.0, s[1] / 2.0, s[2] / 2.0];
    let r = r.min(h[0]).min(h[1]).min(h[2]).max(0.0);
    let inner = [h[0] - r, h[1] - r, h[2] - r];
    // Point on the unit-cube surface → (position, normal) on the rounded box.
    let surf = |u: [f32; 3]| -> ([f32; 3], [f32; 3]) {
        let q = [u[0] * h[0], u[1] * h[1], u[2] * h[2]];
        let k = [
            q[0].clamp(-inner[0], inner[0]),
            q[1].clamp(-inner[1], inner[1]),
            q[2].clamp(-inner[2], inner[2]),
        ];
        let d = [q[0] - k[0], q[1] - k[1], q[2] - k[2]];
        let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        if l < 1e-6 {
            return ([k[0] + c[0], k[1] + c[1], k[2] + c[2]], [0.0, 1.0, 0.0]);
        }
        let nrm = [d[0] / l, d[1] / l, d[2] / l];
        ([k[0] + nrm[0] * r + c[0], k[1] + nrm[1] * r + c[1], k[2] + nrm[2] * r + c[2]], nrm)
    };
    // Each face: axis, sign, and the two tangent axes (ordered for CCW winding).
    let faces: [(usize, f32, usize, usize); 6] = [
        (2, 1.0, 0, 1),  // +z front
        (2, -1.0, 1, 0), // -z back
        (0, 1.0, 1, 2),  // +x
        (0, -1.0, 2, 1), // -x
        (1, 1.0, 2, 0),  // +y top
        (1, -1.0, 0, 2), // -y bottom
    ];
    let mut v = Vec::with_capacity(6 * n * n * 6 * 8);
    let mut push = |u: [f32; 3]| {
        let (p, nrm) = surf(u);
        v.extend_from_slice(&p);
        v.extend_from_slice(&nrm);
        v.extend_from_slice(&[0.0, 0.0]);
    };
    for (ax, sign, ta, tb) in faces {
        for i in 0..n {
            for j in 0..n {
                let (a0, a1) = (i as f32 / n as f32 * 2.0 - 1.0, (i + 1) as f32 / n as f32 * 2.0 - 1.0);
                let (b0, b1) = (j as f32 / n as f32 * 2.0 - 1.0, (j + 1) as f32 / n as f32 * 2.0 - 1.0);
                let mk = |a: f32, b: f32| {
                    let mut u = [0.0; 3];
                    u[ax] = sign;
                    u[ta] = a;
                    u[tb] = b;
                    u
                };
                push(mk(a0, b0));
                push(mk(a1, b0));
                push(mk(a1, b1));
                push(mk(a0, b0));
                push(mk(a1, b1));
                push(mk(a0, b1));
            }
        }
    }
    v
}

#[derive(Clone, Copy)]
enum Axis {
    X,
    Y,
    Z,
}

/// Closed cylinder centred at `c`, radius `r`, length `h` along `axis`.
fn cylinder(c: [f32; 3], r: f32, h: f32, axis: Axis, seg: usize) -> Vec<f32> {
    // Build along Y, then swap axes.
    let mut v: Vec<f32> = Vec::with_capacity(seg * 12 * 8);
    let (y0, y1) = (-h / 2.0, h / 2.0);
    let mut push = |p: [f32; 3], n: [f32; 3]| {
        v.extend_from_slice(&p);
        v.extend_from_slice(&n);
        v.extend_from_slice(&[0.0, 0.0]);
    };
    for i in 0..seg {
        let a0 = i as f32 / seg as f32 * std::f32::consts::TAU;
        let a1 = (i + 1) as f32 / seg as f32 * std::f32::consts::TAU;
        let (s0, c0) = a0.sin_cos();
        let (s1, c1) = a1.sin_cos();
        let p00 = [r * c0, y0, -r * s0];
        let p01 = [r * c1, y0, -r * s1];
        let p10 = [r * c0, y1, -r * s0];
        let p11 = [r * c1, y1, -r * s1];
        // Smooth side normals.
        let n0 = [c0, 0.0, -s0];
        let n1 = [c1, 0.0, -s1];
        // Side quad (CCW seen from outside).
        push(p00, n0);
        push(p01, n1);
        push(p11, n1);
        push(p00, n0);
        push(p11, n1);
        push(p10, n0);
        // Top cap (normal +y) and bottom cap (normal -y).
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
            Axis::X => ([y, x, z], [ny, nx, nz]), // swap x/y (mirror; winding fixed below)
            Axis::Z => ([x, z, y], [nx, nz, ny]), // swap y/z
        };
        v[i] = p[0] + c[0];
        v[i + 1] = p[1] + c[1];
        v[i + 2] = p[2] + c[2];
        v[i + 3] = n[0];
        v[i + 4] = n[1];
        v[i + 5] = n[2];
    }
    if !matches!(axis, Axis::Y) {
        // An axis swap mirrors the geometry: flip triangle winding so culling
        // keeps the outside faces.
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

/// Rotates `verts` around the Z axis by `a` radians about `pivot`.
fn rotate_z_about(mut verts: Vec<f32>, a: f32, pivot: [f32; 3]) -> Vec<f32> {
    let (s, c) = a.sin_cos();
    for i in (0..verts.len()).step_by(8) {
        let x = verts[i] - pivot[0];
        let y = verts[i + 1] - pivot[1];
        verts[i] = x * c - y * s + pivot[0];
        verts[i + 1] = x * s + y * c + pivot[1];
        let nx = verts[i + 3];
        let ny = verts[i + 4];
        verts[i + 3] = nx * c - ny * s;
        verts[i + 4] = nx * s + ny * c;
    }
    verts
}

fn auto_radius(s: [f32; 3]) -> f32 {
    (s[0].min(s[1]).min(s[2]) * 0.34).min(0.9)
}

struct Builder {
    parts: Vec<Part>,
}

impl Builder {
    fn new() -> Self {
        Self { parts: Vec::new() }
    }
    fn add(&mut self, color: [f32; 3], gloss: f32, verts: Vec<f32>) -> &mut Self {
        self.parts.push(Part { color, gloss, verts });
        self
    }
    /// Rounded box with an automatic edge radius (a third of the thinnest side,
    /// capped), the default look of every part.
    fn boxp(&mut self, color: [f32; 3], c: [f32; 3], s: [f32; 3]) -> &mut Self {
        self.add(color, 0.0, rounded_box(c, s, auto_radius(s), 6))
    }
    fn boxg(&mut self, color: [f32; 3], c: [f32; 3], s: [f32; 3]) -> &mut Self {
        self.add(color, 1.0, rounded_box(c, s, auto_radius(s), 6))
    }
    /// Rounded box with an explicit radius (bodies with softer edges).
    fn boxr(&mut self, color: [f32; 3], c: [f32; 3], s: [f32; 3], r: f32) -> &mut Self {
        self.add(color, 0.0, rounded_box(c, s, r, 8))
    }
    fn cyl(&mut self, color: [f32; 3], c: [f32; 3], r: f32, h: f32, axis: Axis) -> &mut Self {
        self.add(color, 0.0, cylinder(c, r, h, axis, 36))
    }
}

// Shared palette (keeps the set coherent).
const PORT: u32 = 0x1a1a1f; // controller ports / slots
const SCREEN: u32 = 0x1d2433;

/// Builds the console of `system`, or None for unknown ids.
pub fn build(system: &str) -> Option<Vec<Part>> {
    let mut b = Builder::new();
    match system {
        "n64" => {
            let body = rgb(0x4a4a53);
            b.boxr(body, [0.0, 0.0, 0.0], [26.0, 5.5, 19.0], 1.4);
            // Shoulders either side of the cartridge bay.
            b.boxr(body, [-8.5, 3.6, -2.0], [8.5, 2.6, 13.0], 1.2);
            b.boxr(body, [8.5, 3.6, -2.0], [8.5, 2.6, 13.0], 1.2);
            b.boxp(rgb(0x33333a), [0.0, 3.4, -2.0], [8.5, 2.2, 13.0]); // bay
            b.boxp(rgb(0x8d8d93), [0.0, 6.4, -2.0], [7.4, 3.2, 2.0]); // cartridge
            b.boxp(rgb(0xd33a3a), [0.0, 6.4, -0.9], [5.0, 2.0, 0.3]); // label
            // Front: four controller ports, power/reset on top-left.
            for x in [-7.5, -2.5, 2.5, 7.5] {
                b.cyl(rgb(PORT), [x, -0.3, 9.6], 1.3, 0.6, Axis::Z);
            }
            b.boxp(rgb(0x55565e), [-9.5, 2.9, 5.5], [3.5, 0.5, 2.2]);
            b.boxp(rgb(0x55565e), [-5.0, 2.9, 5.5], [2.4, 0.5, 2.2]);
            b.boxp(rgb(0x4fa64f), [9.0, 2.9, 6.5], [2.2, 0.3, 2.2]); // logo chip
        }
        "psx" => {
            let body = rgb(0xcfccc3);
            b.boxr(body, [0.0, 0.0, 0.0], [27.0, 6.0, 19.0], 1.6);
            b.cyl(rgb(0xd8d5cc), [3.0, 3.3, -1.0], 7.6, 0.6, Axis::Y); // lid
            b.boxp(rgb(0x8e8c85), [-9.5, 3.2, 5.5], [3.2, 0.4, 2.0]); // power
            b.boxp(rgb(0x8e8c85), [-9.5, 3.2, 2.0], [3.2, 0.4, 2.0]); // reset
            b.boxp(rgb(0x8e8c85), [-9.5, 3.2, -3.0], [3.2, 0.4, 2.0]); // open
            for x in [-7.0, -2.0] {
                b.boxp(rgb(PORT), [x, -0.8, 9.6], [4.2, 2.4, 0.4]); // pads
                b.boxp(rgb(PORT), [x, 1.6, 9.6], [4.2, 0.9, 0.4]); // memory cards
            }
        }
        "ps2" => {
            let body = rgb(0x17171d);
            b.boxr(body, [0.0, 0.0, 0.0], [30.0, 7.8, 18.0], 0.8);
            b.boxp(rgb(0x2244cc), [-13.4, 0.0, 9.1], [1.6, 7.8, 0.4]); // blue edge
            b.boxp(rgb(0x2a2a33), [4.0, 2.4, 9.1], [17.0, 1.4, 0.4]); // tray
            for x in [-7.0, -2.5] {
                b.boxp(rgb(PORT), [x, -1.6, 9.1], [3.6, 2.0, 0.4]);
            }
            for i in 0..4 {
                let y = -3.2 + i as f32 * 0.9; // horizontal grooves on the front
                b.boxp(rgb(0x202027), [2.0, y, 9.2], [24.0, 0.25, 0.2]);
            }
            b.cyl(rgb(0x3a3a44), [9.0, -2.0, 9.2], 0.8, 0.3, Axis::Z); // reset
            b.cyl(rgb(0x3a3a44), [11.5, -2.0, 9.2], 0.8, 0.3, Axis::Z); // eject
        }
        "gc" => {
            let body = rgb(0x4a3e93);
            b.boxr(body, [0.0, 0.0, 0.0], [15.0, 11.0, 16.0], 1.6);
            b.boxp(rgb(0x5d50ad), [0.0, 5.7, 0.5], [12.5, 0.4, 12.5]); // lid
            b.cyl(rgb(0x3e3480), [0.0, 6.05, 0.5], 4.6, 0.3, Axis::Y); // disc dome
            // Handle at the back.
            b.boxp(rgb(0x3e3480), [-4.5, 2.0, -9.0], [1.6, 6.0, 1.6]);
            b.boxp(rgb(0x3e3480), [4.5, 2.0, -9.0], [1.6, 6.0, 1.6]);
            b.boxp(rgb(0x3e3480), [0.0, 5.5, -9.0], [10.6, 1.6, 1.6]);
            for x in [-4.5, -1.5, 1.5, 4.5] {
                b.cyl(rgb(PORT), [x, 2.6, 8.1], 1.1, 0.4, Axis::Z);
            }
            b.boxp(rgb(PORT), [-3.0, -1.5, 8.1], [3.0, 1.2, 0.4]);
            b.boxp(rgb(PORT), [3.0, -1.5, 8.1], [3.0, 1.2, 0.4]);
            b.cyl(rgb(0x8e8ab8), [6.0, 5.8, 6.0], 0.7, 0.3, Axis::Y); // power
        }
        "wii" => {
            b.boxr(rgb(0xeef0f3), [0.0, 0.0, 0.0], [4.4, 21.5, 15.7], 0.8);
            b.boxp(rgb(0x7fb8ff), [0.0, 5.0, 7.95], [0.4, 12.0, 0.3]); // disc slot glow
            b.boxp(rgb(0xd8dbe0), [0.0, -5.0, 7.95], [3.6, 2.4, 0.2]); // sd door
            b.cyl(rgb(0xc9cdd3), [0.0, -8.5, 7.95], 0.5, 0.2, Axis::Z); // power
            b.boxp(rgb(0xcfd3d8), [0.0, -11.3, 0.0], [10.0, 1.2, 12.0]); // stand
        }
        "xbox" => {
            b.boxr(rgb(0x121214), [0.0, 0.0, 0.0], [32.0, 10.0, 26.0], 2.0);
            b.cyl(rgb(0x1a1a1e), [0.0, 5.1, -2.0], 7.0, 0.4, Axis::Y); // raised ring
            b.cyl(rgb(0x5cc230), [0.0, 5.3, -2.0], 4.6, 0.4, Axis::Y); // jewel
            for x in [-11.0, -5.0, 5.0, 11.0] {
                b.cyl(rgb(PORT), [x, -1.5, 13.1], 1.5, 0.5, Axis::Z);
            }
            b.boxp(rgb(0x202024), [0.0, 2.6, 13.1], [20.0, 1.6, 0.4]); // tray
            b.cyl(rgb(0x5cc230), [13.0, 2.6, 13.2], 1.0, 0.4, Axis::Z); // power
            b.cyl(rgb(0x3a3a40), [-13.0, 2.6, 13.2], 1.0, 0.4, Axis::Z); // eject
        }
        "x360" => {
            b.boxr(rgb(0xe9e9ec), [0.0, 0.0, 0.0], [31.0, 8.3, 26.0], 1.8);
            b.boxr(rgb(0xc9cbd0), [0.0, 0.0, 12.9], [29.4, 7.4, 1.2], 1.0); // faceplate
            b.boxr(rgb(0x9fa2a8), [0.0, 0.0, -12.9], [28.0, 6.5, 0.6], 0.3); // back vents
            b.boxp(rgb(0x3a3a40), [-4.0, 1.8, 13.5], [17.0, 1.6, 0.2]); // tray
            b.cyl(rgb(0xdfe1e6), [9.5, 0.0, 13.6], 2.3, 0.3, Axis::Z); // ring
            b.cyl(rgb(0x58c843), [9.5, 0.0, 13.8], 1.4, 0.2, Axis::Z); // power led
            for x in [-11.0, -7.5] {
                b.boxp(rgb(PORT), [x, -2.0, 13.5], [2.4, 1.6, 0.2]); // memory units
            }
        }
        "dc" => {
            b.boxr(rgb(0xeeeeee), [0.0, 0.0, 0.0], [19.0, 7.6, 19.5], 2.2);
            b.cyl(rgb(0xf4f4f4), [0.0, 4.1, -1.0], 7.6, 0.6, Axis::Y); // lid
            b.cyl(rgb(0xf27a1a), [0.0, 4.55, -1.0], 1.7, 0.3, Axis::Y); // swirl
            for x in [-6.0, -2.0, 2.0, 6.0] {
                b.cyl(rgb(PORT), [x, -1.2, 9.9], 1.2, 0.5, Axis::Z);
            }
            b.boxp(rgb(0xd3d3d3), [-7.5, 4.0, 6.5], [3.0, 0.4, 2.0]); // power
            b.boxp(rgb(0xd3d3d3), [7.5, 4.0, 6.5], [3.0, 0.4, 2.0]); // open
            b.cyl(rgb(0xf27a1a), [-7.5, 4.3, 3.0], 0.4, 0.3, Axis::Y); // led
        }
        "gb" => {
            b.boxr(rgb(0xc7c6c0), [0.0, 0.0, 0.0], [9.0, 14.8, 3.2], 1.0);
            b.boxp(rgb(0x3c3c48), [0.0, 3.4, 1.7], [7.6, 5.8, 0.3]); // bezel
            b.boxg(rgb(0x8fa44a), [-0.4, 3.4, 1.9], [4.6, 4.1, 0.2]); // screen
            b.boxp(rgb(0x6e1f3f), [0.0, 6.0, 1.9], [7.6, 0.25, 0.15]); // purple stripe
            b.boxp(rgb(PORT), [-2.4, -2.3, 1.8], [2.6, 0.9, 0.5]); // d-pad
            b.boxp(rgb(PORT), [-2.4, -2.3, 1.8], [0.9, 2.6, 0.5]);
            b.cyl(rgb(0xa8356b), [1.7, -2.9, 1.9], 0.6, 0.5, Axis::Z); // B
            b.cyl(rgb(0xa8356b), [3.2, -2.1, 1.9], 0.6, 0.5, Axis::Z); // A
            b.boxp(rgb(0x7a7a80), [-0.9, -5.3, 1.7], [1.4, 0.45, 0.3]); // select
            b.boxp(rgb(0x7a7a80), [0.9, -5.3, 1.7], [1.4, 0.45, 0.3]); // start
        }
        "gba" => {
            b.boxr(rgb(0x5a4fa8), [0.0, 0.0, 0.0], [14.5, 8.2, 2.5], 1.1);
            b.boxp(rgb(0x2a2a35), [0.0, 0.5, 1.3], [7.2, 5.4, 0.3]); // bezel
            b.boxg(rgb(0x9aa3b8), [0.0, 0.5, 1.5], [6.0, 4.0, 0.2]); // screen
            b.boxp(rgb(PORT), [-5.6, 0.4, 1.4], [2.4, 0.8, 0.4]); // d-pad
            b.boxp(rgb(PORT), [-5.6, 0.4, 1.4], [0.8, 2.4, 0.4]);
            b.cyl(rgb(0xc7c3e3), [5.1, -0.1, 1.5], 0.55, 0.4, Axis::Z); // B
            b.cyl(rgb(0xc7c3e3), [6.4, 0.7, 1.5], 0.55, 0.4, Axis::Z); // A
            b.boxp(rgb(0x3f3870), [-5.4, 4.3, -0.2], [4.0, 0.5, 1.6]); // L
            b.boxp(rgb(0x3f3870), [5.4, 4.3, -0.2], [4.0, 0.5, 1.6]); // R
            b.boxp(rgb(0xc7c3e3), [-3.0, -2.6, 1.35], [1.3, 0.4, 0.2]);
            b.boxp(rgb(0xc7c3e3), [-3.0, -3.4, 1.35], [1.3, 0.4, 0.2]);
        }
        "nds" | "3ds" => {
            let is3 = system == "3ds";
            let shell = if is3 { rgb(0xb7202e) } else { rgb(0xf2f2f4) };
            let inner = if is3 { rgb(0x202024) } else { rgb(0xf7f7f9) };
            let (w, d, lid_h) = if is3 { (13.4, 7.4, 7.4) } else { (13.3, 7.4, 7.4) };
            b.boxr(shell, [0.0, 0.0, 0.0], [w, 1.1, d], 0.5); // bottom half
            b.boxp(inner, [0.0, 0.58, 0.0], [w - 0.8, 0.1, d - 0.8]); // inner face
            b.boxg(rgb(SCREEN), [0.0, 0.66, 0.2], [if is3 { 5.6 } else { 5.6 }, 0.1, 4.2]); // bottom screen
            b.boxp(rgb(PORT), [-4.6, 0.7, 0.4], [2.2, 0.15, 0.7]); // d-pad
            b.boxp(rgb(PORT), [-4.6, 0.7, 0.4], [0.7, 0.15, 2.2]);
            for (x, z) in [(4.0, 0.4), (5.1, -0.6), (5.1, 1.4), (6.2, 0.4)] {
                b.cyl(rgb(if is3 { 0x3a3a40 } else { 0x9aa0a8 }), [x, 0.72, z], 0.45, 0.15, Axis::Y);
            }
            if is3 {
                b.cyl(rgb(0x46464c), [-4.6, 0.75, -2.0], 0.9, 0.2, Axis::Y); // circle pad
            }
            // Lid, hinged at the back and tilted ~110°.
            let hinge = [0.0, 0.55, -d / 2.0];
            let tilt = -0.42f32; // leaning back ~25° from upright
            let lid = rounded_box([0.0, 0.55 + lid_h / 2.0, -d / 2.0 + 0.55], [w, lid_h, 1.1], 0.5, 8);
            b.add(shell, 0.0, rotate_x_about(lid, tilt, hinge));
            let face = cuboid([0.0, 0.55 + lid_h / 2.0, -d / 2.0 + 1.11], [w - 0.8, lid_h - 0.8, 0.05]);
            b.add(inner, 0.0, rotate_x_about(face, tilt, hinge));
            let scr_w = if is3 { 7.7 } else { 5.6 };
            let screen = cuboid([0.0, 0.55 + lid_h / 2.0, -d / 2.0 + 1.16], [scr_w, 4.3, 0.05]);
            b.add(rgb(SCREEN), 1.0, rotate_x_about(screen, tilt, hinge));
            b.cyl(shell, hinge, 0.6, w - 1.0, Axis::X);
        }
        "psp" => {
            b.boxr(rgb(0x15151a), [0.0, 0.0, 0.0], [17.0, 7.4, 2.3], 1.1);
            b.boxg(rgb(0x2a3350), [0.0, 0.35, 1.2], [9.6, 5.4, 0.2]); // screen
            b.boxp(rgb(PORT), [-6.6, 0.8, 1.25], [2.2, 0.7, 0.3]); // d-pad
            b.boxp(rgb(PORT), [-6.6, 0.8, 1.25], [0.7, 2.2, 0.3]);
            b.cyl(rgb(0x55555c), [-6.6, -2.3, 1.3], 0.75, 0.4, Axis::Z); // analog nub
            for (x, y) in [(6.6, 1.9), (5.4, 0.8), (7.8, 0.8), (6.6, -0.3)] {
                b.cyl(rgb(0xb9b9c0), [x, y, 1.3], 0.45, 0.3, Axis::Z);
            }
            b.boxp(rgb(0x2a2a30), [-6.5, 3.75, 0.2], [3.5, 0.3, 1.4]); // L
            b.boxp(rgb(0x2a2a30), [6.5, 3.75, 0.2], [3.5, 0.3, 1.4]); // R
            b.boxp(rgb(0xb9b9c0), [4.5, -2.8, 1.2], [1.2, 0.3, 0.2]); // start
            b.boxp(rgb(0xb9b9c0), [2.8, -2.8, 1.2], [1.2, 0.3, 0.2]); // select
        }
        "ps5" => {
            b.boxr(rgb(0x111115), [0.0, 0.0, 0.0], [8.0, 38.0, 24.0], 2.5); // core
            // White side plates, flared outwards at the top.
            let l = rounded_box([-4.7, 0.0, 0.0], [1.6, 40.0, 26.0], 0.8, 8);
            b.add(rgb(0xf4f4f6), 0.0, rotate_z_about(l, 0.035, [-4.0, -19.0, 0.0]));
            let r = rounded_box([4.7, 0.0, 0.0], [1.6, 40.0, 26.0], 0.8, 8);
            b.add(rgb(0xf4f4f6), 0.0, rotate_z_about(r, -0.035, [4.0, -19.0, 0.0]));
            b.boxg(rgb(0x4f8cff), [0.0, 19.3, 0.0], [7.4, 0.5, 22.0]); // light strip
            b.cyl(rgb(0x1a1a1f), [0.0, -19.6, 0.0], 7.0, 0.9, Axis::Y); // stand
            b.boxp(rgb(0x2a2a30), [0.0, 8.0, 12.05], [1.2, 14.0, 0.3]); // disc slot
        }
        "pc" => {
            b.boxr(rgb(0x1c1d22), [0.0, 0.0, 0.0], [21.0, 45.0, 45.0], 1.2);
            b.boxg(rgb(0x2a3a5a), [10.6, 1.0, 0.0], [0.3, 40.0, 41.0]); // glass side
            b.boxp(rgb(0x26272d), [0.0, 0.0, 22.6], [19.5, 43.0, 0.3]); // front mesh
            for y in [-13.0, 0.0, 13.0] {
                b.cyl(rgb(0x3ad6c8), [0.0, y, 22.9], 5.6, 0.3, Axis::Z); // RGB fans
                b.cyl(rgb(0x1c1d22), [0.0, y, 23.05], 2.0, 0.3, Axis::Z);
            }
            b.cyl(rgb(0x3ad6c8), [4.0, 22.6, 14.0], 1.0, 0.3, Axis::Y); // power
            for (x, z) in [(-8.0, -18.0), (8.0, -18.0), (-8.0, 18.0), (8.0, 18.0)] {
                b.boxp(rgb(0x111114), [x, -23.0, z], [3.0, 1.2, 4.0]); // feet
            }
        }
        _ => return None,
    }
    Some(b.parts)
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
