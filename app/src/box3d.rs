//! Experimental 3D box viewer: draws a textured game box with OpenGL on top of
//! the Slint scene (rendering notifier, `AfterRendering`). Slint owns the dim
//! backdrop, the drag area and the buttons; this module only paints the box
//! inside the region the UI reports.
//!
//! Shaders are written without a `#version` line so they compile on both
//! desktop compatibility contexts (GLSL 1.10) and GLES 2 (GLSL ES 1.00).

use glow::HasContext;
use std::cell::RefCell;
use std::rc::Rc;

/// Everything the UI thread shares with the renderer.
#[derive(Default)]
pub struct ViewerState {
    pub visible: bool,
    /// Region in physical pixels where the box is drawn (x, y from top-left).
    pub region: (f32, f32, f32, f32),
    pub yaw: f32,
    pub pitch: f32,
    pub vel_yaw: f32,
    pub vel_pitch: f32,
    pub dragging: bool,
    /// Box proportions: width/height of the front and depth/height.
    pub aspect: f32,
    pub depth: f32,
    /// System accent colour for the spine (rgb 0..1).
    pub accent: [f32; 3],
    /// Glossy plastic (jewel/DVD case) vs matte cardboard.
    pub glossy: bool,
    /// Cover pixels waiting to be uploaded (w, h, rgba).
    pub pending_cover: Option<(u32, u32, Vec<u8>)>,
    pub t: f32,
}

pub type Shared = Rc<RefCell<ViewerState>>;

pub(crate) struct Tex(pub glow::Texture);

pub struct BoxRenderer {
    gl: Rc<glow::Context>,
    program: glow::Program,
    vbo: glow::Buffer,
    cover: Option<Tex>,
    spine: Tex,
    side: Tex,
    glow: glow::Program,
    glow_vbo: glow::Buffer,
}

pub(crate) const VS: &str = r#"
attribute vec3 a_pos; attribute vec3 a_nrm; attribute vec2 a_uv;
uniform mat4 u_mvp; uniform mat4 u_model;
varying vec3 v_nrm; varying vec2 v_uv; varying vec3 v_pos;
void main() {
  v_uv = a_uv;
  v_nrm = mat3(u_model) * a_nrm;
  v_pos = (u_model * vec4(a_pos, 1.0)).xyz;
  gl_Position = u_mvp * vec4(a_pos, 1.0);
}"#;

pub(crate) const FS: &str = r#"
#ifdef GL_ES
precision mediump float;
#endif
uniform sampler2D u_tex; uniform vec3 u_tint; uniform float u_gloss; uniform float u_dim;
varying vec3 v_nrm; varying vec2 v_uv; varying vec3 v_pos;
void main() {
  vec3 n = normalize(v_nrm);
  vec3 l = normalize(vec3(-0.45, 0.75, 0.9));
  vec3 v = normalize(vec3(0.0, 0.0, 3.2) - v_pos);
  float diff = max(dot(n, l), 0.0);
  // Soft fill from the opposite side + faint rim so dark plastics read in 3D.
  float fill = max(dot(n, normalize(vec3(0.7, -0.2, -0.6))), 0.0) * 0.22;
  float rim = pow(1.0 - max(dot(n, v), 0.0), 3.0) * 0.12;
  vec3 h = normalize(l + v);
  float spec = pow(max(dot(n, h), 0.0), mix(24.0, 90.0, u_gloss)) * mix(0.15, 0.55, u_gloss);
  vec4 c = texture2D(u_tex, v_uv) * vec4(u_tint, 1.0);
  vec3 col = c.rgb * (0.40 + 0.70 * diff + fill) * u_dim + vec3(spec + rim) * u_dim;
  gl_FragColor = vec4(col, 1.0);
}"#;

const GLOW_VS: &str = r#"
attribute vec2 a_pos; varying vec2 v_p;
void main() { v_p = a_pos; gl_Position = vec4(a_pos, 0.0, 1.0); }"#;

