//! Full-body procedural pixel-art Axolotl renderer (Baby Calf -> Yearling -> Adolescent -> Adult Female -> Adult Male Bull)
//! with realistic deep-ocean cetacean swimming physics (slow, heavy, majestic), flapping pectoral scythe-wings,
//! trailing kuru neural whips, dual tail flukes, 4x4 ordered Bayer dithering, and bottom-left pixel-art HUD metrics.

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use crate::coat::{ShadeRamp, AxolotlCoat};
use crate::history::AccountSnapshot;

/// Native canvas width in pixels (exported at 2x scale = 640px).
pub const FRAME_WIDTH_PIXELS: u32 = 320;
/// Native canvas height in pixels (exported at 2x scale = 280px).
pub const FRAME_HEIGHT_PIXELS: u32 = 140;
/// Animation frame rate (15 FPS).
pub const FRAMES_PER_SECOND: u32 = 15;
/// Total frames in one seamless deep-ocean swimming loop (75 frames = 5.0 seconds per slow cycle).
pub const LOOP_FRAME_COUNT: u32 = 48;

pub const OUTLINE_COLOR: [u8; 4] = [15, 18, 26, 255];

const BAYER_4X4: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

/// Camera perspective for rendering the Axolotl.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CameraView {
    /// Lateral side profile swim (shows calf-to-bull sensor crest, jaw triangles, and full fins).
    Side,
    /// Dynamic 3/4 dive perspective (Payakan reference pose with split head scoop).
    Dive,
    /// Dorsal top-down swim view.
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

/// Biological growth parameters derived from commit/contribution count.
#[derive(Clone, Copy, Debug)]
pub struct AxolotlBiology {
    pub commits: u64,
    pub growth: f64,
    pub maturity: f64,
    pub female_phase: f64,
    pub bull_phase: f64,
    pub body_len: f64,
    pub max_radius: f64,
    pub fin_stubbiness: f64,
    pub kuru_scale: f64,
    pub jaw_tooth_count: f64,
}

impl AxolotlBiology {
    /// Compute Axolotl life-stage proportions on a log-like curve (`0` through `2,500+` contributions):
    /// - `0..150`: Stage 1: Baby Axolotl Calf
    /// - `150..500`: Stage 2: Yearling Calf
    /// - `500..1,200`: Stage 3: Adolescent Axolotl
    /// - `1,200..2,500`: Stage 4: Adult Female
    /// - `2,500+`: Stage 5: Adult Male Bull
    pub fn from_commits(commits: u64) -> Self {
        let g = ((commits as f64) / 2800.0).powf(0.45).clamp(0.0, 1.0);
        let maturity = smooth_step((g - 0.14) / 0.82);
        let female_phase = smooth_step((g - 0.42) / 0.34);
        let bull_phase = smooth_step((g - 0.72) / 0.25);

        let body_len = lerp(64.0, 186.0, g);
        let fatness_ratio = lerp(0.165, 0.115, maturity);
        let max_radius = body_len * fatness_ratio;

        Self {
            commits,
            growth: g,
            maturity,
            female_phase,
            bull_phase,
            body_len,
            max_radius,
            fin_stubbiness: lerp(0.52, 1.16, maturity),
            kuru_scale: smooth_step((g - 0.12) / 0.48),
            jaw_tooth_count: lerp(2.5, 6.0, maturity).round(),
        }
    }

    pub fn stage_name(&self) -> &'static str {
        if self.commits >= 2500 {
            "Adult Male Bull (Hooked Hammer-Crest)"
        } else if self.commits >= 1200 {
            "Mature Axolotl (Sloped Sensor Crest)"
        } else if self.commits >= 500 {
            "Adolescent Axolotl"
        } else if self.commits >= 150 {
            "Yearling Calf (Kuru Active)"
        } else {
            "Baby Axolotl Calf"
        }
    }

    pub fn hud_stage_label(&self) -> &'static str {
        if self.commits >= 2500 {
            "STAGE 5: ADULT BULL"
        } else if self.commits >= 1200 {
            "STAGE 4: MATURE AXOLOTL"
        } else if self.commits >= 500 {
            "STAGE 3: ADOLESCENT"
        } else if self.commits >= 150 {
            "STAGE 2: YEARLING CALF"
        } else {
            "STAGE 1: BABY CALF"
        }
    }
}

