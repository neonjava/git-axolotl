//! Procedural pixel-art axolotl renderer.
//!
//! The body is a chain of variable-radius nodes along a spine. Gills, legs and toes are smaller
//! chains painted behind or in front of it. Growth follows real metamorphosis: a legless hatchling
//! gains front legs, then back legs, then a full gill crown and frilled tail fin.

use std::f64::consts::{PI, TAU};

use crate::coat::{AxolotlCoat, ShadeRamp};
use crate::history::AccountSnapshot;

/// Native canvas width in pixels (exported at 2x scale = 640px).
pub const FRAME_WIDTH_PIXELS: u32 = 320;
/// Native canvas height in pixels (exported at 2x scale = 280px).
pub const FRAME_HEIGHT_PIXELS: u32 = 140;
/// Animation frame rate (15 FPS).
pub const FRAMES_PER_SECOND: u32 = 15;
/// Total frames in one seamless swimming loop (48 frames = 3.2 seconds).
pub const LOOP_FRAME_COUNT: u32 = 48;

pub const OUTLINE_COLOR: [u8; 4] = [15, 18, 26, 255];
const EYE_COLOR: [u8; 4] = [20, 16, 26, 255];
const EYE_GLINT: [u8; 4] = [250, 250, 255, 255];

const BAYER_4X4: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
const LIGHT: [f64; 3] = [-0.30, -0.62, 0.72];
/// Bottom rows kept clear so the axolotl never overlaps the HUD text.
const HUD_RESERVED_BOTTOM: f64 = 22.0;

/// Camera perspective for rendering the axolotl.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CameraView {
    /// Side profile, head to the left.
    Side,
    /// Head-down dive, head to the right, with both gill sets visible.
    Dive,
    /// Top-down view with gills and legs spread on both sides.
    Top,
}

/// A rendered RGBA pixel buffer of `FRAME_WIDTH_PIXELS * FRAME_HEIGHT_PIXELS`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AxolotlFrame {
    pub rgba_pixels: Vec<[u8; 4]>,
}

impl AxolotlFrame {
    pub fn as_rgba_bytes(&self) -> &[u8] {
        self.rgba_pixels.as_flattened()
    }
}

/// Growth parameters derived from a contribution count.
#[derive(Clone, Copy, Debug)]
pub struct AxolotlBiology {
    pub commits: u64,
    pub growth: f64,
    pub maturity: f64,
    /// Front leg development, `0.0` (absent) to `1.0`. Complete by Stage 2.
    pub front_legs: f64,
    /// Back leg development, `0.0` (absent) to `1.0`. Complete by Stage 3.
    pub back_legs: f64,
    /// Stage 5 progress, which enlarges the gill crown and frills the fin.
    pub elder: f64,
    pub body_len: f64,
    pub head_radius: f64,
    pub gill_scale: f64,
    pub gill_fronds: usize,
    pub fin_scale: f64,
    pub spot_threshold: f64,
}

impl AxolotlBiology {
    /// Compute life-stage proportions on a log-like curve (`0` through `2,500+` contributions):
    /// - `0..150`: Stage 1: Hatchling
    /// - `150..500`: Stage 2: Larva
    /// - `500..1,200`: Stage 3: Juvenile
    /// - `1,200..2,500`: Stage 4: Adult
    /// - `2,500+`: Stage 5: Elder
    pub fn from_commits(commits: u64) -> Self {
        let g = ((commits as f64) / 2800.0).powf(0.45).clamp(0.0, 1.0);
        let maturity = smooth_step((g - 0.14) / 0.82);
        let front_legs = smooth_step((g - 0.20) / 0.07);
        let back_legs = smooth_step((g - 0.39) / 0.07);
        let elder = smooth_step((g - 0.80) / 0.15);

        let body_len = lerp(72.0, 190.0, g);
        let head_radius = body_len * lerp(0.158, 0.108, maturity);

        Self {
            commits,
            growth: g,
            maturity,
            front_legs,
            back_legs,
            elder,
            body_len,
            head_radius,
            gill_scale: lerp(0.62, 1.05, maturity) + 0.42 * elder,
            gill_fronds: lerp(2.0, 6.0, maturity).round() as usize + (elder * 2.0).round() as usize,
            fin_scale: lerp(1.08, 0.86, maturity) + 0.26 * elder,
            spot_threshold: lerp(0.82, 0.67, maturity),
        }
    }

    pub fn stage_name(&self) -> &'static str {
        match stage_index(self.commits) {
            4 => "Elder Axolotl (Full Gill Crown)",
            3 => "Adult Axolotl",
            2 => "Juvenile Axolotl (Four Legs)",
            1 => "Larva (Front Legs)",
            _ => "Hatchling",
        }
    }

    pub fn hud_stage_label(&self) -> &'static str {
        match stage_index(self.commits) {
            4 => "STAGE 5: ELDER",
            3 => "STAGE 4: ADULT",
            2 => "STAGE 3: JUVENILE",
            1 => "STAGE 2: LARVA",
            _ => "STAGE 1: HATCHLING",
        }
    }
}

fn stage_index(commits: u64) -> usize {
    match commits {
        2500.. => 4,
        1200.. => 3,
        500.. => 2,
        150.. => 1,
        _ => 0,
    }
}