const GLOW_FS: &str = r#"
#ifdef GL_ES
precision mediump float;
#endif
varying vec2 v_p; uniform vec3 u_color; uniform float u_alpha;
void main() {
  float d = length(v_p * vec2(1.0, 1.35));
  float a = smoothstep(0.95, 0.0, d) * u_alpha;
  gl_FragColor = vec4(u_color * a, a);
}"#;

pub(crate) fn compile(gl: &glow::Context, vs: &str, fs: &str) -> Result<glow::Program, String> {
    unsafe {
        let program = gl.create_program()?;
        for (kind, src) in [(glow::VERTEX_SHADER, vs), (glow::FRAGMENT_SHADER, fs)] {
            let sh = gl.create_shader(kind)?;
            gl.shader_source(sh, src);
            gl.compile_shader(sh);
            if !gl.get_shader_compile_status(sh) {
                return Err(gl.get_shader_info_log(sh));
            }
            gl.attach_shader(program, sh);
        }
        gl.link_program(program);
        if !gl.get_program_link_status(program) {
            return Err(gl.get_program_info_log(program));
        }
        Ok(program)
    }
}

/// Cube with per-face normals/uvs; width `w`, height 1, depth `d` (centered).
pub(crate) fn cube(w: f32, d: f32) -> Vec<f32> {
    let (x, y, z) = (w / 2.0, 0.5, d / 2.0);
    // Each face: 2 triangles × (pos3, nrm3, uv2); uv flipped so textures are upright.
    let face = |p: [[f32; 3]; 4], n: [f32; 3]| -> Vec<f32> {
        let uv = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
        let idx = [0, 1, 2, 0, 2, 3];
        let mut v = Vec::new();
        for i in idx {
            v.extend_from_slice(&p[i]);
            v.extend_from_slice(&n);
            v.extend_from_slice(&uv[i]);
        }
        v
    };
    let mut v = Vec::new();
    v.extend(face([[-x, -y, z], [x, -y, z], [x, y, z], [-x, y, z]], [0.0, 0.0, 1.0])); // front
    v.extend(face([[x, -y, -z], [-x, -y, -z], [-x, y, -z], [x, y, -z]], [0.0, 0.0, -1.0])); // back
    v.extend(face([[-x, -y, -z], [-x, -y, z], [-x, y, z], [-x, y, -z]], [-1.0, 0.0, 0.0])); // left spine
    v.extend(face([[x, -y, z], [x, -y, -z], [x, y, -z], [x, y, z]], [1.0, 0.0, 0.0])); // right spine
    v.extend(face([[-x, y, z], [x, y, z], [x, y, -z], [-x, y, -z]], [0.0, 1.0, 0.0])); // top
    v.extend(face([[-x, -y, -z], [x, -y, -z], [x, -y, z], [-x, -y, z]], [0.0, -1.0, 0.0])); // bottom
    v
}

pub(crate) fn upload(gl: &glow::Context, w: u32, h: u32, rgba: &[u8]) -> Result<Tex, String> {
    unsafe {
        let t = gl.create_texture()?;
        gl.bind_texture(glow::TEXTURE_2D, Some(t));
        gl.tex_image_2d(glow::TEXTURE_2D, 0, glow::RGBA as i32, w as i32, h as i32, 0, glow::RGBA, glow::UNSIGNED_BYTE, glow::PixelUnpackData::Slice(Some(rgba)));
        gl.generate_mipmap(glow::TEXTURE_2D);
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR_MIPMAP_LINEAR as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
        Ok(Tex(t))
    }
}

/// Procedural spine: a soft vertical gradient (lighter top) with darker edges.
fn spine_pixels() -> (u32, u32, Vec<u8>) {
    let (w, h) = (16u32, 256u32);
    let mut px = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let edge = if x == 0 || x == w - 1 { 0.75 } else { 1.0 };
            let v = (0.78 + 0.22 * (1.0 - y as f32 / h as f32)) * edge;
            let c = (v * 255.0) as u8;
            px.extend_from_slice(&[c, c, c, 255]);
        }
    }
    (w, h, px)
}

