//! The carousel's Wi-Fi icon: Compact Arcs (approved 25 October 2026).
//!
//! A central dot and three concentric 92-degree arcs in a 17 x 13 box, drawn as an
//! anti-aliased coverage mask (16 x 16 samples per pixel), like the other icons that become
//! textures. It sits 9 px left of the clock's first digit, on the clock's baseline; the
//! clock does not move. Lit parts are the clock's white; unused arcs, and the whole symbol
//! when not connected, are the menus' muted grey at 38%; unknown is that grey, solid.

use crate::cart::CartFace;
use crate::footer::{Printed, FOOTER_Y};
use slot_gfx::{Draw, TexId};

pub const WIFI_W: u32 = 17;
pub const WIFI_H: u32 = 13;
/// Between the icon's right edge and the clock's first digit.
const GAP: f32 = 9.0;
/// The clock's digits start this far into its face (measured: 1 px for "1", 0 for "0").
const CLOCK_INK_INSET: f32 = 1.0;
/// From the top of the footer band to the top of the icon: its bottom row is the clock's
/// last row of ink (measured: digits on rows 6 to 17 of the 24 px band).
const TOP: f32 = 5.0;

const INK: [u8; 3] = [0xf6, 0xf4, 0xef];
const GREY: [u8; 3] = [0x9a, 0x9a, 0xa4];
const FAINT: f32 = 0.38;

const DOT_R: f32 = 1.45;
const DOT_CY: f32 = WIFI_H as f32 - DOT_R - 0.15;
const CX: f32 = WIFI_W as f32 / 2.0;
const STROKE: f32 = 1.6;
/// Arc centre lines, 3.2 px apart; the outer arc's top edge lands on the box's top pixel
/// boundary, so it is crisp next to the digits.
const RADII: [f32; 3] = [
    DOT_CY - STROKE / 2.0 - 6.4,
    DOT_CY - STROKE / 2.0 - 3.2,
    DOT_CY - STROKE / 2.0,
];
const HALF_SPREAD_DEG: f32 = 46.0;
const SAMPLES: u32 = 16;

#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
pub enum WifiIcon {
    /// Connected; the signal is weak (dot and one arc lit).
    Weak,
    Moderate,
    Strong,
    /// Wi-Fi is on but not connected: the whole symbol faint.
    NotConnected,
    /// Connected, but no fresh signal reading: the whole symbol grey, no level implied.
    Unknown,
}

impl WifiIcon {
    pub const ALL: [WifiIcon; 5] = [
        WifiIcon::Weak,
        WifiIcon::Moderate,
        WifiIcon::Strong,
        WifiIcon::NotConnected,
        WifiIcon::Unknown,
    ];

    /// From the number of signal bars (1 to 3), as the Networks list uses.
    pub fn from_bars(bars: u8) -> WifiIcon {
        match bars {
            0 | 1 => WifiIcon::Weak,
            2 => WifiIcon::Moderate,
            _ => WifiIcon::Strong,
        }
    }

    /// Lit arcs, for the signal levels.
    fn lit(self) -> Option<usize> {
        match self {
            WifiIcon::Weak => Some(1),
            WifiIcon::Moderate => Some(2),
            WifiIcon::Strong => Some(3),
            _ => None,
        }
    }

    /// Index into `WifiIcon::ALL`.
    pub fn index(self) -> usize {
        WifiIcon::ALL.iter().position(|&i| i == self).unwrap_or(0)
    }
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum Part {
    Dot,
    Arc(usize),
}

fn inside(part: Part, x: f32, y: f32) -> bool {
    let (dx, dy) = (x - CX, y - DOT_CY);
    let d = (dx * dx + dy * dy).sqrt();
    match part {
        Part::Dot => d <= DOT_R,
        Part::Arc(i) => {
            let r = RADII[i];
            let half = STROKE / 2.0;
            let spread = HALF_SPREAD_DEG.to_radians();
            // Angle from straight up, positive to the right.
            let a = dx.atan2(-dy);
            if a.abs() <= spread && (d - r).abs() <= half {
                return true;
            }
            // Round caps at both ends.
            [-spread, spread].iter().any(|&e| {
                let (ex, ey) = (r * e.sin(), -r * e.cos());
                let (cx, cy) = (dx - ex, dy - ey);
                (cx * cx + cy * cy).sqrt() <= half
            })
        }
    }
}

/// Coverage (0 to 1) of the given parts, per pixel.
fn coverage(parts: &[Part]) -> Vec<f32> {
    let n = (SAMPLES * SAMPLES) as f32;
    let mut out = vec![0.0; (WIFI_W * WIFI_H) as usize];
    for py in 0..WIFI_H {
        for px in 0..WIFI_W {
            let mut hit = 0u32;
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let x = px as f32 + (sx as f32 + 0.5) / SAMPLES as f32;
                    let y = py as f32 + (sy as f32 + 0.5) / SAMPLES as f32;
                    if parts.iter().any(|&p| inside(p, x, y)) {
                        hit += 1;
                    }
                }
            }
            out[(py * WIFI_W + px) as usize] = hit as f32 / n;
        }
    }
    out
}

/// Lays `colour` at `alpha` (0 to 1) over the pixel (straight alpha, as the text faces).
fn over(px: &mut [u8], alpha: f32, colour: [u8; 3]) {
    let a = (alpha * 255.0).round() as u32;
    if a == 0 {
        return;
    }
    let under = px[3] as u32 * (255 - a) / 255;
    let out = a + under;
    for c in 0..3 {
        px[c] = ((colour[c] as u32 * a + px[c] as u32 * under + out / 2) / out) as u8;
    }
    px[3] = out as u8;
}

