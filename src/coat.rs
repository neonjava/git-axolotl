//! Converts GitHub language hex colors into 4-shade pixel-art dither ramps for the axolotl's
//! skin, markings, belly, fin and gills.

use crate::history::AccountSnapshot;

/// A 4-shade pixel-art ramp `[shadow, mid_dark, mid_light, highlight]` in RGBA.
pub type ShadeRamp = [[u8; 4]; 4];

/// Complete palette for rendering an axolotl.
#[derive(Clone, Debug)]
pub struct AxolotlCoat {
    /// Main body and leg skin (primary language).
    pub skin: ShadeRamp,
    /// Speckled spots on the back (darker primary language).
    pub markings: ShadeRamp,
    /// Pale underside.
    pub belly: ShadeRamp,
    /// Translucent-looking tail fin.
    pub fin: ShadeRamp,
    /// Gill stalks (secondary language).
    pub gill: ShadeRamp,
    /// Feathery gill fronds when no language tones are in use.
    pub gill_tip: ShadeRamp,
    /// One tone per top language, shown on the spots and gill fronds.
    pub tones: Vec<LanguageTone>,
}

/// A language's color on the axolotl.
#[derive(Clone, Debug)]
pub struct LanguageTone {
    pub ramp: ShadeRamp,
    /// Share of the spots this language gets, already softened so small languages stay visible.
    pub spot_share: f64,
}

/// How many languages are listed in the HUD and painted on the axolotl.
pub const TOP_LANGUAGE_COUNT: usize = 10;

pub const NATURAL_SKIN: ShadeRamp = [
    [150, 92, 104, 255],
    [205, 132, 142, 255],
    [238, 176, 178, 255],
    [255, 220, 214, 255],
];

pub const NATURAL_MARKINGS: ShadeRamp = [
    [92, 56, 74, 255],
    [128, 76, 92, 255],
    [160, 100, 112, 255],
    [188, 128, 134, 255],
];

pub const NATURAL_BELLY: ShadeRamp = [
    [206, 160, 160, 255],
    [236, 196, 190, 255],
    [250, 222, 212, 255],
    [255, 242, 232, 255],
];

pub const NATURAL_FIN: ShadeRamp = [
    [190, 130, 142, 255],
    [226, 170, 174, 255],
    [246, 204, 200, 255],
    [255, 232, 226, 255],
];

pub const NATURAL_GILL: ShadeRamp = [
    [122, 24, 54, 255],
    [176, 38, 70, 255],
    [226, 66, 92, 255],
    [250, 118, 128, 255],
];

pub const NATURAL_GILL_TIP: ShadeRamp = [
    [196, 60, 96, 255],
    [240, 104, 124, 255],
    [255, 150, 156, 255],
    [255, 200, 196, 255],
];

/// Parse a `#RRGGBB` string into `[r, g, b]`.
pub fn parse_hex_color(hex: &str) -> Option<[u8; 3]> {
    let clean = hex.trim().trim_start_matches('#');
    if clean.len() != 6 || !clean.is_ascii() {
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
    /// Derive a coat from the account's top GitHub languages, blended with natural axolotl pink.
    pub fn from_snapshot(snapshot: &AccountSnapshot, natural_only: bool) -> Self {
        if natural_only || snapshot.languages.is_empty() {
            return Self {
                skin: NATURAL_SKIN,
                markings: NATURAL_MARKINGS,
                belly: NATURAL_BELLY,
                fin: NATURAL_FIN,
                gill: NATURAL_GILL,
                gill_tip: NATURAL_GILL_TIP,
                tones: Vec::new(),
            };
        }

        let color_at = |i: usize| {
            snapshot
                .languages
                .get(i)
                .and_then(|l| parse_hex_color(&l.hex_color))
        };
        let lang1 = color_at(0).unwrap_or([49, 120, 198]);
        let lang2 = color_at(1).unwrap_or(lang1);
        let lang3 = color_at(2).unwrap_or(lang2);

        Self {
            skin: blend_ramps(NATURAL_SKIN, build_ramp_from_rgb(lang1, 1.0), 0.62),
            markings: blend_ramps(NATURAL_MARKINGS, build_ramp_from_rgb(lang1, 0.55), 0.55),
            belly: blend_ramps(NATURAL_BELLY, build_ramp_from_rgb(lang1, 1.1), 0.22),
            fin: blend_ramps(NATURAL_FIN, build_ramp_from_rgb(lang1, 1.05), 0.45),
            gill: blend_ramps(NATURAL_GILL, build_ramp_from_rgb(lang2, 0.95), 0.70),
            gill_tip: blend_ramps(NATURAL_GILL_TIP, build_ramp_from_rgb(lang3, 1.1), 0.70),
            tones: language_tones(snapshot),
        }
    }

    /// Pick a spot color from `roll` (`0.0..=1.0`). The top language keeps the dark markings,
    /// because its bright tone would vanish against skin of the same color.
    pub fn spot_ramp(&self, roll: f64) -> &ShadeRamp {
        let total: f64 = self.tones.iter().map(|t| t.spot_share).sum();
        let mut remaining = roll * total;
        for (i, tone) in self.tones.iter().enumerate() {
            remaining -= tone.spot_share;
            if remaining <= 0.0 {
                return if i == 0 { &self.markings } else { &tone.ramp };
            }
        }
        &self.markings
    }

    /// The color for gill frond number `index`, cycling through every language.
    pub fn frond_ramp(&self, index: usize) -> &ShadeRamp {
        if self.tones.is_empty() {
            &self.gill_tip
        } else {
            &self.tones[index % self.tones.len()].ramp
        }
    }
}

fn language_tones(snapshot: &AccountSnapshot) -> Vec<LanguageTone> {
    snapshot
        .languages
        .iter()
        .take(TOP_LANGUAGE_COUNT)
        .filter_map(|lang| {
            parse_hex_color(&lang.hex_color).map(|rgb| LanguageTone {
                ramp: build_ramp_from_rgb(rgb, 1.0),
                spot_share: (lang.weight as f64).sqrt().max(1.0),
            })
        })
        .collect()
}