impl BoxRenderer {
    pub fn new(gl: Rc<glow::Context>) -> Result<Self, String> {
        unsafe {
            let program = compile(&gl, VS, FS)?;
            let glow_prog = compile(&gl, GLOW_VS, GLOW_FS)?;
            let vbo = gl.create_buffer()?;
            let glow_vbo = gl.create_buffer()?;
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(glow_vbo));
            let quad: [f32; 12] = [-1.0, -1.0, 1.0, -1.0, 1.0, 1.0, -1.0, -1.0, 1.0, 1.0, -1.0, 1.0];
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytemuck_cast(&quad), glow::STATIC_DRAW);
            let (sw, sh, sp) = spine_pixels();
            let spine = upload(&gl, sw, sh, &sp)?;
            let side = upload(&gl, 1, 1, &[214, 216, 222, 255])?;
            Ok(Self { gl, program, vbo, cover: None, spine, side, glow: glow_prog, glow_vbo })
        }
    }

    /// Draws the box for the current state into `state.region` of a window of
    /// `win_w × win_h` physical pixels. Restores the GL state Slint relies on.
    pub fn render(&mut self, state: &mut ViewerState, win_w: f32, win_h: f32) {
        if let Some((w, h, px)) = state.pending_cover.take() {
            if let Ok(t) = upload(&self.gl, w, h, &px) {
                self.cover = Some(t);
            }
        }
        let gl = &self.gl;
        let (rx, ry, rw, rh) = state.region;
        if rw < 4.0 || rh < 4.0 {
            return;
        }
        unsafe {
            // GL viewport origin is bottom-left.
            gl.viewport(rx as i32, (win_h - ry - rh) as i32, rw as i32, rh as i32);
            gl.enable(glow::SCISSOR_TEST);
            gl.scissor(rx as i32, (win_h - ry - rh) as i32, rw as i32, rh as i32);

            // Floating glow behind the box.
            gl.use_program(Some(self.glow));
            gl.enable(glow::BLEND);
            gl.blend_func(glow::ONE, glow::ONE_MINUS_SRC_ALPHA);
            gl.disable(glow::DEPTH_TEST);
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.glow_vbo));
            let a = gl.get_attrib_location(self.glow, "a_pos").unwrap_or(0);
            gl.enable_vertex_attrib_array(a);
            gl.vertex_attrib_pointer_f32(a, 2, glow::FLOAT, false, 8, 0);
            gl.uniform_3_f32(gl.get_uniform_location(self.glow, "u_color").as_ref(), state.accent[0], state.accent[1], state.accent[2]);
            gl.uniform_1_f32(gl.get_uniform_location(self.glow, "u_alpha").as_ref(), 0.35);
            gl.draw_arrays(glow::TRIANGLES, 0, 6);
            gl.disable_vertex_attrib_array(a);

            // The box.
            gl.enable(glow::DEPTH_TEST);
            gl.depth_func(glow::LEQUAL);
            gl.clear(glow::DEPTH_BUFFER_BIT);
            gl.enable(glow::CULL_FACE);
            gl.cull_face(glow::BACK);
            gl.disable(glow::BLEND);
            gl.use_program(Some(self.program));
            let verts = cube(state.aspect.max(0.2), state.depth.max(0.02));
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(self.vbo));
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, bytemuck_cast(&verts), glow::DYNAMIC_DRAW);
            let stride = 8 * 4;
            for (name, size, off) in [("a_pos", 3, 0), ("a_nrm", 3, 12), ("a_uv", 2, 24)] {
                if let Some(loc) = gl.get_attrib_location(self.program, name) {
                    gl.enable_vertex_attrib_array(loc);
                    gl.vertex_attrib_pointer_f32(loc, size, glow::FLOAT, false, stride, off);
                }
            }
            // Camera: fit the box height in view; subtle floating bob.
            let aspect_vp = rw / rh;
            let proj = perspective(28f32.to_radians(), aspect_vp, 0.1, 20.0);
            let dist = 3.2 * state.aspect.max(1.0).sqrt();
            let view = translate(0.0, 0.0, -dist);
            let bob = (state.t * 1.3).sin() * 0.025;
            let model = mul(mul(translate(0.0, bob, 0.0), rot_x(state.pitch)), rot_y(state.yaw));
            let mvp = mul(mul(proj, view), model);
            gl.uniform_matrix_4_f32_slice(gl.get_uniform_location(self.program, "u_mvp").as_ref(), false, &mvp);
            gl.uniform_matrix_4_f32_slice(gl.get_uniform_location(self.program, "u_model").as_ref(), false, &model);
            gl.uniform_1_i32(gl.get_uniform_location(self.program, "u_tex").as_ref(), 0);
            gl.uniform_1_f32(gl.get_uniform_location(self.program, "u_gloss").as_ref(), if state.glossy { 1.0 } else { 0.0 });
            gl.active_texture(glow::TEXTURE0);
            let tint_loc = gl.get_uniform_location(self.program, "u_tint");
            let dim_loc = gl.get_uniform_location(self.program, "u_dim");
            let ac = state.accent;
            // face order: front, back, left, right, top, bottom
            let faces: [(Option<&Tex>, [f32; 3], f32); 6] = [
                (self.cover.as_ref(), [1.0, 1.0, 1.0], 1.0),
                (self.cover.as_ref(), [0.55, 0.55, 0.6], 0.8),
                (Some(&self.spine), ac, 1.0),
                (Some(&self.spine), ac, 1.0),
                (Some(&self.side), [1.0, 1.0, 1.0], 0.9),
                (Some(&self.side), [1.0, 1.0, 1.0], 0.7),
            ];
            for (i, (tex, tint, dim)) in faces.iter().enumerate() {
                match tex {
                    Some(t) => gl.bind_texture(glow::TEXTURE_2D, Some(t.0)),
                    None => gl.bind_texture(glow::TEXTURE_2D, Some(self.side.0)),
                }
                gl.uniform_3_f32(tint_loc.as_ref(), tint[0], tint[1], tint[2]);
                gl.uniform_1_f32(dim_loc.as_ref(), *dim);
                gl.draw_arrays(glow::TRIANGLES, (i * 6) as i32, 6);
            }
            for name in ["a_pos", "a_nrm", "a_uv"] {
                if let Some(loc) = gl.get_attrib_location(self.program, name) {
                    gl.disable_vertex_attrib_array(loc);
                }
            }
            // Back to a state femtovg expects.
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
}

