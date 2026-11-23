//! Pure, platform-independent scroll-feel math and configuration types (SPEC-30).

pub mod smooth;
pub mod speed;

pub use smooth::AxisSmoother;
pub use speed::{
    auto_multiplier, effective_multiplier, parse_default_display_density,
    parse_virtual_display_density, parse_virtual_display_density_for_id, parse_wm_density,
    AxisAccumulator, InertiaLevel, ScrollConfig, ScrollStyle, SpeedMode, WmDensity,
};
