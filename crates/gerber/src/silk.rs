//! A stroke font for silkscreen text: digits, capitals and a few marks, each a set of polylines on
//! a 4-wide x 6-tall grid (plus 1 of advance). Legible at 1 mm, which is what a reference
//! designator on a small board gets. Original work; no font file is embedded.

use board_geom::Point;
use board_units::Nm;

type Stroke = &'static [(i8, i8)];

/// Glyph strokes on the grid: x in 0..=4, y in 0..=6 (y up). Unknown characters draw a box.
fn glyph(c: char) -> &'static [Stroke] {
    match c.to_ascii_uppercase() {
        '0' => &[&[(0, 0), (4, 0), (4, 6), (0, 6), (0, 0)], &[(0, 0), (4, 6)]],
        '1' => &[&[(1, 5), (2, 6), (2, 0)], &[(0, 0), (4, 0)]],
        '2' => &[&[(0, 5), (1, 6), (3, 6), (4, 5), (4, 4), (0, 0), (4, 0)]],
        '3' => &[&[(0, 6), (4, 6), (2, 3), (4, 2), (4, 1), (3, 0), (1, 0), (0, 1)]],
        '4' => &[&[(3, 0), (3, 6), (0, 2), (4, 2)]],
        '5' => &[&[(4, 6), (0, 6), (0, 3), (3, 3), (4, 2), (4, 1), (3, 0), (0, 0)]],
        '6' => &[&[(4, 6), (1, 6), (0, 5), (0, 1), (1, 0), (3, 0), (4, 1), (4, 2), (3, 3), (0, 3)]],
        '7' => &[&[(0, 6), (4, 6), (1, 0)]],
        '8' => &[&[(1, 3), (0, 4), (0, 5), (1, 6), (3, 6), (4, 5), (4, 4), (3, 3), (1, 3), (0, 2), (0, 1), (1, 0), (3, 0), (4, 1), (4, 2), (3, 3)]],
        '9' => &[&[(0, 0), (3, 0), (4, 1), (4, 5), (3, 6), (1, 6), (0, 5), (0, 4), (1, 3), (4, 3)]],
        'A' => &[&[(0, 0), (0, 4), (2, 6), (4, 4), (4, 0)], &[(0, 2), (4, 2)]],
        'B' => &[&[(0, 0), (0, 6), (3, 6), (4, 5), (4, 4), (3, 3), (0, 3)], &[(3, 3), (4, 2), (4, 1), (3, 0), (0, 0)]],
        'C' => &[&[(4, 5), (3, 6), (1, 6), (0, 5), (0, 1), (1, 0), (3, 0), (4, 1)]],
        'D' => &[&[(0, 0), (0, 6), (2, 6), (4, 4), (4, 2), (2, 0), (0, 0)]],
        'E' => &[&[(4, 6), (0, 6), (0, 0), (4, 0)], &[(0, 3), (3, 3)]],
        'F' => &[&[(4, 6), (0, 6), (0, 0)], &[(0, 3), (3, 3)]],
        'G' => &[&[(4, 5), (3, 6), (1, 6), (0, 5), (0, 1), (1, 0), (3, 0), (4, 1), (4, 3), (2, 3)]],
        'H' => &[&[(0, 0), (0, 6)], &[(4, 0), (4, 6)], &[(0, 3), (4, 3)]],
        'I' => &[&[(1, 6), (3, 6)], &[(2, 6), (2, 0)], &[(1, 0), (3, 0)]],
        'J' => &[&[(1, 6), (4, 6), (4, 1), (3, 0), (1, 0), (0, 1)]],
        'K' => &[&[(0, 0), (0, 6)], &[(4, 6), (0, 2)], &[(1, 3), (4, 0)]],
        'L' => &[&[(0, 6), (0, 0), (4, 0)]],
        'M' => &[&[(0, 0), (0, 6), (2, 3), (4, 6), (4, 0)]],
        'N' => &[&[(0, 0), (0, 6), (4, 0), (4, 6)]],
        'O' => &[&[(1, 0), (0, 1), (0, 5), (1, 6), (3, 6), (4, 5), (4, 1), (3, 0), (1, 0)]],
        'P' => &[&[(0, 0), (0, 6), (3, 6), (4, 5), (4, 4), (3, 3), (0, 3)]],
        'Q' => &[&[(1, 0), (0, 1), (0, 5), (1, 6), (3, 6), (4, 5), (4, 1), (3, 0), (1, 0)], &[(2, 2), (4, 0)]],
        'R' => &[&[(0, 0), (0, 6), (3, 6), (4, 5), (4, 4), (3, 3), (0, 3)], &[(2, 3), (4, 0)]],
        'S' => &[&[(4, 5), (3, 6), (1, 6), (0, 5), (0, 4), (1, 3), (3, 3), (4, 2), (4, 1), (3, 0), (1, 0), (0, 1)]],
        'T' => &[&[(0, 6), (4, 6)], &[(2, 6), (2, 0)]],
        'U' => &[&[(0, 6), (0, 1), (1, 0), (3, 0), (4, 1), (4, 6)]],
        'V' => &[&[(0, 6), (2, 0), (4, 6)]],
        'W' => &[&[(0, 6), (1, 0), (2, 4), (3, 0), (4, 6)]],
        'X' => &[&[(0, 0), (4, 6)], &[(0, 6), (4, 0)]],
        'Y' => &[&[(0, 6), (2, 3), (4, 6)], &[(2, 3), (2, 0)]],
        'Z' => &[&[(0, 6), (4, 6), (0, 0), (4, 0)]],
        '-' => &[&[(0, 3), (4, 3)]],
        '+' => &[&[(0, 3), (4, 3)], &[(2, 1), (2, 5)]],
        '.' => &[&[(2, 0), (2, 1)]],
        '/' => &[&[(0, 0), (4, 6)]],
        ' ' => &[],
        _ => &[&[(0, 0), (4, 0), (4, 6), (0, 6), (0, 0)]],
    }
}

