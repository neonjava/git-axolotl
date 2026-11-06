//! Converts GitHub language hex colors into 4-shade pixel-art dither ramps for the Axolotl's
//! carapace armor, ventral pleated belly, sensor crest core, and ivory war-stripes.

use crate::history::AccountSnapshot;

/// A 4-shade pixel-art ramp `[shadow, mid_dark, mid_light, highlight]` in RGBA.
pub type ShadeRamp = [[u8; 4]; 4];

/// Complete palette for rendering a Axolotl.
#[derive(Clone, Debug)]
pub struct AxolotlCoat {
    /// Adult dorsal carapace armor (or primary language dark ramp).
    pub armor: ShadeRamp,
    /// Calf teal skin & adult ventral pleated belly (derived from primary/secondary language).
    pub belly: ShadeRamp,
    /// White/ivory jaw war-stripes, calf dorsal patches, and pectoral wing leading edges.
    pub cream: ShadeRamp,
    /// Reddish-maroon (or secondary/tertiary language) sensor crest organ core.
    pub crest_core: ShadeRamp,
    /// Female/adolescent olive/gold crest tip accent.
    pub crest_tip: ShadeRamp,
}

pub const CANON_ARMOR: ShadeRamp = [
    [22, 26, 32, 255],
    [38, 44, 52, 255],
    [62, 70, 80, 255],
    [96, 106, 118, 255],
];

pub const CANON_BELLY: ShadeRamp = [
    [20, 58, 86, 255],
    [34, 88, 126, 255],
    [58, 128, 168, 255],
    [104, 174, 212, 255],
];

pub const CANON_CREAM: ShadeRamp = [
    [124, 132, 138, 255],
    [172, 180, 186, 255],
    [216, 222, 226, 255],
    [248, 250, 252, 255],
];

pub const CANON_CREST_RED: ShadeRamp = [
    [68, 24, 28, 255],
    [112, 38, 44, 255],
    [158, 60, 66, 255],
    [204, 96, 98, 255],
];

pub const CANON_CREST_TIP: ShadeRamp = [
    [98, 112, 64, 255],
    [142, 158, 88, 255],
    [184, 198, 122, 255],
    [220, 232, 164, 255],
];

/// Parse a `#RRGGBB` string into `[r, g, b]`.
pub fn parse_hex_color(hex: &str) -> Option<[u8; 3]> {
    let clean = hex.trim().trim_start_matches('#');
    if clean.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&clean[0..2], 16).ok()?;
    let g = u8::from_str_radix(&clean[2..4], 16).ok()?;
    let b = u8::from_str_radix(&clean[4..6], 16).ok()?;
    Some([r, g, b])
}

/// Build a 4-shade pixel-art ramp `[shadow, mid_dark, mid_light, highlight]` from a base RGB color.
pub fn build_ramp_from_rgb([r, g, b]: [u8; 3], darken_bias: f64) -> ShadeRamp {
    let scales = [
        0.34 * darken_bias,
        0.58 * darken_bias,
        0.88 * darken_bias,
        (1.18 * darken_bias).min(1.35),
    ];
    scales.map(|s| {
        let nr = (f64::from(r) * s + (s - 0.5).max(0.0) * 35.0)
            .round()
            .clamp(14.0, 255.0) as u8;
        let ng = (f64::from(g) * s + (s - 0.5).max(0.0) * 35.0)
            .round()
            .clamp(14.0, 255.0) as u8;
        let nb = (f64::from(b) * s + (s - 0.5).max(0.0) * 35.0)
            .round()
            .clamp(18.0, 255.0) as u8;
        [nr, ng, nb, 255]
    })
}

/// Blend two ramps by `weight_b` (0.0..=1.0).
pub fn blend_ramps(a: ShadeRamp, b: ShadeRamp, weight_b: f64) -> ShadeRamp {
    let w = weight_b.clamp(0.0, 1.0);
    let mut out = [[0_u8; 4]; 4];
    for i in 0..4 {
        for c in 0..3 {
            let va = f64::from(a[i][c]);
            let vb = f64::from(b[i][c]);
            out[i][c] = (va + (vb - va) * w).round().clamp(0.0, 255.0) as u8;
        }
        out[i][3] = 255;
    }
    out
}

impl AxolotlCoat {
    /// Derive a Axolotl coat from the account's top GitHub languages blended with canon Pandoran biology.
    pub fn from_snapshot(snapshot: &AccountSnapshot, use_canon_only: bool) -> Self {
        if use_canon_only || snapshot.languages.is_empty() {
            return Self {
                armor: CANON_ARMOR,
                belly: CANON_BELLY,
                cream: CANON_CREAM,
                crest_core: CANON_CREST_RED,
                crest_tip: CANON_CREST_TIP,
            };
        }

        let lang1_rgb = snapshot
            .languages
            .first()
            .and_then(|l| parse_hex_color(&l.hex_color))
            .unwrap_or([49, 120, 198]); // TypeScript Blue fallback

        let lang2_rgb = snapshot
            .languages
            .get(1)
            .and_then(|l| parse_hex_color(&l.hex_color))
            .unwrap_or([222, 165, 132]); // Rust Amber fallback

        let lang3_rgb = snapshot
            .languages
            .get(2)
            .and_then(|l| parse_hex_color(&l.hex_color))
            .unwrap_or(lang2_rgb);

        // Primary language tints the ventral belly & calf skin; darkens into the dorsal carapace armor
        let belly = blend_ramps(CANON_BELLY, build_ramp_from_rgb(lang1_rgb, 1.0), 0.65);
        let armor = blend_ramps(CANON_ARMOR, build_ramp_from_rgb(lang1_rgb, 0.52), 0.45);
        // Secondary language tints the sensor crest organ core
        let crest_core = blend_ramps(CANON_CREST_RED, build_ramp_from_rgb(lang2_rgb, 0.90), 0.58);
        // Tertiary language tints the crest tip
        let crest_tip = blend_ramps(CANON_CREST_TIP, build_ramp_from_rgb(lang3_rgb, 1.05), 0.55);

        Self {
            armor,
            belly,
            cream: CANON_CREAM,
            crest_core,
            crest_tip,
        }
    }
}