pub(crate) fn bytemuck_cast(v: &[f32]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(v.as_ptr() as *const u8, v.len() * 4) }
}

// ---- tiny column-major 4x4 helpers ----
pub(crate) type M4 = [f32; 16];
pub(crate) fn mul(a: M4, b: M4) -> M4 {
    let mut r = [0.0; 16];
    for c in 0..4 {
        for rr in 0..4 {
            r[c * 4 + rr] = (0..4).map(|k| a[k * 4 + rr] * b[c * 4 + k]).sum();
        }
    }
    r
}
pub(crate) fn translate(x: f32, y: f32, z: f32) -> M4 {
    [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, x, y, z, 1.0]
}
pub(crate) fn rot_y(a: f32) -> M4 {
    let (s, c) = a.sin_cos();
    [c, 0.0, -s, 0.0, 0.0, 1.0, 0.0, 0.0, s, 0.0, c, 0.0, 0.0, 0.0, 0.0, 1.0]
}
pub(crate) fn rot_x(a: f32) -> M4 {
    let (s, c) = a.sin_cos();
    [1.0, 0.0, 0.0, 0.0, 0.0, c, s, 0.0, 0.0, -s, c, 0.0, 0.0, 0.0, 0.0, 1.0]
}
pub(crate) fn perspective(fovy: f32, aspect: f32, near: f32, far: f32) -> M4 {
    let f = 1.0 / (fovy / 2.0).tan();
    let nf = 1.0 / (near - far);
    [f / aspect, 0.0, 0.0, 0.0, 0.0, f, 0.0, 0.0, 0.0, 0.0, (far + near) * nf, -1.0, 0.0, 0.0, 2.0 * far * near * nf, 0.0]
}
