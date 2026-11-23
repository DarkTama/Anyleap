//! Phase 1 scroll speed multiplier, density parsing, and per-axis remainder accumulator.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Timeout after which any fractional remainder in an axis accumulator is discarded.
pub const ACCUMULATOR_RESET_AFTER: Duration = Duration::from_millis(250);

/// Minimum and maximum Auto multiplier bounds (SPEC-30 §5.1).
const AUTO_MIN_MULTIPLIER: f32 = 1.0;
const AUTO_MAX_MULTIPLIER: f32 = 4.0;

/// Minimum and maximum Manual multiplier bounds (SPEC-30 §5.1).
const MANUAL_MIN_MULTIPLIER: f32 = 1.0;
const MANUAL_MAX_MULTIPLIER: f32 = 5.0;

/// Fallback native display density when `wm density` and `dumpsys display` fail.
pub const DEFAULT_NATIVE_DENSITY: u32 = 400;

/// How mouse-wheel events over a mirror window are handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScrollStyle {
    #[default]
    Smooth,
    Paged,
}

/// Whether scroll speed is computed from display density or manually fixed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SpeedMode {
    #[default]
    Auto,
    Manual,
}

/// Touchpad momentum strength (Phase 3; persisted and accepted on IPC).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InertiaLevel {
    #[default]
    Off,
    Low,
    Medium,
    High,
}

/// Per-session scroll-feel configuration sent by the frontend via `set_scroll_feel`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScrollConfig {
    #[serde(default)]
    pub style: ScrollStyle,
    #[serde(default, alias = "speed_mode")]
    pub speed_mode: SpeedMode,
    #[serde(default = "default_manual_speed")]
    pub manual: f32,
    #[serde(default)]
    pub smooth: bool,
    #[serde(default)]
    pub inertia: InertiaLevel,
}

fn default_manual_speed() -> f32 {
    1.0
}

impl Default for ScrollConfig {
    fn default() -> Self {
        Self {
            style: ScrollStyle::Smooth,
            speed_mode: SpeedMode::Auto,
            manual: 1.0,
            smooth: false,
            inertia: InertiaLevel::Off,
        }
    }
}

/// Round a positive float to the nearest 0.25 step.
fn round_quarter(v: f32) -> f32 {
    (v * 4.0).round() / 4.0
}

/// Compute the Auto scroll speed multiplier from physical and session densities.
///
/// Missing or zero densities fall back to `1.0`. The ratio `native / session`
/// is clamped to `[1.0, 4.0]` and rounded to the nearest `0.25`.
pub fn auto_multiplier(native_density: Option<u32>, session_density: Option<u32>) -> f32 {
    let (Some(native), Some(session)) = (native_density, session_density) else {
        return 1.0;
    };
    if native == 0 || session == 0 {
        return 1.0;
    }
    let ratio = (native as f32 / session as f32).clamp(AUTO_MIN_MULTIPLIER, AUTO_MAX_MULTIPLIER);
    round_quarter(ratio)
}

/// Compute the effective scroll multiplier for a `ScrollConfig` and cached densities.
pub fn effective_multiplier(
    config: &ScrollConfig,
    native_density: Option<u32>,
    session_density: Option<u32>,
) -> f32 {
    match config.speed_mode {
        SpeedMode::Auto => auto_multiplier(native_density, session_density),
        SpeedMode::Manual => {
            if config.manual.is_finite() {
                config
                    .manual
                    .clamp(MANUAL_MIN_MULTIPLIER, MANUAL_MAX_MULTIPLIER)
            } else {
                1.0
            }
        }
    }
}

/// Parsed output of `adb shell wm density`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WmDensity {
    pub physical: Option<u32>,
    pub override_: Option<u32>,
}

impl WmDensity {
    /// Active density for a physical Mirror session: override wins over physical.
    pub fn mirror_density(self) -> Option<u32> {
        self.override_.or(self.physical)
    }
}