/// The icon's texture for a state.
pub fn wifi_icon_face(icon: WifiIcon) -> CartFace {
    let all = [Part::Dot, Part::Arc(0), Part::Arc(1), Part::Arc(2)];
    let layers: Vec<(Vec<Part>, [u8; 3], f32)> = match (icon, icon.lit()) {
        (_, Some(n)) => vec![
            ((n..3).map(Part::Arc).collect(), GREY, FAINT),
            (
                std::iter::once(Part::Dot)
                    .chain((0..n).map(Part::Arc))
                    .collect(),
                INK,
                1.0,
            ),
        ],
        (WifiIcon::NotConnected, None) => vec![(all.to_vec(), GREY, FAINT)],
        _ => vec![(all.to_vec(), GREY, 1.0)],
    };
    let mut rgba = vec![0u8; (WIFI_W * WIFI_H * 4) as usize];
    for (parts, colour, strength) in layers {
        if parts.is_empty() {
            continue;
        }
        for (i, c) in coverage(&parts).into_iter().enumerate() {
            over(&mut rgba[i * 4..i * 4 + 4], c * strength, colour);
        }
    }
    CartFace {
        rgba,
        w: WIFI_W,
        h: WIFI_H,
    }
}

/// Where the icon is drawn for a clock face drawn by `draw_footer`.
pub fn wifi_icon_xy(clock: Printed) -> (f32, f32) {
    let clock_x = slot_gfx::OUT_W as f32 - crate::footer::FOOTER_MARGIN - clock.w as f32;
    (
        clock_x + CLOCK_INK_INSET - GAP - WIFI_W as f32,
        FOOTER_Y + TOP,
    )
}

/// Draws the icon (the texture for the state to show) left of the clock. Nothing for
/// Wi-Fi Off (`None`).
pub fn draw_wifi_icon(clock: Printed, icon: Option<TexId>, out: &mut Vec<Draw>) {
    let Some(tex) = icon else {
        return;
    };
    let (x, y) = wifi_icon_xy(clock);
    out.push(Draw::Tex {
        x,
        y,
        w: WIFI_W as f32,
        h: WIFI_H as f32,
        tex,
        alpha: 1.0,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alpha(f: &CartFace, x: u32, y: u32) -> u8 {
        f.rgba[((y * f.w + x) * 4 + 3) as usize]
    }

    fn bright(f: &CartFace) -> usize {
        f.rgba
            .chunks(4)
            .filter(|p| p[3] > 128 && p[0] > 0xc0)
            .count()
    }

    #[test]
    fn the_outer_arc_is_crisp_on_the_top_row_and_never_clipped() {
        let f = wifi_icon_face(WifiIcon::Strong);
        // Its top row is solid at the centre; nothing is cut off above.
        assert!(
            alpha(&f, WIFI_W / 2, 0) > 200,
            "{}",
            alpha(&f, WIFI_W / 2, 0)
        );
        const { assert!(RADII[2] + STROKE / 2.0 <= DOT_CY + 0.001) };
        // The dot sits on the bottom row, the clock's baseline.
        assert!(alpha(&f, WIFI_W / 2, WIFI_H - 1) > 128);
    }

    #[test]
    fn each_level_lights_more_and_unknown_and_not_connected_light_nothing() {
        let lit: Vec<usize> = WifiIcon::ALL
            .iter()
            .map(|&i| bright(&wifi_icon_face(i)))
            .collect();
        let [weak, moderate, strong, not_connected, unknown] = lit[..] else {
            unreachable!()
        };
        assert!(weak > 0 && weak < moderate && moderate < strong, "{lit:?}");
        assert_eq!(not_connected, 0, "no white when not connected");
        assert_eq!(unknown, 0, "no white when unknown");
    }

    #[test]
    fn unused_arcs_stay_visible_faintly_so_the_shape_never_changes() {
        // The outer arc's top centre: white when lit, faint otherwise, never empty.
        let top = |i: WifiIcon| {
            let f = wifi_icon_face(i);
            let at = ((WIFI_W / 2) * 4) as usize;
            (f.rgba[at], f.rgba[at + 3])
        };
        assert!(top(WifiIcon::Strong).1 > 200 && top(WifiIcon::Strong).0 > 0xc0);
        for i in [WifiIcon::Weak, WifiIcon::Moderate, WifiIcon::NotConnected] {
            let (c, a) = top(i);
            assert!((60..110).contains(&a) && c < 0xc0, "{i:?}: {c} {a}");
        }
        assert!(top(WifiIcon::Unknown).1 > 200 && top(WifiIcon::Unknown).0 == 0x9a);
    }

    #[test]
    fn not_connected_is_fainter_than_unknown() {
        let peak = |i: WifiIcon| wifi_icon_face(i).rgba.chunks(4).map(|p| p[3]).max();
        let (nc, un) = (
            peak(WifiIcon::NotConnected).unwrap(),
            peak(WifiIcon::Unknown).unwrap(),
        );
        assert!(nc < 110 && un > 240, "{nc} {un}");
    }

    #[test]
    fn it_sits_9_px_left_of_the_clock_on_its_baseline() {
        let clock = Printed { face: None, w: 50 };
        // The clock's face starts at x 646 and its first digit at 647; ink rows 445 to 456.
        assert_eq!(wifi_icon_xy(clock), (621.0, 444.0));
        assert_eq!(621 + WIFI_W, 638);
    }
}