#[derive(Clone, Copy)]
struct Node {
    x: f64,
    y: f64,
    /// Outer radii, which include the fin on the body.
    r_top: f64,
    r_bot: f64,
    /// Flesh radii. Anything between these and the outer radii is fin.
    core_top: f64,
    core_bot: f64,
    u: f64,
}

impl Node {
    fn round(x: f64, y: f64, r: f64, u: f64) -> Self {
        Self {
            x,
            y,
            r_top: r,
            r_bot: r,
            core_top: r,
            core_bot: r,
            u,
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum PartKind {
    GillStalk,
    GillFrond,
    Leg,
}

struct Part {
    z: i32,
    kind: PartKind,
    pts: Vec<Node>,
}

struct Hit {
    u: f64,
    dist: f64,
    outer_r: f64,
    core_r: f64,
    dx: f64,
    dy: f64,
}

/// Maps pose coordinates onto the canvas. Fixed per loop so the camera never jitters.
#[derive(Clone, Copy)]
struct Framing {
    scale: f64,
    center_x: f64,
    center_y: f64,
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

fn smooth_step(t: f64) -> f64 {
    let c = t.clamp(0.0, 1.0);
    c * c * (3.0 - 2.0 * c)
}

fn dither_threshold(px: u32, py: u32) -> f64 {
    (f64::from(BAYER_4X4[(py & 3) as usize][(px & 3) as usize]) + 0.5) / 16.0
}

fn choose_shade(ramp: &ShadeRamp, brightness: f64, px: u32, py: u32) -> [u8; 4] {
    let pos = brightness.clamp(0.0, 1.0) * (ramp.len() - 1) as f64;
    let base_idx = pos.floor() as usize;
    let shade_idx = base_idx + usize::from(pos.fract() > dither_threshold(px, py));
    ramp[shade_idx.min(ramp.len() - 1)]
}

fn hash_2d(ix: i64, iy: i64, seed: u64) -> f64 {
    let mut n = (ix as u64)
        .wrapping_mul(374_761_393)
        .wrapping_add((iy as u64).wrapping_mul(668_265_263))
        .wrapping_add(seed.wrapping_mul(1_442_695_041));
    n = (n ^ (n >> 13)).wrapping_mul(1_274_126_177);
    ((n ^ (n >> 16)) & 0xFFFF_FFFF) as f64 / 4_294_967_295.0
}

fn smooth_noise(x: f64, y: f64, seed: u64) -> f64 {
    let ix = x.floor() as i64;
    let iy = y.floor() as i64;
    let fx = x - x.floor();
    let fy = y - y.floor();
    let ux = fx * fx * (3.0 - 2.0 * fx);
    let uy = fy * fy * (3.0 - 2.0 * fy);
    let a = hash_2d(ix, iy, seed);
    let b = hash_2d(ix + 1, iy, seed);
    let c = hash_2d(ix, iy + 1, seed);
    let d = hash_2d(ix + 1, iy + 1, seed);
    a + (b - a) * ux + (c - a) * uy + (a - b - c + d) * ux * uy
}

/// Find the chain segment that covers a point, preferring the one the point sits deepest inside.
fn sample_chain(pts: &[Node], sx: f64, sy: f64) -> Option<Hit> {
    let mut best_norm = f64::INFINITY;
    let mut best = None;
    for seg in pts.windows(2) {
        let (a, b) = (seg[0], seg[1]);
        let vx = b.x - a.x;
        let vy = b.y - a.y;
        let l_sq = vx * vx + vy * vy;
        if l_sq < 1e-6 {
            continue;
        }
        let t = (((sx - a.x) * vx + (sy - a.y) * vy) / l_sq).clamp(0.0, 1.0);
        let dx = sx - (a.x + t * vx);
        let dy = sy - (a.y + t * vy);
        let dist = dx.hypot(dy);
        let (outer_r, core_r) = if dy < 0.0 {
            (
                a.r_top + t * (b.r_top - a.r_top),
                a.core_top + t * (b.core_top - a.core_top),
            )
        } else {
            (
                a.r_bot + t * (b.r_bot - a.r_bot),
                a.core_bot + t * (b.core_bot - a.core_bot),
            )
        };
        let norm = dist / outer_r;
        if dist <= outer_r && norm < best_norm {
            best_norm = norm;
            best = Some(Hit {
                u: a.u + t * (b.u - a.u),
                dist,
                outer_r,
                core_r,
                dx,
                dy,
            });
        }
    }
    best
}

fn lit_brightness(nx: f64, ny: f64) -> f64 {
    let nz = (1.0 - (nx * nx + ny * ny)).max(0.0).sqrt();
    (nx * LIGHT[0] + ny * LIGHT[1] + nz * LIGHT[2]).max(0.0)
}

/// Pixel rectangle covering a chain, clipped to the canvas interior.
fn chain_bounds(pts: &[Node], w: u32, h: u32) -> (u32, u32, u32, u32) {
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for p in pts {
        let r = p.r_top.max(p.r_bot) + 1.0;
        min_x = min_x.min(p.x - r);
        max_x = max_x.max(p.x + r);
        min_y = min_y.min(p.y - r);
        max_y = max_y.max(p.y + r);
    }
    (
        min_x.floor().max(1.0) as u32,
        min_y.floor().max(1.0) as u32,
        (max_x.ceil().max(1.0) as u32).min(w - 2),
        (max_y.ceil().max(1.0) as u32).min(h - 2),
    )
}

fn compute_framing(bio: &AxolotlBiology, view: CameraView) -> Framing {
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for step in 0..8 {
        let (spine, parts) = build_pose(bio, view, TAU * f64::from(step) / 8.0);
        for p in spine.iter().chain(parts.iter().flat_map(|part| &part.pts)) {
            let r = p.r_top.max(p.r_bot);
            min_x = min_x.min(p.x - r);
            max_x = max_x.max(p.x + r);
            min_y = min_y.min(p.y - p.r_top);
            max_y = max_y.max(p.y + p.r_bot);
        }
    }

    let avail_w = f64::from(FRAME_WIDTH_PIXELS) - 24.0;
    let avail_h = f64::from(FRAME_HEIGHT_PIXELS) - HUD_RESERVED_BOTTOM - 12.0;
    let scale = (avail_w / (max_x - min_x).max(1.0))
        .min(avail_h / (max_y - min_y).max(1.0))
        .min(1.0);
    Framing {
        scale,
        center_x: (min_x + max_x) * 0.5,
        center_y: (min_y + max_y) * 0.5,
    }
}

/// Render a single still frame of the axolotl.
pub fn render_still_frame(
    snapshot: &AccountSnapshot,
    coat: &AxolotlCoat,
    view: CameraView,
) -> AxolotlFrame {
    render_swim_frame(snapshot, coat, view, 12)
}

/// Render frame `frame_index` of the swimming loop.
pub fn render_swim_frame(
    snapshot: &AccountSnapshot,
    coat: &AxolotlCoat,
    view: CameraView,
    frame_index: u32,
) -> AxolotlFrame {
    let bio = AxolotlBiology::from_commits(snapshot.total_contributions);
    let framing = compute_framing(&bio, view);
    render_framed(snapshot, coat, view, &bio, &framing, frame_index)
}

/// Render the whole seamless swimming loop.
pub fn render_swim_loop(
    snapshot: &AccountSnapshot,
    coat: &AxolotlCoat,
    view: CameraView,
) -> Vec<AxolotlFrame> {
    let bio = AxolotlBiology::from_commits(snapshot.total_contributions);
    let framing = compute_framing(&bio, view);
    (0..LOOP_FRAME_COUNT)
        .map(|f| render_framed(snapshot, coat, view, &bio, &framing, f))
        .collect()
}

fn render_framed(
    snapshot: &AccountSnapshot,
    coat: &AxolotlCoat,
    view: CameraView,
    bio: &AxolotlBiology,
    framing: &Framing,
    frame_index: u32,
) -> AxolotlFrame {
    let w = FRAME_WIDTH_PIXELS;
    let h = FRAME_HEIGHT_PIXELS;
    let mut pixels = vec![[0_u8; 4]; (w * h) as usize];

    let turn = TAU * f64::from(frame_index % LOOP_FRAME_COUNT) / f64::from(LOOP_FRAME_COUNT);
    let target_x = f64::from(w) * 0.5 + turn.sin() * 4.0;
    let target_y = (f64::from(h) - HUD_RESERVED_BOTTOM) * 0.5 + turn.cos() * 2.0;

    let (mut spine, mut parts) = build_pose(bio, view, turn);
    let place = |p: &mut Node| {
        p.x = target_x + (p.x - framing.center_x) * framing.scale;
        p.y = target_y + (p.y - framing.center_y) * framing.scale;
        p.r_top *= framing.scale;
        p.r_bot *= framing.scale;
        p.core_top *= framing.scale;
        p.core_bot *= framing.scale;
    };
    spine.iter_mut().for_each(place);
    parts.iter_mut().flat_map(|p| &mut p.pts).for_each(place);
    parts.sort_by_key(|p| p.z);

    for part in parts.iter().filter(|p| p.z < 0) {
        paint_part(&mut pixels, part, coat, true);
    }
    paint_body(
        &mut pixels,
        &spine,
        coat,
        bio,
        view,
        snapshot.identity_seed(),
    );
    paint_face(&mut pixels, &spine, coat, bio, view, framing.scale);
    for part in parts.iter().filter(|p| p.z >= 0) {
        paint_part(&mut pixels, part, coat, false);
    }

    paint_outline(&mut pixels);
    paint_metrics_hud(&mut pixels, w, h, bio, coat, snapshot);
    AxolotlFrame {
        rgba_pixels: pixels,
    }
}

fn paint_part(pixels: &mut [[u8; 4]], part: &Part, coat: &AxolotlCoat, far_side: bool) {
    let w = FRAME_WIDTH_PIXELS;
    let (min_x, min_y, max_x, max_y) = chain_bounds(&part.pts, w, FRAME_HEIGHT_PIXELS);
    for py in min_y..=max_y {
        for px in min_x..=max_x {
            let Some(hit) = sample_chain(&part.pts, f64::from(px) + 0.5, f64::from(py) + 0.5)
            else {
                continue;
            };
            let ramp = match part.kind {
                PartKind::GillStalk => &coat.gill,
                PartKind::GillFrond => {
                    if hit.u > 0.45 {
                        &coat.gill_tip
                    } else {
                        &coat.gill
                    }
                }
                PartKind::Leg => &coat.skin,
            };
            let r = hit.core_r.max(0.5);
            let mut bright = 0.34 + 0.64 * lit_brightness(hit.dx / r, hit.dy / r);
            if far_side {
                bright -= 0.26;
            }
            pixels[(py * w + px) as usize] = choose_shade(ramp, bright, px, py);
        }
    }
}

fn paint_body(
    pixels: &mut [[u8; 4]],
    spine: &[Node],
    coat: &AxolotlCoat,
    bio: &AxolotlBiology,
    view: CameraView,
    seed: u64,
) {
    let w = FRAME_WIDTH_PIXELS;
    let (min_x, min_y, max_x, max_y) = chain_bounds(spine, w, FRAME_HEIGHT_PIXELS);
    for py in min_y..=max_y {
        for px in min_x..=max_x {
            let Some(hit) = sample_chain(spine, f64::from(px) + 0.5, f64::from(py) + 0.5) else {
                continue;
            };
            let idx = (py * w + px) as usize;

            if hit.dist > hit.core_r {
                let ray = (hit.u * 120.0).sin() > 0.55;
                let bright = if hit.dist > hit.outer_r - 0.9 {
                    0.34
                } else if ray {
                    0.52
                } else {
                    0.76
                };
                pixels[idx] = choose_shade(&coat.fin, bright, px, py);
                continue;
            }

            let v = hit.dy / hit.core_r;
            let mut ramp = &coat.skin;
            let belly_line = if hit.u < 0.22 { 0.66 } else { 0.40 };
            if view != CameraView::Top && v > belly_line {
                ramp = &coat.belly;
            }
            if hit.u > 0.03 && (view == CameraView::Top || v < 0.42) {
                let spot =
                    smooth_noise(hit.u * bio.body_len / 4.2, v * bio.head_radius / 3.4, seed);
                if spot > bio.spot_threshold {
                    ramp = &coat.markings;
                }
            }

            let mut bright = 0.36 + 0.64 * lit_brightness(hit.dx / hit.core_r, hit.dy / hit.core_r);
            let on_flank = hit.u > 0.30 && hit.u < 0.60 && v > -0.35 && v < 0.55;
            if view != CameraView::Top
                && on_flank
                && bio.maturity > 0.25
                && (hit.u * bio.body_len * 0.62).sin() > 0.82
            {
                bright -= 0.20;
            }
            pixels[idx] = choose_shade(ramp, bright, px, py);
        }
    }
}

fn node_at(spine: &[Node], u: f64) -> Node {
    let idx = (u * (spine.len() - 1) as f64).round() as usize;
    spine[idx.min(spine.len() - 1)]
}

fn paint_disc(pixels: &mut [[u8; 4]], cx: f64, cy: f64, r: f64, color: [u8; 4]) {
    let w = FRAME_WIDTH_PIXELS as i32;
    let h = FRAME_HEIGHT_PIXELS as i32;
    let reach = r.ceil() as i32 + 1;
    let (ix, iy) = (cx.floor() as i32, cy.floor() as i32);
    for y in iy - reach..=iy + reach {
        for x in ix - reach..=ix + reach {
            let inside = (f64::from(x) + 0.5 - cx).hypot(f64::from(y) + 0.5 - cy) <= r;
            if inside && x > 0 && y > 0 && x < w - 1 && y < h - 1 {
                pixels[(y * w + x) as usize] = color;
            }
        }
    }
}

fn paint_eye(pixels: &mut [[u8; 4]], x: f64, y: f64, r: f64) {
    paint_disc(pixels, x, y, r, EYE_COLOR);
    paint_disc(
        pixels,
        x - r * 0.35,
        y - r * 0.40,
        (r * 0.34).max(0.55),
        EYE_GLINT,
    );
}

fn paint_face(
    pixels: &mut [[u8; 4]],
    spine: &[Node],
    coat: &AxolotlCoat,
    bio: &AxolotlBiology,
    view: CameraView,
    scale: f64,
) {
    let eye_r = (bio.head_radius * scale * lerp(0.19, 0.145, bio.maturity)).clamp(1.5, 3.4);

    if view == CameraView::Top {
        let node = node_at(spine, 0.085);
        paint_eye(pixels, node.x, node.y - node.core_top * 0.66, eye_r);
        paint_eye(pixels, node.x, node.y + node.core_bot * 0.66, eye_r);
        return;
    }

    let eye = node_at(spine, 0.082);
    paint_eye(pixels, eye.x, eye.y - eye.core_top * 0.20, eye_r);

    // The wide upturned smile that gives the animal its face.
    let steps = 40;
    let w = FRAME_WIDTH_PIXELS as i32;
    for i in 0..=steps {
        let t = f64::from(i) / f64::from(steps);
        let node = node_at(spine, lerp(0.004, 0.125, t));
        let drop = 0.40 + 0.10 * (t * PI).sin() - 0.36 * t.powi(3);
        let x = node.x.floor() as i32;
        let y = (node.y + node.core_bot * drop).floor() as i32;
        let idx = (y * w + x) as usize;
        if pixels.get(idx).is_some_and(|p| p[3] != 0) {
            pixels[idx] = coat.markings[0];
        }
    }
}

/// Add a 1px dark edge around everything drawn so far.
fn paint_outline(pixels: &mut [[u8; 4]]) {
    let stride = FRAME_WIDTH_PIXELS as usize;
    let orig = pixels.to_vec();
    for py in 1..FRAME_HEIGHT_PIXELS as usize - 1 {
        for px in 1..stride - 1 {
            let idx = py * stride + px;
            if orig[idx][3] == 0
                && (orig[idx - 1][3] != 0
                    || orig[idx + 1][3] != 0
                    || orig[idx - stride][3] != 0
                    || orig[idx + stride][3] != 0)
            {
                pixels[idx] = OUTLINE_COLOR;
            }
        }
    }
}

/// Flesh radii at `u` along the body, as `(top, bottom)` multiples of the head radius.
fn body_profile(u: f64, view: CameraView) -> (f64, f64) {
    let (head_top, head_bot) = if view == CameraView::Top {
        (1.26, 1.26)
    } else {
        (1.00, 0.92)
    };
    if u < 0.20 {
        let h = u / 0.20;
        let round = (1.0 - (1.0 - h) * (1.0 - h)).sqrt();
        let shape = 0.52 + 0.48 * round;
        (head_top * shape, head_bot * shape)
    } else if u < 0.28 {
        let t = smooth_step((u - 0.20) / 0.08);
        (lerp(head_top, 0.62, t), lerp(head_bot, 0.64, t))
    } else if u < 0.58 {
        let bulge = 0.12 * ((u - 0.28) / 0.30 * PI).sin();
        (0.62 + bulge * 0.5, 0.64 + bulge)
    } else {
        let t = (u - 0.58) / 0.42;
        let taper = (1.0 - t).powf(1.15);
        (0.62 * taper, 0.64 * taper)
    }
}

/// Dorsal and ventral fin heights at `u`, as multiples of the head radius.
fn fin_profile(u: f64, view: CameraView, bio: &AxolotlBiology, turn: f64) -> (f64, f64) {
    if view == CameraView::Top {
        return (0.16, 0.16);
    }
    let frill = 1.0 + bio.elder * 0.16 * (u * 46.0 - turn * 2.0).sin();
    let dorsal = if u > 0.42 {
        0.44 * (PI * ((u - 0.42) / 0.58).powf(0.75)).sin()
    } else {
        0.0
    };
    let ventral = if u > 0.66 {
        0.36 * (PI * ((u - 0.66) / 0.34).powf(0.80)).sin()
    } else {
        0.0
    };
    (
        (dorsal * bio.fin_scale * frill).max(0.0),
        (ventral * bio.fin_scale * frill).max(0.0),
    )
}

/// Build the spine and appendages at phase `turn` (`0..TAU`), centred on the origin.
fn build_pose(bio: &AxolotlBiology, view: CameraView, turn: f64) -> (Vec<Node>, Vec<Part>) {
    let len = bio.body_len;
    let head_r = bio.head_radius;
    let n_pts = (len.round() as usize).max(60);
    // -1.0 puts the head on the left, 1.0 on the right.
    let forward = if view == CameraView::Dive { 1.0 } else { -1.0 };
    let amp = head_r * if view == CameraView::Top { 0.95 } else { 0.44 };
    let pitch = (turn.cos() * 0.04).sin();

    let mut spine = Vec::with_capacity(n_pts);
    for i in 0..n_pts {
        let u = i as f64 / (n_pts - 1) as f64;
        let x = forward * (0.5 - u) * len;
        let head_bob = turn.sin() * head_r * 0.10 * (1.0 - u).powf(1.3);
        let wave = (u * TAU * 1.15 - turn).sin() * amp * u.powf(1.3);
        let mut y = head_bob + wave + (u - 0.35) * len * pitch;
        if view == CameraView::Dive {
            y += (0.45 - u) * len * 0.26 - (u * PI).sin() * head_r * 0.5;
        }

        let (core_top, core_bot) = body_profile(u, view);
        let (fin_top, fin_bot) = fin_profile(u, view, bio, turn);
        let core_top = (core_top * head_r).max(0.6);
        let core_bot = (core_bot * head_r).max(0.6);
        spine.push(Node {
            x,
            y,
            r_top: (core_top + fin_top * head_r).max(1.2),
            r_bot: (core_bot + fin_bot * head_r).max(1.2),
            core_top,
            core_bot,
            u,
        });
    }

    let mut parts = Vec::new();
    add_gills(&mut parts, &spine, bio, view, forward, turn);
    add_legs(&mut parts, &spine, bio, view, forward, turn);
    (spine, parts)
}

/// Add one gill stalk with fronds on both sides, curling backwards as it extends.
#[allow(clippy::too_many_arguments)]
fn add_gill_stalk(
    parts: &mut Vec<Part>,
    bio: &AxolotlBiology,
    root: (f64, f64),
    angle: f64,
    length: f64,
    back: f64,
    lateral: f64,
    sway_phase: f64,
    z: i32,
) {
    const SEGMENTS: usize = 6;
    let head_r = bio.head_radius;
    let mut stalk = vec![Node::round(root.0, root.1, (head_r * 0.20).max(1.1), 0.0)];
    let (mut x, mut y) = root;
    let mut headings = Vec::with_capacity(SEGMENTS);
    for s in 1..=SEGMENTS {
        let t = s as f64 / SEGMENTS as f64;
        let heading = angle - t * 0.50 + (sway_phase - t * 2.2).sin() * 0.20 * t;
        x += back * heading.cos() * length / SEGMENTS as f64;
        y += lateral * heading.sin() * length / SEGMENTS as f64;
        stalk.push(Node::round(x, y, lerp(head_r * 0.18, 0.9, t).max(0.9), t));
        headings.push(heading);
    }

    let frond_len = length * 0.30;
    for j in 0..bio.gill_fronds {
        let t = (j as f64 + 1.0) / (bio.gill_fronds as f64 + 0.4);
        let seg = ((t * SEGMENTS as f64) as usize).min(SEGMENTS - 1);
        let base = stalk[seg + 1];
        let heading = headings[seg];
        let reach = frond_len * (1.0 - 0.45 * t);
        for side in [-1.0, 1.0] {
            let spread = heading + side * 0.95 + (sway_phase - t * 3.0).sin() * 0.12;
            parts.push(Part {
                z,
                kind: PartKind::GillFrond,
                pts: vec![
                    Node::round(base.x, base.y, 0.85, 0.0),
                    Node::round(
                        base.x + back * spread.cos() * reach,
                        base.y + lateral * spread.sin() * reach,
                        0.75,
                        1.0,
                    ),
                ],
            });
        }
    }

    parts.push(Part {
        z,
        kind: PartKind::GillStalk,
        pts: stalk,
    });
}

fn add_gills(
    parts: &mut Vec<Part>,
    spine: &[Node],
    bio: &AxolotlBiology,
    view: CameraView,
    forward: f64,
    turn: f64,
) {
    const LENGTHS: [f64; 3] = [1.30, 1.42, 1.18];
    let back = -forward;
    let root = node_at(spine, 0.185);
    let head_r = bio.head_radius;

    for (i, factor) in LENGTHS.iter().enumerate() {
        let fi = i as f64;
        let length = head_r * bio.gill_scale * factor;
        let phase = turn - fi * 0.8;
        if view == CameraView::Top {
            for side in [-1.0, 1.0] {
                let offset = root.core_top * (0.92 - fi * 0.20);
                add_gill_stalk(
                    parts,
                    bio,
                    (root.x + back * fi * head_r * 0.10, root.y + side * offset),
                    1.30 - fi * 0.42,
                    length,
                    back,
                    side,
                    phase,
                    2,
                );
            }
        } else {
            let height = root.core_top * (0.86 - fi * 0.34);
            // Far gills fan out slightly lower so they show between the near ones.
            add_gill_stalk(
                parts,
                bio,
                (root.x + forward * head_r * 0.12, root.y - height * 0.9),
                1.08 - fi * 0.50,
                length * 0.94,
                back,
                -1.0,
                phase + 0.5,
                -1,
            );
            add_gill_stalk(
                parts,
                bio,
                (root.x + back * fi * head_r * 0.06, root.y - height),
                1.34 - fi * 0.50,
                length,
                back,
                -1.0,
                phase,
                2,
            );
        }
    }
}

/// Add one three-joint leg ending in splayed toes.
#[allow(clippy::too_many_arguments)]
fn add_leg(
    parts: &mut Vec<Part>,
    bio: &AxolotlBiology,
    hip: (f64, f64),
    length: f64,
    back: f64,
    lateral: f64,
    phase: f64,
    z: i32,
) {
    let head_r = bio.head_radius;
    let paddle = phase.sin() * length * 0.22;
    let knee = (
        hip.0 + back * (length * 0.20 + paddle * 0.5),
        hip.1 + lateral * length * 0.55,
    );
    let foot = (
        knee.0 + back * (length * 0.06 + paddle),
        knee.1 + lateral * length * 0.45,
    );
    parts.push(Part {
        z,
        kind: PartKind::Leg,
        pts: vec![
            Node::round(hip.0, hip.1, (head_r * 0.30).max(1.2), 0.0),
            Node::round(knee.0, knee.1, (head_r * 0.24).max(1.1), 0.5),
            Node::round(foot.0, foot.1, (head_r * 0.20).max(1.0), 1.0),
        ],
    });

    let toe_len = (length * 0.36).max(1.5);
    let heading = (foot.1 - knee.1).atan2(foot.0 - knee.0);
    for toe in [-0.72, 0.0, 0.72] {
        let angle = heading + toe;
        parts.push(Part {
            z,
            kind: PartKind::Leg,
            pts: vec![
                Node::round(foot.0, foot.1, 0.95, 0.0),
                Node::round(
                    foot.0 + angle.cos() * toe_len,
                    foot.1 + angle.sin() * toe_len,
                    0.8,
                    1.0,
                ),
            ],
        });
    }
}

fn add_legs(
    parts: &mut Vec<Part>,
    spine: &[Node],
    bio: &AxolotlBiology,
    view: CameraView,
    forward: f64,
    turn: f64,
) {
    let back = -forward;
    let head_r = bio.head_radius;
    let pairs = [
        (0.300, bio.front_legs, 0.0),
        (0.585, bio.back_legs, PI * 0.5),
    ];

    for (u, development, offset) in pairs {
        if development < 0.05 {
            continue;
        }
        let length = head_r * 1.08 * development;
        let node = node_at(spine, u);
        if view == CameraView::Top {
            for side in [-1.0, 1.0] {
                let hip = (node.x, node.y + side * node.core_bot * 0.80);
                let phase = turn + offset + (side + 1.0) * 0.5 * PI;
                add_leg(parts, bio, hip, length, back, side, phase, -1);
            }
        } else {
            let far = (
                node.x + forward * head_r * 0.42,
                node.y + node.core_bot * 0.45,
            );
            add_leg(
                parts,
                bio,
                far,
                length * 0.92,
                back,
                1.0,
                turn + offset + PI,
                -2,
            );
            let near = (node.x, node.y + node.core_bot * 0.55);
            add_leg(parts, bio, near, length, back, 1.0, turn + offset, 3);
        }
    }
}

/// Paint the bottom-left pixel-art HUD metrics:
/// Line 1: `<STAGE>` (e.g., `STAGE 4: ADULT`, with a language-colored pip)
/// Line 2: `CONTRIBUTIONS: <count>`, then `ALL TIME: <count> SINCE <year>` when history is known
/// Line 3: Top 3 languages with corresponding pixel-color indicator pips
fn paint_metrics_hud(
    rgba_pixels: &mut [[u8; 4]],
    w: u32,
    h: u32,
    bio: &AxolotlBiology,
    coat: &AxolotlCoat,
    snapshot: &AccountSnapshot,
) {
    let base_x = 8_u32;
    let line1_y = h - 23;
    let line2_y = h - 15;
    let line3_y = h - 7;

    let pip_color = coat.gill[3];
    for dy in 0..5_u32 {
        for dx in 0..3_u32 {
            set_outlined_pixel(rgba_pixels, w, h, base_x + dx, line1_y + dy, pip_color);
        }
    }

    let line1 = bio.hud_stage_label();
    draw_pixel_string(rgba_pixels, w, h, base_x + 6, line1_y, line1, coat.belly[3]);

    let line2 = format!("CONTRIBUTIONS: {}", bio.commits);
    draw_pixel_string(rgba_pixels, w, h, base_x, line2_y, &line2, coat.skin[3]);

    let mut history = String::new();
    if snapshot.lifetime_contributions > bio.commits {
        history = format!("ALL TIME: {}", snapshot.lifetime_contributions);
    }
    if let Some(year) = snapshot.first_active_year() {
        history = format!("{history} SINCE {year}").trim_start().to_owned();
    }
    let history_x = base_x + (line2.chars().count() as u32) * 4 + 6;
    draw_pixel_string(
        rgba_pixels,
        w,
        h,
        history_x,
        line2_y,
        &history,
        coat.belly[2],
    );

    // Line 3: Top 3 languages with their respective pixel color dots
    let mut lang_x = base_x;
    for lang in snapshot.languages.iter().take(3) {
        if let Some(rgb) = crate::coat::parse_hex_color(&lang.hex_color) {
            let col = [rgb[0], rgb[1], rgb[2], 255];
            for dy in 0..3_u32 {
                for dx in 0..3_u32 {
                    set_outlined_pixel(rgba_pixels, w, h, lang_x + dx, line3_y + 1 + dy, col);
                }
            }
        }
        lang_x += 5;
        let short_name = lang.name.chars().take(8).collect::<String>();
        draw_pixel_string(
            rgba_pixels,
            w,
            h,
            lang_x,
            line3_y,
            &short_name,
            coat.belly[2],
        );
        lang_x += (short_name.chars().count() as u32) * 4 + 6;
    }
}

/// Native dimensions for the 1280x640 (4x scaled) shareable evolution & social preview card.
pub const SHARE_CARD_WIDTH_PIXELS: u32 = 320;
pub const SHARE_CARD_HEIGHT_PIXELS: u32 = 160;

/// Render a 320x160 native frame (exported at 4x = 1280x640) for social sharing ("Your Axolotl This Year" card & repo social preview).
pub fn render_share_card_frame(
    snapshot: &AccountSnapshot,
    coat: &AxolotlCoat,
    view: CameraView,
) -> Vec<[u8; 4]> {
    let w = SHARE_CARD_WIDTH_PIXELS;
    let h = SHARE_CARD_HEIGHT_PIXELS;
    let bg = [13_u8, 17, 23, 255];
    let panel_bg = [19_u8, 25, 34, 255];
    let border_col = coat.skin[1];
    let mut pixels = vec![bg; (w * h) as usize];

    // Outer 1px border frame at 3px inset
    for x in 3..(w - 3) {
        pixels[(3 * w + x) as usize] = border_col;
        pixels[((h - 4) * w + x) as usize] = border_col;
    }
    for y in 3..(h - 3) {
        pixels[(y * w + 3) as usize] = border_col;
        pixels[(y * w + (w - 4)) as usize] = border_col;
    }

    // Top header banner bar (y = 4..18)
    for y in 4..18 {
        for x in 4..(w - 4) {
            pixels[(y * w + x) as usize] = panel_bg;
        }
    }
    for x in 4..(w - 4) {
        pixels[(18 * w + x) as usize] = border_col;
    }

    let bio = AxolotlBiology::from_commits(snapshot.total_contributions);

    draw_pixel_string(&mut pixels, w, h, 9, 9, "GIT-AXOLOTL", coat.belly[3]);
    let stage_short = bio.hud_stage_label();
    let right_x = w.saturating_sub((stage_short.len() as u32) * 4 + 8);
    draw_pixel_string(&mut pixels, w, h, right_x, 9, stage_short, coat.skin[3]);

    // Composite the still axolotl frame into the center (y_offset = 14)
    let still = render_still_frame(snapshot, coat, view);
    for sy in 0..FRAME_HEIGHT_PIXELS {
        let dy = sy + 14;
        if dy >= h - 4 {
            continue;
        }
        for sx in 4..(FRAME_WIDTH_PIXELS - 4) {
            let src_px = still.rgba_pixels[(sy * FRAME_WIDTH_PIXELS + sx) as usize];
            if src_px[3] != 0 {
                pixels[(dy * w + sx) as usize] = src_px;
            }
        }
    }

    // Bottom-right 5-stage evolution progress pills
    let stages = ["1.HTCH", "2.LRVA", "3.JUVN", "4.ADLT", "5.ELDR"];
    let active_idx = stage_index(bio.commits);
    let mut sx = 148_u32;
    let sy = h - 13;
    for (idx, label) in stages.iter().enumerate() {
        let col = if idx == active_idx {
            coat.belly[3]
        } else if idx < active_idx {
            coat.skin[2]
        } else {
            coat.markings[1]
        };
        draw_pixel_string(&mut pixels, w, h, sx, sy, label, col);
        sx += (label.len() as u32) * 4 + 6;
    }

    pixels
}

fn draw_pixel_string(
    rgba_pixels: &mut [[u8; 4]],
    w: u32,
    h: u32,
    start_x: u32,
    start_y: u32,
    text: &str,
    color: [u8; 4],
) {
    let mut cursor_x = start_x;
    for ch in text.chars() {
        let glyph = glyph_3x5(ch);
        for (row_idx, &row_bits) in glyph.iter().enumerate() {
            for col_idx in 0..3_u32 {
                if (row_bits >> (2 - col_idx)) & 1 == 1 {
                    set_outlined_pixel(
                        rgba_pixels,
                        w,
                        h,
                        cursor_x + col_idx,
                        start_y + row_idx as u32,
                        color,
                    );
                }
            }
        }
        cursor_x += 4;
    }
}

fn set_outlined_pixel(
    rgba_pixels: &mut [[u8; 4]],
    w: u32,
    h: u32,
    px: u32,
    py: u32,
    color: [u8; 4],
) {
    if px == 0 || px >= w - 1 || py == 0 || py >= h - 1 {
        return;
    }
    // 1px dark outline around text pixels so it reads crisply on both light and dark GitHub themes
    for dy in -1_i32..=1 {
        for dx in -1_i32..=1 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let nx = (px as i32 + dx) as u32;
            let ny = (py as i32 + dy) as u32;
            let n_idx = (ny * w + nx) as usize;
            if rgba_pixels[n_idx][3] == 0 {
                rgba_pixels[n_idx] = OUTLINE_COLOR;
            }
        }
    }
    rgba_pixels[(py * w + px) as usize] = color;
}

/// Compact 3x5 pixel-art bitmap font for uppercase letters, digits, and punctuation.
fn glyph_3x5(ch: char) -> [u8; 5] {
    match ch.to_ascii_uppercase() {
        'A' => [0b010, 0b101, 0b111, 0b101, 0b101],
        'B' => [0b110, 0b101, 0b110, 0b101, 0b110],
        'C' => [0b011, 0b100, 0b100, 0b100, 0b011],
        'D' => [0b110, 0b101, 0b101, 0b101, 0b110],
        'E' => [0b111, 0b100, 0b110, 0b100, 0b111],
        'F' => [0b111, 0b100, 0b110, 0b100, 0b100],
        'G' => [0b011, 0b100, 0b101, 0b101, 0b011],
        'H' => [0b101, 0b101, 0b111, 0b101, 0b101],
        'I' => [0b111, 0b010, 0b010, 0b010, 0b111],
        'J' => [0b001, 0b001, 0b001, 0b101, 0b010],
        'K' => [0b101, 0b101, 0b110, 0b101, 0b101],
        'L' => [0b100, 0b100, 0b100, 0b100, 0b111],
        'M' => [0b101, 0b111, 0b111, 0b101, 0b101],
        'N' => [0b110, 0b101, 0b101, 0b101, 0b101],
        'O' => [0b010, 0b101, 0b101, 0b101, 0b010],
        'P' => [0b110, 0b101, 0b110, 0b100, 0b100],
        'Q' => [0b010, 0b101, 0b101, 0b011, 0b001],
        'R' => [0b110, 0b101, 0b110, 0b101, 0b101],
        'S' => [0b011, 0b100, 0b010, 0b001, 0b110],
        'T' => [0b111, 0b010, 0b010, 0b010, 0b010],
        'U' => [0b101, 0b101, 0b101, 0b101, 0b111],
        'V' => [0b101, 0b101, 0b101, 0b101, 0b010],
        'W' => [0b101, 0b101, 0b111, 0b111, 0b101],
        'X' => [0b101, 0b101, 0b010, 0b101, 0b101],
        'Y' => [0b101, 0b101, 0b010, 0b010, 0b010],
        'Z' => [0b111, 0b001, 0b010, 0b100, 0b111],
        '0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        '1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        '2' => [0b111, 0b001, 0b111, 0b100, 0b111],
        '3' => [0b111, 0b001, 0b011, 0b001, 0b111],
        '4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        '5' => [0b111, 0b100, 0b111, 0b001, 0b111],
        '6' => [0b111, 0b100, 0b111, 0b101, 0b111],
        '7' => [0b111, 0b001, 0b001, 0b010, 0b010],
        '8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        '9' => [0b111, 0b101, 0b111, 0b001, 0b111],
        ':' => [0b000, 0b010, 0b000, 0b010, 0b000],
        '-' => [0b000, 0b000, 0b111, 0b000, 0b000],
        '.' => [0b000, 0b000, 0b000, 0b000, 0b010],
        '+' => [0b000, 0b010, 0b111, 0b010, 0b000],
        '#' => [0b101, 0b111, 0b101, 0b111, 0b101],
        _ => [0b000, 0b000, 0b000, 0b000, 0b000],
    }
}