/// Parse `adb shell wm density` output into physical and optional override densities.
pub fn parse_wm_density(text: &str) -> WmDensity {
    let mut out = WmDensity::default();
    for line in text.lines() {
        let Some((label, value)) = line.split_once(':') else {
            continue;
        };
        let Ok(dpi) = value.trim().parse::<u32>() else {
            continue;
        };
        if dpi == 0 {
            continue;
        }
        if label.contains("Override") {
            out.override_ = Some(dpi);
        } else if label.contains("Physical") {
            out.physical = Some(dpi);
        }
    }
    out
}

/// Extract density dpi from a `DisplayInfo{...}` or `DisplayDeviceInfo{...}` line.
///
/// Android prints `density 160` in modern `DisplayInfo` dumps (observed on Android 16)
/// and `densityDpi=160` / `densityDpi: 160` in some dumpsys variants.
fn extract_density_from_line(line: &str) -> Option<u32> {
    for marker in [
        "densityDpi=",
        "densityDpi:",
        "densityDpi ",
        ", density ",
        " density ",
    ] {
        if let Some(rest) = line.split(marker).nth(1) {
            let digits: String = rest
                .trim_start()
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            if let Ok(dpi) = digits.parse::<u32>() {
                if dpi > 0 {
                    return Some(dpi);
                }
            }
        }
    }
    None
}

/// Parse the scrcpy virtual display density from `adb shell dumpsys display`.
pub fn parse_virtual_display_density(dumpsys: &str) -> Option<u32> {
    parse_virtual_display_density_for_id(dumpsys, None)
}

/// Parse the scrcpy virtual display density, preferring a specific `display_id` if known.
pub fn parse_virtual_display_density_for_id(dumpsys: &str, display_id: Option<u32>) -> Option<u32> {
    let mut fallback_override = None;
    let mut fallback_any = None;

    for line in dumpsys.lines() {
        if !line.contains("DisplayInfo{\"scrcpy\"") {
            continue;
        }
        let Some(dpi) = extract_density_from_line(line) else {
            continue;
        };
        if let Some(target_id) = display_id {
            let needle = format!("displayId {target_id},");
            if line.contains(&needle) {
                if line.contains("mOverrideDisplayInfo") {
                    return Some(dpi);
                }
                fallback_override = Some(dpi);
            }
        } else if line.contains("mOverrideDisplayInfo") && fallback_override.is_none() {
            fallback_override = Some(dpi);
        }
        if fallback_any.is_none() {
            fallback_any = Some(dpi);
        }
    }

    fallback_override.or(fallback_any)
}

/// Parse the default display (`displayId 0`) density from `adb shell dumpsys display`.
pub fn parse_default_display_density(dumpsys: &str) -> Option<u32> {
    let mut fallback = None;
    for line in dumpsys.lines() {
        if !line.contains("DisplayInfo{") || !line.contains("displayId 0,") {
            continue;
        }
        if let Some(dpi) = extract_density_from_line(line) {
            if line.contains("mOverrideDisplayInfo") {
                return Some(dpi);
            }
            if fallback.is_none() {
                fallback = Some(dpi);
            }
        }
    }
    fallback
}

/// Split a signed integer delta into `i16`-bounded chunks preserving exact sum.
pub fn split_i16_deltas(mut total: i64) -> Vec<i16> {
    let mut out = Vec::new();
    while total > 0 {
        let chunk = total.min(i16::MAX as i64) as i16;
        out.push(chunk);
        total -= chunk as i64;
    }
    while total < 0 {
        let chunk = total.max(i16::MIN as i64) as i16;
        out.push(chunk);
        total -= chunk as i64;
    }
    out
}

/// Per-axis sub-unit remainder accumulator for scaled mouse-wheel deltas.
#[derive(Debug, Default)]
pub struct AxisAccumulator {
    remainder: f32,
    last_sign: i8,
    last_at: Option<Instant>,
}