/// Advance per character in grid units (4 wide + 1 gap).
const ADVANCE: i64 = 5;

/// Polylines for `text` centred on `at`, cap height `height`; `mirror` flips x for the back side.
pub fn text_strokes(text: &str, at: Point, height: Nm, mirror: bool) -> Vec<Vec<Point>> {
    let unit = height / 6;
    let n = text.chars().count() as i64;
    if n == 0 {
        return Vec::new();
    }
    let total_w = unit * (ADVANCE * n - 1);
    let left = at.x - total_w / 2;
    let bottom = at.y - height / 2;
    let mut out = Vec::new();
    for (i, c) in text.chars().enumerate() {
        let cx = left + unit * (ADVANCE * i as i64);
        for stroke in glyph(c) {
            let pts: Vec<Point> = stroke
                .iter()
                .map(|(gx, gy)| {
                    let x = cx + unit * (*gx as i64);
                    let x = if mirror { at.x * 2 - x } else { x };
                    Point::new(x, bottom + unit * (*gy as i64))
                })
                .collect();
            out.push(pts);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_printable_glyph_is_defined_and_text_is_centred() {
        for c in "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ-+./".chars() {
            assert!(!glyph(c).is_empty(), "{c}");
        }
        let s = text_strokes("U1", Point::mm(10, 10), Nm::mm(1), false);
        let xs: Vec<i64> = s.iter().flatten().map(|p| p.x.0).collect();
        let (min, max) = (*xs.iter().min().unwrap(), *xs.iter().max().unwrap());
        // Two glyphs: 9 units wide at 1/6 mm per unit, centred on x = 10 mm.
        assert_eq!(min + max, 2 * Nm::mm(10).0);
        assert_eq!(max - min, Nm::mm(1).0 / 6 * 9);
        let m = text_strokes("U1", Point::mm(10, 10), Nm::mm(1), true);
        let xm: Vec<i64> = m.iter().flatten().map(|p| p.x.0).collect();
        assert_eq!(xm.iter().min(), xs.iter().min());
        assert!(text_strokes("", Point::ORIGIN, Nm::mm(1), false).is_empty());
    }
}
