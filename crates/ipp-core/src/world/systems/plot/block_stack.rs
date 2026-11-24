//! Shared layer sizing and stable per-row variation for planar and spatial bars.

use super::PlotRowIdentity;

/// A remainder shorter than one plank thickens the last full block.
pub(super) fn fitted_count(height: f64, edge: f64) -> f64 {
    let units = height / edge;
    (units.floor() + f64::from(units.fract() >= 1.0 / 6.0)).max(1.0)
}

/// Zero-based layer. The golden-ratio step separates neighbours without accumulating drift.
pub(super) fn variation(row: PlotRowIdentity, layer: u64) -> f32 {
    let key = row.row_id.0 ^ (u64::from(row.series) << 32);
    let mixed = (key ^ (key >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    let seed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    let phase = seed.wrapping_add(layer.wrapping_add(1).wrapping_mul(0x9e37_79b9_7f4a_7c15));
    (phase >> 48) as f32 / 32768.0 - 1.0
}