impl AxisAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Clear carried remainder (called on idle timeout or direction reversal).
    pub fn reset(&mut self) {
        self.remainder = 0.0;
        self.last_sign = 0;
        self.last_at = None;
    }

    /// Scale `delta` by `multiplier`, carry fractional remainder, and split into `i16` chunks.
    pub fn push(&mut self, delta: i16, multiplier: f32, now: Instant) -> Vec<i16> {
        if delta == 0 || !multiplier.is_finite() {
            return Vec::new();
        }
        let sign: i8 = if delta > 0 { 1 } else { -1 };
        let timed_out = self
            .last_at
            .map(|prev| now.saturating_duration_since(prev) >= ACCUMULATOR_RESET_AFTER)
            .unwrap_or(false);
        let reversed = self.last_sign != 0 && self.last_sign != sign;
        if timed_out || reversed {
            self.remainder = 0.0;
        }

        self.last_sign = sign;
        self.last_at = Some(now);

        let total = (delta as f32) * multiplier + self.remainder;
        let emit = total.trunc();
        self.remainder = total - emit;
        split_i16_deltas(emit as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_multiplier_cases_and_clamps() {
        assert_eq!(auto_multiplier(Some(400), Some(400)), 1.0);
        assert_eq!(auto_multiplier(Some(400), Some(160)), 2.5);
        assert_eq!(auto_multiplier(Some(400), Some(320)), 1.25);
        assert_eq!(auto_multiplier(Some(480), Some(160)), 3.0);
        assert_eq!(auto_multiplier(Some(400), Some(100)), 4.0);
        assert_eq!(auto_multiplier(Some(300), Some(400)), 1.0);
        assert_eq!(auto_multiplier(None, Some(160)), 1.0);
        assert_eq!(auto_multiplier(Some(400), None), 1.0);
        assert_eq!(auto_multiplier(Some(0), Some(160)), 1.0);
        assert_eq!(auto_multiplier(Some(400), Some(0)), 1.0);
    }

    #[test]
    fn auto_multiplier_rounds_to_quarter() {
        // 440 / 160 = 2.75 exact
        assert_eq!(auto_multiplier(Some(440), Some(160)), 2.75);
        // 440 / 240 = 1.8333... -> 1.75
        assert_eq!(auto_multiplier(Some(440), Some(240)), 1.75);
        // 440 / 320 = 1.375 -> rounds to 1.5
        assert_eq!(auto_multiplier(Some(440), Some(320)), 1.5);
    }

    #[test]
    fn effective_multiplier_manual_clamp() {
        let mut cfg = ScrollConfig {
            speed_mode: SpeedMode::Manual,
            manual: 2.25,
            ..ScrollConfig::default()
        };
        assert_eq!(effective_multiplier(&cfg, Some(400), Some(160)), 2.25);

        cfg.manual = 0.25;
        assert_eq!(effective_multiplier(&cfg, Some(400), Some(160)), 1.0);

        cfg.manual = 9.0;
        assert_eq!(effective_multiplier(&cfg, Some(400), Some(160)), 5.0);

        cfg.manual = f32::NAN;
        assert_eq!(effective_multiplier(&cfg, Some(400), Some(160)), 1.0);
    }

    #[test]
    fn wm_density_parsing() {
        assert_eq!(
            parse_wm_density("Physical density: 440\n"),
            WmDensity {
                physical: Some(440),
                override_: None,
            }
        );
        assert_eq!(
            parse_wm_density("Physical density: 440\n").mirror_density(),
            Some(440)
        );

        let with_override = parse_wm_density("Physical density: 440\nOverride density: 320\n");
        assert_eq!(
            with_override,
            WmDensity {
                physical: Some(440),
                override_: Some(320),
            }
        );
        assert_eq!(with_override.mirror_density(), Some(320));

        assert_eq!(parse_wm_density(""), WmDensity::default());
        assert_eq!(
            parse_wm_density("Physical density: 0\n"),
            WmDensity::default()
        );
        assert_eq!(parse_wm_density("garbage"), WmDensity::default());
    }

    #[test]
    fn dumpsys_virtual_and_default_display_density() {
        let real_dump = r#"
    mBaseDisplayInfo=DisplayInfo{"Built-in Screen", displayId 0, displayGroupId 0, real 1080 x 2436, app 1080 x 2436, density 440, physicalDensity 440 (392.78 x 392.82) dpi, layerStack 0}
    mOverrideDisplayInfo=DisplayInfo{"Built-in Screen", displayId 0, displayGroupId 0, real 1080 x 2436, app 1080 x 2436, density 440, physicalDensity 440 (392.78 x 392.82) dpi, layerStack 0}
    mBaseDisplayInfo=DisplayInfo{"scrcpy", displayId 79, displayGroupId 1, real 1020 x 1836, app 1020 x 1836, density 320, physicalDensity 320 (320.0 x 320.0) dpi, layerStack 79}
    mOverrideDisplayInfo=DisplayInfo{"scrcpy", displayId 80, displayGroupId 1, real 1920 x 1080, app 1920 x 1080, density 160, physicalDensity 160 (160.0 x 160.0) dpi, layerStack 80}
"#;
        assert_eq!(parse_default_display_density(real_dump), Some(440));
        assert_eq!(
            parse_virtual_display_density_for_id(real_dump, Some(80)),
            Some(160)
        );
        assert_eq!(
            parse_virtual_display_density_for_id(real_dump, Some(79)),
            Some(320)
        );

        let density_dpi_dump = r#"    mOverrideDisplayInfo=DisplayInfo{"scrcpy", displayId 5, real 1920 x 1080, densityDpi=240, type VIRTUAL}"#;
        assert_eq!(parse_virtual_display_density(density_dpi_dump), Some(240));
        assert_eq!(parse_virtual_display_density("no virtual display"), None);
    }

    #[test]
    fn accumulator_carries_remainder_and_matches_sum() {
        let mut acc = AxisAccumulator::new();
        let t0 = Instant::now();
        let mut emitted = Vec::new();
        for (i, d) in [3i16, 7, 12].into_iter().enumerate() {
            let t = t0 + Duration::from_millis((i as u64) * 16);
            emitted.extend(acc.push(d, 2.5, t));
        }
        assert_eq!(emitted, vec![7, 18, 30]);
        let sum: i64 = emitted.iter().map(|&v| v as i64).sum();
        assert_eq!(sum, 55);

        // Repeated 3 at 2.5x: 7.5 -> 7, 7.5+0.5=8 -> 8, 7.5 -> 7 (total 22 vs 22.5, within 1)
        let mut acc2 = AxisAccumulator::new();
        let mut sum2 = 0i64;
        for i in 0..3 {
            let out = acc2.push(3, 2.5, t0 + Duration::from_millis(i * 10));
            sum2 += out.iter().map(|&v| v as i64).sum::<i64>();
        }
        assert!(((sum2 as f32) - (9.0 * 2.5)).abs() <= 1.0);
    }

    #[test]
    fn accumulator_resets_after_silence_and_direction_change() {
        let mut acc = AxisAccumulator::new();
        let t0 = Instant::now();
        // 3 * 2.5 = 7.5 -> emits 7, carries 0.5
        assert_eq!(acc.push(3, 2.5, t0), vec![7]);
        // After 250 ms silence, remainder resets -> 3 * 2.5 = 7.5 -> emits 7 (not 8)
        assert_eq!(acc.push(3, 2.5, t0 + Duration::from_millis(250)), vec![7]);
        // Direction reversal within 10 ms resets carried 0.5 -> -3 * 2.5 = -7.5 -> emits -7
        assert_eq!(acc.push(-3, 2.5, t0 + Duration::from_millis(260)), vec![-7]);
    }

    #[test]
    fn accumulator_splits_i16_overflow() {
        let mut acc = AxisAccumulator::new();
        let t0 = Instant::now();
        let pos = acc.push(10_000, 4.0, t0);
        assert_eq!(pos, vec![32767, 7233]);
        assert_eq!(pos.iter().map(|&v| v as i64).sum::<i64>(), 40_000);

        let neg = acc.push(-10_000, 4.0, t0 + Duration::from_millis(10));
        assert_eq!(neg, vec![-32768, -7232]);
        assert_eq!(neg.iter().map(|&v| v as i64).sum::<i64>(), -40_000);
    }
}
