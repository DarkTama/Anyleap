//! Pure, platform-independent scroll-feel math and configuration types (SPEC-30).

pub mod smooth;
pub mod speed;

pub use smooth::AxisSmoother;
pub use speed::{
    auto_multiplier, classify_wheel_source, effective_multiplier, effective_touchpad_speed,
    parse_default_display_density, parse_virtual_display_density,
    parse_virtual_display_density_for_id, parse_wm_density, AxisAccumulator, InertiaLevel,
    ScrollConfig, ScrollStyle, SpeedMode, WheelSource, WmDensity, LLMHF_INJECTED,
};