#[derive(Clone, Copy)]
struct SpineNode {
    x: f64,
    y: f64,
    r_top: f64,
    r_bot: f64,
    r: f64,
    u: f64,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum PartKind {
    Opercular,
    MainWing,
    AuxFlipper,
    Kuru,
    Fluke,
    CrimsonScoop,
}

struct AppendagePart {
    z: i32,
    kind: PartKind,
    pts: Vec<SpineNode>,
}

struct SampleHit {
    u: f64,
    dist: f64,
    active_r: f64,
    lat_frac: f64,
    nx: f64,
    ny: f64,
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

fn sample_chain(pts: &[SpineNode], sx: f64, sy: f64) -> Option<SampleHit> {
    let mut best_norm = f64::INFINITY;
    let mut best = None;
    for seg in pts.windows(2) {
        let a = seg[0];
        let b = seg[1];
        let vx = b.x - a.x;
        let vy = b.y - a.y;
        let l_sq = vx * vx + vy * vy;
        if l_sq < 1e-6 {
            continue;
        }
        let ox = sx - a.x;
        let oy = sy - a.y;
        let t = ((ox * vx + oy * vy) / l_sq).clamp(0.0, 1.0);
        let cx = a.x + t * vx;
        let cy = a.y + t * vy;
        let dist = (sx - cx).hypot(sy - cy);
        let r_top = a.r_top + t * (b.r_top - a.r_top);
        let r_bot = a.r_bot + t * (b.r_bot - a.r_bot);
        let len = l_sq.sqrt();
        let signed_lat = (ox * vy - oy * vx) / len;
        let active_r = if signed_lat < 0.0 { r_top } else { r_bot };
        let norm = dist / active_r;

        if dist <= active_r && norm < best_norm {
            best_norm = norm;
            best = Some(SampleHit {
                u: a.u + t * (b.u - a.u),
                dist,
                active_r,
                lat_frac: signed_lat / active_r,
                nx: (sx - cx) / active_r,
                ny: (sy - cy) / active_r,
            });
        }
    }
    best
}

fn pt_node(x: f64, y: f64, r: f64, u: f64) -> SpineNode {
    SpineNode {
        x,
        y,
        r_top: r,
        r_bot: r,
        r,
        u,
    }
}

/// Render a single still frame of the Axolotl.
pub fn render_still_frame(
    snapshot: &AccountSnapshot,
    coat: &AxolotlCoat,
    view: CameraView,
) -> AxolotlFrame {
    render_swim_frame(snapshot, coat, view, 12)
}

/// Render frame `frame_index` (`0..LOOP_FRAME_COUNT`) of the Axolotl's seamless deep-ocean swimming loop.
pub fn render_swim_frame(
    snapshot: &AccountSnapshot,
    coat: &AxolotlCoat,
    view: CameraView,
    frame_index: u32,
) -> AxolotlFrame {
    let w = FRAME_WIDTH_PIXELS;
    let h = FRAME_HEIGHT_PIXELS;
    let mut rgba_pixels = vec![[0_u8; 4]; (w * h) as usize];

    let effective_commits = snapshot
        .total_contributions
        .max(snapshot.monthly_contributions);
    let bio = AxolotlBiology::from_commits(effective_commits);

    // Seamless slow 5-second period phase in [0, TAU)
    let loop_turn = TAU * f64::from(frame_index % LOOP_FRAME_COUNT) / f64::from(LOOP_FRAME_COUNT);

    let body_len = bio.body_len;
    let _max_radius = bio.max_radius;
    let n_pts = (body_len.round() as usize).max(36);

    // Compute fixed reference bounding box at rest (loop_turn = 0) so the camera NEVER jitters or bounces!
    let (rest_spine, rest_parts) = build_pose_geometry(&bio, view, 0.0, w, h);
    let mut ref_min_x = f64::INFINITY;
    let mut ref_min_y = f64::INFINITY;
    let mut ref_max_x = f64::NEG_INFINITY;
    let mut ref_max_y = f64::NEG_INFINITY;
    for step in 0..8 {
        let (s_spine, s_parts) = build_pose_geometry(&bio, view, TAU * (step as f64 / 8.0), w, h);
        for p in &s_spine {
            ref_min_x = ref_min_x.min(p.x - p.r - 4.0);
            ref_max_x = ref_max_x.max(p.x + p.r + 4.0);
            ref_min_y = ref_min_y.min(p.y - p.r_top - 4.0);
            ref_max_y = ref_max_y.max(p.y + p.r_bot + 4.0);
        }
        for part in &s_parts {
            for p in &part.pts {
                ref_min_x = ref_min_x.min(p.x - p.r - 4.0);
                ref_max_x = ref_max_x.max(p.x + p.r + 4.0);
                ref_min_y = ref_min_y.min(p.y - p.r - 4.0);
                ref_max_y = ref_max_y.max(p.y + p.r + 4.0);
            }
        }
    }

    for p in &rest_spine {
        ref_min_x = ref_min_x.min(p.x - p.r - 4.0);
        ref_max_x = ref_max_x.max(p.x + p.r + 4.0);
        ref_min_y = ref_min_y.min(p.y - p.r_top - 4.0);
        ref_max_y = ref_max_y.max(p.y + p.r_bot + 4.0);
    }
    for part in &rest_parts {
        for p in &part.pts {
            ref_min_x = ref_min_x.min(p.x - p.r - 4.0);
            ref_max_x = ref_max_x.max(p.x + p.r + 4.0);
            ref_min_y = ref_min_y.min(p.y - p.r - 4.0);
            ref_max_y = ref_max_y.max(p.y + p.r + 4.0);
        }
    }

    // Reserve bottom 24px for the bottom-left HUD metrics so the whale never overlaps the text
    let hud_reserved_bottom = 22.0;
    let avail_w = f64::from(w) - 16.0;
    let avail_h = f64::from(h) - hud_reserved_bottom - 10.0;
    let raw_w = (ref_max_x - ref_min_x).max(1.0);
    let raw_h = (ref_max_y - ref_min_y).max(1.0);
    let fit_scale = (avail_w / raw_w).min(avail_h / raw_h).min(1.0);
    let ref_cx = (ref_min_x + ref_max_x) * 0.5;
    let ref_cy = (ref_min_y + ref_max_y) * 0.5;

    // Subtle, majestic deep-ocean glide drift (only 6px horizontal, 1.5px vertical)
    let glide_x = loop_turn.sin() * 5.0;
    let glide_y = loop_turn.cos() * 2.5;
    let target_cx = f64::from(w) * 0.52 + glide_x;
    let target_cy = (f64::from(h) - hud_reserved_bottom) * 0.48 + glide_y;

    // Build animated pose for current frame
    let (mut spine, mut parts) = build_pose_geometry(&bio, view, loop_turn, w, h);

    for p in &mut spine {
        p.x = target_cx + (p.x - ref_cx) * fit_scale;
        p.y = target_cy + (p.y - ref_cy) * fit_scale;
        p.r_top *= fit_scale;
        p.r_bot *= fit_scale;
        p.r *= fit_scale;
    }
    for part in &mut parts {
        for p in &mut part.pts {
            p.x = target_cx + (p.x - ref_cx) * fit_scale;
            p.y = target_cy + (p.y - ref_cy) * fit_scale;
            p.r_top *= fit_scale;
            p.r_bot *= fit_scale;
            p.r *= fit_scale;
        }
    }

    let mut min_x = w as usize;
    let mut min_y = h as usize;
    let mut max_x = 0_usize;
    let mut max_y = 0_usize;
    for p in &spine {
        let mr = p.r_top.max(p.r_bot) + 3.0;
        min_x = min_x.min((p.x - mr).floor().max(1.0) as usize);
        min_y = min_y.min((p.y - mr).floor().max(1.0) as usize);
        max_x = max_x.max(((p.x + mr).ceil() as usize).min(w as usize - 2));
        max_y = max_y.max(((p.y + mr).ceil() as usize).min(h as usize - 2));
    }
    for part in &parts {
        for p in &part.pts {
            min_x = min_x.min((p.x - p.r - 3.0).floor().max(1.0) as usize);
            min_y = min_y.min((p.y - p.r - 3.0).floor().max(1.0) as usize);
            max_x = max_x.max(((p.x + p.r + 3.0).ceil() as usize).min(w as usize - 2));
            max_y = max_y.max(((p.y + p.r + 3.0).ceil() as usize).min(h as usize - 2));
        }
    }

    let light = [-0.30, -0.62, 0.72];

    let shade_part = |part: &AppendagePart, s: &SampleHit, px: u32, py: u32| -> [u8; 4] {
        let is_rim = s.dist > s.active_r - 0.7 && s.active_r > 1.6;
        let ramp = match part.kind {
            PartKind::MainWing | PartKind::Opercular => {
                if s.u > 0.62 || s.lat_frac.abs() > 0.38 {
                    &coat.cream
                } else if bio.maturity < 0.4 {
                    &coat.belly
                } else {
                    &coat.armor
                }
            }
            PartKind::Fluke => {
                let fluke_darken = smooth_noise(s.u * 6.0, s.lat_frac * 4.0, 55);
                if fluke_darken < bio.maturity * 1.15 {
                    if bio.maturity < 0.45 {
                        &coat.belly
                    } else {
                        &coat.armor
                    }
                } else {
                    &coat.cream
                }
            }
            PartKind::CrimsonScoop => &coat.crest_core,
            PartKind::Kuru => &coat.belly,
            PartKind::AuxFlipper => {
                if bio.maturity < 0.35 {
                    &coat.belly
                } else {
                    &coat.armor
                }
            }
        };

        if is_rim {
            return ramp[0];
        }
        let nz = (1.0 - (s.nx * s.nx + s.ny * s.ny)).max(0.0).sqrt();
        let mut bright = 0.26 + 0.72 * (s.nx * light[0] + s.ny * light[1] + nz * light[2]).max(0.0);
        if part.kind == PartKind::CrimsonScoop && (s.u * 18.0).sin() > 0.2 {
            bright += 0.18;
        }
        choose_shade(ramp, bright, px, py)
    };

    for py in min_y..=max_y {
        for px in min_x..=max_x {
            let sx = px as f64 + 0.5;
            let sy = py as f64 + 0.5;
            let p_idx = py * (w as usize) + px;

            let mut painted = false;
            for part in parts.iter().filter(|p| p.z > 0) {
                if let Some(s) = sample_chain(&part.pts, sx, sy) {
                    rgba_pixels[p_idx] = shade_part(part, &s, px as u32, py as u32);
                    painted = true;
                    break;
                }
            }
            if painted {
                continue;
            }

            if let Some(b) = sample_chain(&spine, sx, sy) {
                if view != CameraView::Top
                    && bio.bull_phase > 0.35
                    && b.u > 0.08
                    && b.u < 0.18
                    && b.ny < -0.55
                    && (b.u - 0.08) / 0.10 < 0.65 * bio.bull_phase
                {
                    continue;
                }

                let mut ramp = if bio.maturity < 0.38 {
                    &coat.belly
                } else {
                    &coat.armor
                };

                if view == CameraView::Top {
                    let band_noise = (b.u * 38.0 + b.lat_frac.abs() * 5.5).sin();
                    if b.u < 0.18 && b.lat_frac.abs() < 0.62 && bio.female_phase > 0.25 {
                        ramp = &coat.crest_core;
                    } else if band_noise > (bio.maturity * 1.35 - 0.25) {
                        ramp = &coat.cream;
                    }
                } else {
                    let is_ventral = b.ny > 0.12;
                    if is_ventral {
                        ramp = if bio.maturity < 0.55 || b.u < 0.35 {
                            &coat.cream
                        } else {
                            &coat.belly
                        };
                    }
                    if b.u < 0.18 && b.ny < -0.38 {
                        ramp = if bio.female_phase > 0.3 && bio.bull_phase < 0.4 && b.u < 0.04 {
                            &coat.crest_tip
                        } else {
                            &coat.crest_core
                        };
                    }
                    if b.u > 0.03 && b.u < 0.22 && b.ny > -0.15 && b.ny < 0.68 {
                        let tooth = ((b.u - 0.03) * bio.jaw_tooth_count * 32.0).sin();
                        ramp = if b.ny > 0.15 + tooth.max(0.0) * 0.32 {
                            &coat.cream
                        } else if bio.maturity < 0.38 {
                            &coat.belly
                        } else {
                            &coat.armor
                        };
                    }
                    if !is_ventral && b.u > 0.18 {
                        let swirl =
                            (b.u * 34.0 - b.ny * 5.0).sin() + 0.5 * (b.u * 19.0 + b.ny * 7.0).cos();
                        let threshold = lerp(0.15, 1.18, bio.maturity);
                        if swirl > threshold {
                            ramp = &coat.cream;
                        }
                    }
                    if (b.u - 0.185).abs() * body_len * fit_scale < 2.2
                        && (b.ny - 0.02).abs() * b.active_r < 2.2
                    {
                        rgba_pixels[p_idx] = [240, 175, 95, 255];
                        continue;
                    }
                }

                if b.dist > b.active_r - 0.8 {
                    rgba_pixels[p_idx] = ramp[0];
                    continue;
                }

                let nz = (1.0 - (b.nx * b.nx + b.ny * b.ny)).max(0.0).sqrt();
                let mut bright =
                    0.24 + 0.74 * (b.nx * light[0] + b.ny * light[1] + nz * light[2]).max(0.0);

                if b.ny > 0.2 && b.u > 0.45 && b.u < 0.85 && (b.u * 65.0).sin() > 0.62 {
                    bright -= 0.22;
                }

                rgba_pixels[p_idx] = choose_shade(ramp, bright, px as u32, py as u32);
                continue;
            }

            for part in parts.iter().filter(|p| p.z < 0) {
                if let Some(s) = sample_chain(&part.pts, sx, sy) {
                    rgba_pixels[p_idx] = shade_part(part, &s, px as u32, py as u32);
                    break;
                }
            }
        }
    }

    // 1px Dark Pixel-Art Outline Pass around the Axolotl
    let orig = rgba_pixels.clone();
    let stride = w as usize;
    for py in min_y..=max_y {
        for px in min_x..=max_x {
            let idx = py * stride + px;
            if orig[idx][3] != 0 {
                continue;
            }
            if orig[idx - 1][3] != 0
                || orig[idx + 1][3] != 0
                || orig[idx - stride][3] != 0
                || orig[idx + stride][3] != 0
            {
                rgba_pixels[idx] = OUTLINE_COLOR;
            }
        }
    }

    // Paint subtle drifting deep-ocean bioluminescent plankton motes
    // Removed background dots around creature as requested

    // Paint Bottom-Left Pixel-Art Metrics HUD: Stage Label, Contributions & Top Languages
    paint_metrics_hud(&mut rgba_pixels, w, h, &bio, coat, snapshot);

    let _ = n_pts;
    AxolotlFrame { rgba_pixels }
}

/// Build the spine and appendages for a given `loop_turn` phase (`0..TAU`) with realistic, gentle deep-sea motion.
fn build_pose_geometry(
    bio: &AxolotlBiology,
    view: CameraView,
    loop_turn: f64,
    w: u32,
    h: u32,
) -> (Vec<SpineNode>, Vec<AppendagePart>) {
    let body_len = bio.body_len;
    let max_radius = bio.max_radius;
    let n_pts = (body_len.round() as usize).max(36);
    let mut spine = Vec::with_capacity(n_pts);

    let dir = if view == CameraView::Dive { 1.0 } else { -1.0 };
    let start_x = f64::from(w) * 0.50 + dir * (body_len * 0.42);
    let center_y = f64::from(h) * 0.44;

    // Realistic, slow, heavy cetacean physics (single 1x harmonic per 75-frame cycle, ~75% lower amplitude!)
    let pitch_angle = loop_turn.cos() * 0.065;
    let wave_amp = max_radius * 0.62;

    for i in 0..n_pts {
        let u = i as f64 / (n_pts - 1) as f64;
        let x = start_x - dir * (u * body_len);

        // Heavy head stays steady; smooth single-harmonic traveling wave glides down the tail stock
        let head_bob = loop_turn.sin() * (max_radius * 0.14) * (1.0 - u).powf(1.3);
        let tail_stroke = (u * PI * 2.35 - loop_turn).sin() * wave_amp * u.powf(0.92);
        let pitch_offset = (u - 0.35) * body_len * pitch_angle.sin();

        let mut y = center_y + head_bob + tail_stroke + pitch_offset;
        if view == CameraView::Dive {
            let arch = -(u * PI * 0.92).sin() * (max_radius * 1.05)
                + if u < 0.45 {
                    ((0.45 - u) / 0.45).powf(1.4) * (max_radius * 0.70)
                } else {
                    0.0
                };
            y += arch;
        }

        let (mut r_top, r_bot) = if u < 0.24 {
            let hu = u / 0.24;
            let base_top = max_radius * lerp(0.38, 0.92, hu.powf(0.55));
            let calf_bump = (hu * PI).sin() * (max_radius * 0.16);
            let fem_wedge = (1.0 - hu).max(0.0).powf(1.2)
                * ((hu * 4.0).min(1.0) * FRAC_PI_2).sin()
                * (max_radius * 0.65);
            let bull_crest = if hu < 0.45 {
                ((hu / 0.45) * FRAC_PI_2).sin() * (max_radius * 1.35)
            } else {
                (((1.0 - hu) / 0.55).max(0.0)).powf(1.6) * (max_radius * 0.75)
            };
            let crest_add = if view == CameraView::Top {
                0.0
            } else {
                calf_bump * (1.0 - bio.female_phase)
                    + fem_wedge * bio.female_phase * (1.0 - bio.bull_phase * 0.4)
                    + bull_crest * bio.bull_phase
            };
            let bot = max_radius * lerp(0.32, 0.96, ((hu * 1.3).min(1.0) * FRAC_PI_2).sin());
            (base_top + crest_add, bot)
        } else if u < 0.68 {
            let mu = (u - 0.24) / 0.44;
            let env = (mu * PI * 0.48).cos();
            (max_radius * 0.95 * env, max_radius * 0.98 * env)
        } else {
            let tu = (u - 0.68) / 0.32;
            let taper = lerp(0.48, 0.13, tu.powf(0.85));
            (max_radius * taper, max_radius * taper)
        };

        r_top = r_top.max(1.2);
        let r_bot = r_bot.max(1.2);
        spine.push(SpineNode {
            x,
            y,
            r_top,
            r_bot,
            r: (r_top + r_bot) * 0.5,
            u,
        });
    }

    let get_pt = |u_val: f64| -> SpineNode {
        let idx = ((u_val * (spine.len() - 1) as f64).round() as usize).min(spine.len() - 1);
        spine[idx]
    };

    let mut parts = Vec::new();
    let fin_scale = bio.fin_stubbiness;
    // Slow, graceful deep-ocean fin sculling (1x loop_turn instead of 2x, gentle amplitude)
    let wing_flap = (loop_turn - 0.65).sin() * (max_radius * 0.78);
    let aux_flap = (loop_turn - 0.95).cos() * (max_radius * 0.42);
    let fluke_whip = (loop_turn - 2.05).cos() * (max_radius * 0.72);

    if view == CameraView::Top {
        for side in [-1.0, 1.0] {
            let op_root = get_pt(0.23);
            let op_len = max_radius * 1.35 * fin_scale;
            parts.push(AppendagePart {
                z: 2,
                kind: PartKind::Opercular,
                pts: vec![
                    pt_node(
                        op_root.x,
                        op_root.y + side * op_root.r * 0.75,
                        max_radius * 0.26,
                        0.0,
                    ),
                    pt_node(
                        op_root.x - dir * (op_len * 0.45 + aux_flap * 0.3),
                        op_root.y + side * (op_root.r * 0.75 + op_len * 0.85),
                        max_radius * 0.22,
                        0.5,
                    ),
                    pt_node(
                        op_root.x - dir * (op_len * 0.75 + aux_flap * 0.6),
                        op_root.y + side * (op_root.r * 0.75 + op_len * 1.15),
                        1.0,
                        1.0,
                    ),
                ],
            });

            let main_root = get_pt(0.38);
            let main_len = max_radius * 3.45 * fin_scale;
            parts.push(AppendagePart {
                z: 3,
                kind: PartKind::MainWing,
                pts: vec![
                    pt_node(
                        main_root.x,
                        main_root.y + side * main_root.r * 0.75,
                        max_radius * 0.42,
                        0.0,
                    ),
                    pt_node(
                        main_root.x - dir * (main_len * 0.28 + wing_flap * 0.3),
                        main_root.y + side * (main_root.r + main_len * 0.52),
                        max_radius * 0.36,
                        0.33,
                    ),
                    pt_node(
                        main_root.x - dir * (main_len * 0.68 + wing_flap * 0.7),
                        main_root.y + side * (main_root.r + main_len * 0.96),
                        max_radius * 0.22,
                        0.66,
                    ),
                    pt_node(
                        main_root.x - dir * (main_len * 0.98 + wing_flap * 1.1),
                        main_root.y + side * (main_root.r + main_len * 1.18),
                        1.0,
                        1.0,
                    ),
                ],
            });

            if bio.kuru_scale > 0.05 {
                let k_len = max_radius * 2.6 * bio.kuru_scale;
                parts.push(AppendagePart {
                    z: 1,
                    kind: PartKind::Kuru,
                    pts: vec![
                        pt_node(
                            op_root.x - dir * 4.0,
                            op_root.y + side * op_root.r * 0.9,
                            1.2,
                            0.0,
                        ),
                        pt_node(
                            op_root.x - dir * k_len * 0.6,
                            op_root.y + side * (op_root.r * 1.15 + k_len * 0.22 + aux_flap * 0.4),
                            0.95,
                            0.5,
                        ),
                        pt_node(
                            op_root.x - dir * k_len * 1.15,
                            op_root.y + side * (op_root.r * 1.05 + k_len * 0.15 - aux_flap * 0.5),
                            0.75,
                            1.0,
                        ),
                    ],
                });
            }

            let pelv_root = get_pt(0.82);
            let p_len = max_radius * 1.15 * fin_scale;
            parts.push(AppendagePart {
                z: 2,
                kind: PartKind::Fluke,
                pts: vec![
                    pt_node(
                        pelv_root.x,
                        pelv_root.y + side * pelv_root.r * 0.6,
                        max_radius * 0.22,
                        0.0,
                    ),
                    pt_node(
                        pelv_root.x - dir * p_len * 0.65,
                        pelv_root.y + side * (pelv_root.r + p_len * 0.75),
                        max_radius * 0.28,
                        0.5,
                    ),
                    pt_node(
                        pelv_root.x - dir * p_len * 1.0,
                        pelv_root.y + side * (pelv_root.r + p_len * 0.95),
                        1.1,
                        1.0,
                    ),
                ],
            });

            let c_root = get_pt(0.96);
            let c_len = max_radius * 2.15 * fin_scale;
            parts.push(AppendagePart {
                z: 2,
                kind: PartKind::Fluke,
                pts: vec![
                    pt_node(c_root.x, c_root.y + side * 1.5, max_radius * 0.26, 0.0),
                    pt_node(
                        c_root.x - dir * c_len * 0.55,
                        c_root.y + side * c_len * 0.48 + fluke_whip * 0.3,
                        max_radius * 0.34,
                        0.5,
                    ),
                    pt_node(
                        c_root.x - dir * c_len * 1.05,
                        c_root.y + side * c_len * 0.72 + fluke_whip * 0.6,
                        1.0,
                        1.0,
                    ),
                ],
            });
        }
    } else {
        let op_root = get_pt(0.24);
        let op_len = max_radius * 1.15 * fin_scale;
        parts.push(AppendagePart {
            z: 3,
            kind: PartKind::Opercular,
            pts: vec![
                pt_node(
                    op_root.x,
                    op_root.y + op_root.r_bot * 0.35,
                    max_radius * 0.28,
                    0.0,
                ),
                pt_node(
                    op_root.x - dir * op_len * 0.35,
                    op_root.y + op_root.r_bot * 0.35 + op_len * 0.65 + aux_flap * 0.5,
                    max_radius * 0.22,
                    0.5,
                ),
                pt_node(
                    op_root.x - dir * op_len * 0.55,
                    op_root.y + op_root.r_bot * 0.35 + op_len * 0.95 + aux_flap,
                    1.1,
                    1.0,
                ),
            ],
        });

        if bio.kuru_scale > 0.05 {
            let k_end = get_pt(0.44);
            parts.push(AppendagePart {
                z: 2,
                kind: PartKind::Kuru,
                pts: vec![
                    pt_node(
                        op_root.x - dir * 3.0,
                        op_root.y + op_root.r_bot * 0.85,
                        1.2,
                        0.0,
                    ),
                    pt_node(
                        (op_root.x + k_end.x) * 0.5,
                        op_root.y + op_root.r_bot * 1.25 + 3.0 + aux_flap * 0.6,
                        1.0,
                        0.5,
                    ),
                    pt_node(k_end.x, k_end.y + k_end.r_bot * 0.75, 0.9, 1.0),
                ],
            });
        }

        let main_root = get_pt(0.38);
        let wing_len = max_radius * 3.55 * fin_scale;
        parts.push(AppendagePart {
            z: 4,
            kind: PartKind::MainWing,
            pts: vec![
                pt_node(
                    main_root.x,
                    main_root.y + main_root.r_bot * 0.22,
                    max_radius * 0.42,
                    0.0,
                ),
                pt_node(
                    main_root.x - dir * wing_len * 0.36,
                    main_root.y + wing_len * 0.28 + wing_flap * 0.35,
                    max_radius * 0.36,
                    0.33,
                ),
                pt_node(
                    main_root.x - dir * wing_len * 0.72,
                    main_root.y + wing_len * 0.54 + wing_flap * 0.75,
                    max_radius * 0.24,
                    0.66,
                ),
                pt_node(
                    main_root.x - dir * wing_len * 1.02,
                    main_root.y + wing_len * 0.72 + wing_flap * 1.15,
                    1.1,
                    1.0,
                ),
            ],
        });

        if bio.maturity > 0.22 {
            let v_root = get_pt(0.45);
            let v_len = max_radius * 1.85 * fin_scale * bio.maturity;
            parts.push(AppendagePart {
                z: -2,
                kind: PartKind::AuxFlipper,
                pts: vec![
                    pt_node(
                        v_root.x,
                        v_root.y + v_root.r_bot * 0.75,
                        max_radius * 0.30,
                        0.0,
                    ),
                    pt_node(
                        v_root.x - dir * (v_len * 0.12 - aux_flap * 0.3),
                        v_root.y + v_root.r_bot * 0.75 + v_len * 0.65,
                        max_radius * 0.22,
                        0.5,
                    ),
                    pt_node(
                        v_root.x - dir * (v_len * 0.24 - aux_flap * 0.6),
                        v_root.y + v_root.r_bot * 0.75 + v_len * 1.02,
                        1.0,
                        1.0,
                    ),
                ],
            });
        }

        let pelv_root = get_pt(0.82);
        let p_scale = max_radius * 1.10 * fin_scale;
        parts.push(AppendagePart {
            z: -1,
            kind: PartKind::Fluke,
            pts: vec![
                pt_node(
                    pelv_root.x,
                    pelv_root.y + pelv_root.r_bot * 0.6,
                    p_scale * 0.24,
                    0.0,
                ),
                pt_node(
                    pelv_root.x - dir * p_scale * 0.55,
                    pelv_root.y + pelv_root.r_bot * 0.7 + p_scale * 0.45 + fluke_whip * 0.4,
                    p_scale * 0.32,
                    0.5,
                ),
                pt_node(
                    pelv_root.x - dir * p_scale * 0.85,
                    pelv_root.y + pelv_root.r_bot * 0.7 + p_scale * 0.62 + fluke_whip * 0.7,
                    1.1,
                    1.0,
                ),
            ],
        });

        let tail_root = get_pt(0.96);
        let f_len = max_radius * 2.20 * fin_scale;
        parts.push(AppendagePart {
            z: -1,
            kind: PartKind::Fluke,
            pts: vec![
                pt_node(tail_root.x, tail_root.y, tail_root.r * 0.9, 0.0),
                pt_node(
                    tail_root.x - dir * f_len * 0.55,
                    tail_root.y - f_len * 0.14 + fluke_whip * 0.5,
                    max_radius * 0.25,
                    0.5,
                ),
                pt_node(
                    tail_root.x - dir * f_len * 0.98,
                    tail_root.y - f_len * 0.20 + fluke_whip,
                    0.9,
                    1.0,
                ),
            ],
        });
        parts.push(AppendagePart {
            z: 2,
            kind: PartKind::Fluke,
            pts: vec![
                pt_node(tail_root.x, tail_root.y + 1.0, tail_root.r * 0.95, 0.0),
                pt_node(
                    tail_root.x - dir * f_len * 0.50,
                    tail_root.y + f_len * 0.16 + fluke_whip * 0.5,
                    max_radius * 0.28,
                    0.5,
                ),
                pt_node(
                    tail_root.x - dir * f_len * 1.05,
                    tail_root.y + f_len * 0.32 + fluke_whip,
                    0.9,
                    1.0,
                ),
            ],
        });

        if view == CameraView::Dive && bio.female_phase > 0.2 {
            let snout = get_pt(0.03);
            let s_len = max_radius * 1.65 * bio.female_phase;
            parts.push(AppendagePart {
                z: -2,
                kind: PartKind::CrimsonScoop,
                pts: vec![
                    pt_node(snout.x - 4.0, snout.y, max_radius * 0.50, 0.0),
                    pt_node(
                        snout.x + s_len * 0.55,
                        snout.y - s_len * 0.12,
                        max_radius * 0.62,
                        0.5,
                    ),
                    pt_node(
                        snout.x + s_len * 0.95,
                        snout.y - s_len * 0.22,
                        max_radius * 0.38,
                        1.0,
                    ),
                ],
            });
        }
    }

    (spine, parts)
}

/// Paint subtle, slowly drifting deep-ocean bioluminescent plankton motes around the whale.
#[allow(dead_code)]
fn paint_deep_ocean_motes(
    rgba_pixels: &mut [[u8; 4]],
    w: u32,
    h: u32,
    loop_turn: f64,
    coat: &AxolotlCoat,
) {
    const MOTE_SEEDS: [(f64, f64, f64); 10] = [
        (38.0, 22.0, 0.0),
        (92.0, 18.0, 1.1),
        (164.0, 14.0, 2.3),
        (248.0, 24.0, 3.7),
        (292.0, 48.0, 0.8),
        (28.0, 78.0, 4.2),
        (134.0, 108.0, 5.1),
        (218.0, 114.0, 1.9),
        (278.0, 102.0, 2.8),
        (72.0, 96.0, 3.3),
    ];

    for (i, &(bx, by, phase_off)) in MOTE_SEEDS.iter().enumerate() {
        let mx = (bx + (loop_turn + phase_off).cos() * 4.0).round() as i32;
        let my = (by + (loop_turn + phase_off).sin() * 2.5).round() as i32;
        if mx <= 2 || mx >= w as i32 - 3 || my <= 2 || my >= h as i32 - 24 {
            continue;
        }
        let idx = (my as u32 * w + mx as u32) as usize;
        if rgba_pixels[idx][3] == 0 {
            let color = if i % 2 == 0 {
                coat.belly[3]
            } else {
                coat.cream[2]
            };
            rgba_pixels[idx] = color;
        }
    }
}

/// Paint the bottom-left pixel-art HUD metrics:
/// Line 1: `<STAGE>` (e.g., `STAGE 4: MATURE AXOLOTL`, with a glowing language pip)
/// Line 2: `CONTRIBUTIONS: <count>`
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

    let pip_color = coat.belly[3];
    for dy in 0..5_u32 {
        for dx in 0..3_u32 {
            set_outlined_pixel(rgba_pixels, w, h, base_x + dx, line1_y + dy, pip_color);
        }
    }

    let line1 = bio.hud_stage_label();
    draw_pixel_string(rgba_pixels, w, h, base_x + 6, line1_y, line1, coat.cream[3]);

    let line2 = format!("CONTRIBUTIONS: {}", bio.commits);
    draw_pixel_string(rgba_pixels, w, h, base_x, line2_y, &line2, coat.belly[3]);

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
            coat.cream[2],
        );
        lang_x += (short_name.len() as u32) * 4 + 6;
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
    let border_col = coat.belly[1];
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

    let effective_commits = snapshot
        .total_contributions
        .max(snapshot.monthly_contributions);
    let bio = AxolotlBiology::from_commits(effective_commits);

    draw_pixel_string(&mut pixels, w, h, 9, 9, "GIT-AXOLOTL", coat.cream[3]);
    let stage_short = bio.hud_stage_label();
    let right_x = w.saturating_sub((stage_short.len() as u32) * 4 + 8);
    draw_pixel_string(&mut pixels, w, h, right_x, 9, stage_short, coat.belly[3]);

    // Composite still Axolotl frame into center (y_offset = 14)
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
    let stages = ["1.CALF", "2.YRLG", "3.ADOL", "4.MATR", "5.BULL"];
    let active_idx = if bio.commits >= 2500 {
        4
    } else if bio.commits >= 1200 {
        3
    } else if bio.commits >= 500 {
        2
    } else if bio.commits >= 150 {
        1
    } else {
        0
    };
    let mut sx = 148_u32;
    let sy = h - 13;
    for (idx, label) in stages.iter().enumerate() {
        let col = if idx == active_idx {
            coat.cream[3]
        } else if idx < active_idx {
            coat.belly[2]
        } else {
            coat.armor[2]
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
        _ => [0b000, 0b000, 0b000, 0b000, 0b000],
    }
}
