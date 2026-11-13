use super::monitors::MonitorDisplay;
use crate::kvm::protocol::ScreenRect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Edge {
    Right = 0,
    Left = 1,
    Top = 2,
    Bottom = 3,
}

impl Edge {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "right" => Some(Self::Right),
            "left" => Some(Self::Left),
            "top" => Some(Self::Top),
            "bottom" => Some(Self::Bottom),
            _ => None,
        }
    }

    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Right,
            1 => Self::Left,
            2 => Self::Top,
            3 => Self::Bottom,
            _ => Self::Right,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Right => "right",
            Self::Left => "left",
            Self::Top => "top",
            Self::Bottom => "bottom",
        }
    }

    pub fn opposite(self) -> Self {
        match self {
            Self::Right => Self::Left,
            Self::Left => Self::Right,
            Self::Top => Self::Bottom,
            Self::Bottom => Self::Top,
        }
    }
}

pub fn union_rect(monitors: &[MonitorDisplay]) -> ScreenRect {
    if monitors.is_empty() {
        return ScreenRect {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        };
    }
    let mut min_x = i32::MAX;
    let mut min_y = i32::MAX;
    let mut max_x = i32::MIN;
    let mut max_y = i32::MIN;
    for m in monitors {
        min_x = min_x.min(m.x);
        min_y = min_y.min(m.y);
        max_x = max_x.max(m.x + m.width);
        max_y = max_y.max(m.y + m.height);
    }
    ScreenRect {
        x: min_x,
        y: min_y,
        width: (max_x - min_x).max(0),
        height: (max_y - min_y).max(0),
    }
}

pub fn normalize_in_union(pos: (i32, i32), union: &ScreenRect) -> (f32, f32) {
    let w = union.width.max(1) as f32;
    let h = union.height.max(1) as f32;
    (
        ((pos.0 - union.x) as f32 / w).clamp(0.0, 1.0),
        ((pos.1 - union.y) as f32 / h).clamp(0.0, 1.0),
    )
}

pub fn monitor_at_point(monitors: &[MonitorDisplay], x: i32, y: i32) -> Option<(usize, &MonitorDisplay)> {
    monitors
        .iter()
        .enumerate()
        .find(|(_, m)| m.width > 0 && m.height > 0 && x >= m.x && x < m.x + m.width && y >= m.y && y < m.y + m.height)
}

pub fn nearest_monitor(monitors: &[MonitorDisplay], x: i32, y: i32) -> Option<(usize, &MonitorDisplay)> {
    let mut best: Option<(i64, usize, &MonitorDisplay)> = None;
    for (idx, m) in monitors.iter().enumerate() {
        if m.width <= 0 || m.height <= 0 {
            continue;
        }
        let cx = x.clamp(m.x, m.x + m.width - 1);
        let cy = y.clamp(m.y, m.y + m.height - 1);
        let dx = x as i64 - cx as i64;
        let dy = y as i64 - cy as i64;
        let dist2 = dx * dx + dy * dy;
        if best.is_none_or(|(bdist, _, _)| dist2 < bdist) {
            best = Some((dist2, idx, m));
        }
    }
    best.map(|(_, idx, m)| (idx, m))
}

/// Pure controller-side per-monitor edge detection.
/// Triggers when `pt` is within 2 px of `link_edge` of its containing monitor,
/// there is no local monitor immediately beyond that edge at `pt`'s perpendicular
/// coordinate, and `pt` is outside the corner deadzone of that monitor.
pub fn detect_controller_edge(
    pt: (i32, i32),
    monitors: &[MonitorDisplay],
    link_edge: Edge,
    deadzone_px: i32,
) -> Option<(usize, f32)> {
    let (idx, mon) = monitor_at_point(monitors, pt.0, pt.1)?;
    let right = mon.x + mon.width;
    let bottom = mon.y + mon.height;

    let (at_edge, neighbor) = match link_edge {
        Edge::Right => (pt.0 >= right - 2, (right, pt.1)),
        Edge::Left => (pt.0 < mon.x + 2, (mon.x - 1, pt.1)),
        Edge::Top => (pt.1 < mon.y + 2, (pt.0, mon.y - 1)),
        Edge::Bottom => (pt.1 >= bottom - 2, (pt.0, bottom)),
    };
    if !at_edge {
        return None;
    }

    if monitor_at_point(monitors, neighbor.0, neighbor.1).is_some() {
        return None;
    }

    let rel = match link_edge {
        Edge::Left | Edge::Right => {
            let dz = deadzone_px.max(0).min((mon.height - 1).max(0) / 2);
            if pt.1 < mon.y + dz || pt.1 >= bottom - dz {
                return None;
            }
            ((pt.1 - mon.y) as f32 / mon.height.max(1) as f32).clamp(0.0, 1.0)
        }
        Edge::Top | Edge::Bottom => {
            let dz = deadzone_px.max(0).min((mon.width - 1).max(0) / 2);
            if pt.0 < mon.x + dz || pt.0 >= right - dz {
                return None;
            }
            ((pt.0 - mon.x) as f32 / mon.width.max(1) as f32).clamp(0.0, 1.0)
        }
    };

    Some((idx, rel))
}

/// Maps `CursorEnter { entry_edge, relative_y }` onto a receiver coordinate 2 px
/// inside the chosen monitor's `entry_edge`.
pub fn map_cursor_enter(monitors: &[MonitorDisplay], entry_edge: Edge, relative_y: f32) -> (i32, i32) {
    let rel = relative_y.clamp(0.0, 1.0);
    let valid: Vec<&MonitorDisplay> = monitors.iter().filter(|m| m.width > 0 && m.height > 0).collect();
    if valid.is_empty() {
        return (2, 2);
    }
    let union = union_rect(monitors);

    let chosen = match entry_edge {
        Edge::Left | Edge::Right => {
            let mapped_y = union.y + (rel * (union.height.max(1) - 1) as f32).round() as i32;
            let containing: Vec<&MonitorDisplay> = valid
                .iter()
                .copied()
                .filter(|m| mapped_y >= m.y && mapped_y < m.y + m.height)
                .collect();
            let pool = if containing.is_empty() {
                // Nearest vertical span if mapped_y falls in a vertical gap
                let min_dist = valid
                    .iter()
                    .map(|m| {
                        let cy = mapped_y.clamp(m.y, m.y + m.height - 1);
                        (mapped_y - cy).abs()
                    })
                    .min()
                    .unwrap_or(0);
                valid
                    .iter()
                    .copied()
                    .filter(|m| {
                        let cy = mapped_y.clamp(m.y, m.y + m.height - 1);
                        (mapped_y - cy).abs() == min_dist
                    })
                    .collect()
            } else {
                containing
            };
            if entry_edge == Edge::Left {
                pool.into_iter().min_by_key(|m| m.x).unwrap_or(valid[0])
            } else {
                pool.into_iter().max_by_key(|m| m.x + m.width).unwrap_or(valid[0])
            }
        }
        Edge::Top | Edge::Bottom => {
            let mapped_x = union.x + (rel * (union.width.max(1) - 1) as f32).round() as i32;
            let containing: Vec<&MonitorDisplay> = valid
                .iter()
                .copied()
                .filter(|m| mapped_x >= m.x && mapped_x < m.x + m.width)
                .collect();
            let pool = if containing.is_empty() {
                let min_dist = valid
                    .iter()
                    .map(|m| {
                        let cx = mapped_x.clamp(m.x, m.x + m.width - 1);
                        (mapped_x - cx).abs()
                    })
                    .min()
                    .unwrap_or(0);
                valid
                    .iter()
                    .copied()
                    .filter(|m| {
                        let cx = mapped_x.clamp(m.x, m.x + m.width - 1);
                        (mapped_x - cx).abs() == min_dist
                    })
                    .collect()
            } else {
                containing
            };
            if entry_edge == Edge::Top {
                pool.into_iter().min_by_key(|m| m.y).unwrap_or(valid[0])
            } else {
                pool.into_iter().max_by_key(|m| m.y + m.height).unwrap_or(valid[0])
            }
        }
    };

    match entry_edge {
        Edge::Left => {
            let x = (chosen.x + 2).min(chosen.x + chosen.width - 1);
            let y = (chosen.y + (rel * chosen.height as f32).round() as i32)
                .clamp(chosen.y, chosen.y + chosen.height - 1);
            (x, y)
        }
        Edge::Right => {
            let x = (chosen.x + chosen.width - 3).max(chosen.x);
            let y = (chosen.y + (rel * chosen.height as f32).round() as i32)
                .clamp(chosen.y, chosen.y + chosen.height - 1);
            (x, y)
        }
        Edge::Top => {
            let x = (chosen.x + (rel * chosen.width as f32).round() as i32)
                .clamp(chosen.x, chosen.x + chosen.width - 1);
            let y = (chosen.y + 2).min(chosen.y + chosen.height - 1);
            (x, y)
        }
        Edge::Bottom => {
            let x = (chosen.x + (rel * chosen.width as f32).round() as i32)
                .clamp(chosen.x, chosen.x + chosen.width - 1);
            let y = (chosen.y + chosen.height - 3).max(chosen.y);
            (x, y)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ReceiverStep {
    Move((i32, i32)),
    Return { edge: Edge, relative_y: f32 },
}

/// Pure receiver-side authoritative cursor step.
/// Applies `delta` to `cursor`: if `next` crosses `entry_edge` outward with no
/// receiver monitor beyond it at that perpendicular coordinate, returns `Return`;
/// otherwise clamps `next` to the nearest receiver monitor rect.
pub fn receiver_step(
    cursor: (i32, i32),
    delta: (i32, i32),
    monitors: &[MonitorDisplay],
    entry_edge: Edge,
) -> ReceiverStep {
    let next = (cursor.0.saturating_add(delta.0), cursor.1.saturating_add(delta.1));
    let Some((_, cur_mon)) = monitor_at_point(monitors, cursor.0, cursor.1)
        .or_else(|| nearest_monitor(monitors, cursor.0, cursor.1))
    else {
        return ReceiverStep::Move(next);
    };

    let cur_right = cur_mon.x + cur_mon.width;
    let cur_bottom = cur_mon.y + cur_mon.height;

    match entry_edge {
        Edge::Left if next.0 < cur_mon.x => {
            let check_y = next.1.clamp(cur_mon.y, cur_bottom - 1);
            let has_beyond = monitors.iter().any(|m| {
                m.width > 0
                    && m.height > 0
                    && m.x < cur_mon.x
                    && check_y >= m.y
                    && check_y < m.y + m.height
            });
            if !has_beyond {
                let rel = ((check_y - cur_mon.y) as f32 / cur_mon.height.max(1) as f32).clamp(0.0, 1.0);
                return ReceiverStep::Return {
                    edge: Edge::Left,
                    relative_y: rel,
                };
            }
        }
        Edge::Right if next.0 >= cur_right => {
            let check_y = next.1.clamp(cur_mon.y, cur_bottom - 1);
            let has_beyond = monitors.iter().any(|m| {
                m.width > 0
                    && m.height > 0
                    && (m.x + m.width) > cur_right
                    && m.x > cur_mon.x
                    && check_y >= m.y
                    && check_y < m.y + m.height
            });
            if !has_beyond {
                let rel = ((check_y - cur_mon.y) as f32 / cur_mon.height.max(1) as f32).clamp(0.0, 1.0);
                return ReceiverStep::Return {
                    edge: Edge::Right,
                    relative_y: rel,
                };
            }
        }
        Edge::Top if next.1 < cur_mon.y => {
            let check_x = next.0.clamp(cur_mon.x, cur_right - 1);
            let has_beyond = monitors.iter().any(|m| {
                m.width > 0
                    && m.height > 0
                    && m.y < cur_mon.y
                    && check_x >= m.x
                    && check_x < m.x + m.width
            });
            if !has_beyond {
                let rel = ((check_x - cur_mon.x) as f32 / cur_mon.width.max(1) as f32).clamp(0.0, 1.0);
                return ReceiverStep::Return {
                    edge: Edge::Top,
                    relative_y: rel,
                };
            }
        }
        Edge::Bottom if next.1 >= cur_bottom => {
            let check_x = next.0.clamp(cur_mon.x, cur_right - 1);
            let has_beyond = monitors.iter().any(|m| {
                m.width > 0
                    && m.height > 0
                    && (m.y + m.height) > cur_bottom
                    && m.y > cur_mon.y
                    && check_x >= m.x
                    && check_x < m.x + m.width
            });
            if !has_beyond {
                let rel = ((check_x - cur_mon.x) as f32 / cur_mon.width.max(1) as f32).clamp(0.0, 1.0);
                return ReceiverStep::Return {
                    edge: Edge::Bottom,
                    relative_y: rel,
                };
            }
        }
        _ => {}
    }

    let Some((_, target_mon)) = nearest_monitor(monitors, next.0, next.1) else {
        return ReceiverStep::Move(next);
    };
    let clamped_x = next.0.clamp(target_mon.x, target_mon.x + target_mon.width - 1);
    let clamped_y = next.1.clamp(target_mon.y, target_mon.y + target_mon.height - 1);
    ReceiverStep::Move((clamped_x, clamped_y))
}

/// Computes the controller cursor position 2 px inside `link_edge` of `mon` on return.
pub fn controller_return_pos(mon: &MonitorDisplay, link_edge: Edge, relative_y: f32) -> (i32, i32) {
    let rel = relative_y.clamp(0.0, 1.0);
    let w = mon.width.max(1);
    let h = mon.height.max(1);
    match link_edge {
        Edge::Right => {
            let x = (mon.x + w - 3).max(mon.x);
            let y = (mon.y + (rel * h as f32).round() as i32).clamp(mon.y, mon.y + h - 1);
            (x, y)
        }
        Edge::Left => {
            let x = (mon.x + 2).min(mon.x + w - 1);
            let y = (mon.y + (rel * h as f32).round() as i32).clamp(mon.y, mon.y + h - 1);
            (x, y)
        }
        Edge::Top => {
            let x = (mon.x + (rel * w as f32).round() as i32).clamp(mon.x, mon.x + w - 1);
            let y = (mon.y + 2).min(mon.y + h - 1);
            (x, y)
        }
        Edge::Bottom => {
            let x = (mon.x + (rel * w as f32).round() as i32).clamp(mon.x, mon.x + w - 1);
            let y = (mon.y + h - 3).max(mon.y);
            (x, y)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrackedKey {
    pub key_code: u16,
    pub scan_code: u16,
    pub extended: bool,
}

#[derive(Debug, Default)]
pub struct PressedInputTracker {
    keys: Vec<TrackedKey>,
    buttons_mask: u8,
}

impl PressedInputTracker {
    pub fn new() -> Self {
        Self {
            keys: Vec::with_capacity(32),
            buttons_mask: 0,
        }
    }

    pub fn record_key(
        &mut self,
        key_code: u16,
        scan_code: u16,
        is_down: bool,
        extended: bool,
        is_injected: bool,
    ) {
        if is_injected {
            return;
        }
        if is_down {
            if !self
                .keys
                .iter()
                .any(|k| k.key_code == key_code && k.extended == extended)
            {
                self.keys.push(TrackedKey {
                    key_code,
                    scan_code,
                    extended,
                });
            }
        } else {
            self.keys
                .retain(|k| !(k.key_code == key_code && k.extended == extended));
        }
    }

    pub fn record_button(&mut self, button: u8, is_down: bool) {
        if !(1..=5).contains(&button) {
            return;
        }
        let bit = 1u8 << (button - 1);
        if is_down {
            self.buttons_mask |= bit;
        } else {
            self.buttons_mask &= !bit;
        }
    }

    pub fn any_button_down(&self) -> bool {
        self.buttons_mask != 0
    }

    pub fn held_keys(&self) -> &[TrackedKey] {
        &self.keys
    }

    pub fn release_keys(&mut self) -> Vec<TrackedKey> {
        self.keys.drain(..).collect()
    }

    pub fn release_all(&mut self) -> (Vec<TrackedKey>, Vec<u8>) {
        let keys = self.keys.drain(..).collect();
        let mut buttons = Vec::new();
        for b in 1..=5u8 {
            if (self.buttons_mask & (1u8 << (b - 1))) != 0 {
                buttons.push(b);
            }
        }
        self.buttons_mask = 0;
        (keys, buttons)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mon(id: &str, x: i32, y: i32, w: i32, h: i32, primary: bool) -> MonitorDisplay {
        MonitorDisplay {
            id: id.to_string(),
            x,
            y,
            width: w,
            height: h,
            is_primary: primary,
        }
    }

    // Verification 4.1: Receiver step
    #[test]
    fn receiver_step_inside_move() {
        let mons = vec![mon("1", 0, 0, 1920, 1080, true)];
        assert_eq!(
            receiver_step((500, 500), (25, -40), &mons, Edge::Left),
            ReceiverStep::Move((525, 460))
        );
    }

    #[test]
    fn receiver_step_clamps_at_non_entry_edges() {
        let mons = vec![mon("1", 0, 0, 1920, 1080, true)];
        // Entry is Left, moving past Right/Top/Bottom clamps inside monitor
        assert_eq!(
            receiver_step((1910, 500), (50, 0), &mons, Edge::Left),
            ReceiverStep::Move((1919, 500))
        );
        assert_eq!(
            receiver_step((500, 10), (0, -30), &mons, Edge::Left),
            ReceiverStep::Move((500, 0))
        );
        assert_eq!(
            receiver_step((500, 1070), (0, 30), &mons, Edge::Left),
            ReceiverStep::Move((500, 1079))
        );
    }

    #[test]
    fn receiver_step_returns_through_entry_edge() {
        let mons = vec![mon("1", 0, 0, 1920, 1080, true)];
        assert_eq!(
            receiver_step((5, 540), (-10, 0), &mons, Edge::Left),
            ReceiverStep::Return {
                edge: Edge::Left,
                relative_y: 0.5,
            }
        );
        assert_eq!(
            receiver_step((1915, 270), (10, 0), &mons, Edge::Right),
            ReceiverStep::Return {
                edge: Edge::Right,
                relative_y: 0.25,
            }
        );
    }

    #[test]
    fn receiver_step_multi_monitor_different_heights() {
        // Mon1: [0..1920, 0..1080], Mon2: [1920..3840, 200..1280]
        let mons = vec![
            mon("1", 0, 0, 1920, 1080, true),
            mon("2", 1920, 200, 1920, 1080, false),
        ];

        // Crossing from Mon2 into Mon1 where both overlap in Y (y=500) -> moves onto Mon1, no return
        assert_eq!(
            receiver_step((1925, 500), (-15, 0), &mons, Edge::Left),
            ReceiverStep::Move((1910, 500))
        );

        // Moving left from Mon2 below Mon1's bottom (y=1150) -> no monitor to the left at y=1150 -> Return!
        match receiver_step((1925, 1150), (-15, 0), &mons, Edge::Left) {
            ReceiverStep::Return { edge, relative_y } => {
                assert_eq!(edge, Edge::Left);
                let expected = (1150 - 200) as f32 / 1080.0;
                assert!((relative_y - expected).abs() < 1e-4);
            }
            other => panic!("expected Return, got {other:?}"),
        }

        // Moving right from Mon1 above Mon2's top (y=100) -> non-entry edge -> clamps to Mon1
        assert_eq!(
            receiver_step((1910, 100), (20, 0), &mons, Edge::Left),
            ReceiverStep::Move((1919, 100))
        );
    }

    #[test]
    fn receiver_step_gap_between_monitors() {
        // 80px horizontal gap between Mon1 [0..1920] and Mon2 [2000..3920]
        let mons = vec![
            mon("1", 0, 0, 1920, 1080, true),
            mon("2", 2000, 0, 1920, 1080, false),
        ];

        // Moving left from Mon2 across the gap does NOT return (Mon1 is beyond at y=500);
        // clamps to nearest monitor rect (Mon1 when closer to Mon1, Mon2 when closer to Mon2).
        assert_eq!(
            receiver_step((2005, 500), (-50, 0), &mons, Edge::Left),
            ReceiverStep::Move((1919, 500))
        );
        assert_eq!(
            receiver_step((2005, 500), (-10, 0), &mons, Edge::Left),
            ReceiverStep::Move((2000, 500))
        );
    }

    // Verification 4.2: Controller edge detection
    #[test]
    fn controller_edge_detect_non_rectangular_and_shorter_side() {
        // Primary [0..1920, 0..1080], shorter side monitor [1920..3200, 300..1020]
        let mons = vec![
            mon("1", 0, 0, 1920, 1080, true),
            mon("2", 1920, 300, 1280, 720, false),
        ];

        // At right edge of Mon1 where Mon2 IS adjacent (y=500): must NOT leap
        assert_eq!(detect_controller_edge((1919, 500), &mons, Edge::Right, 48), None);

        // At right edge of Mon1 ABOVE Mon2 (y=150): no local monitor beyond -> leaps!
        let hit = detect_controller_edge((1919, 150), &mons, Edge::Right, 48).unwrap();
        assert_eq!(hit.0, 0);
        assert!((hit.1 - (150.0 / 1080.0)).abs() < 1e-4);

        // At right edge of Mon2 (x=3199, y=660): leaps with relative_y of Mon2!
        let hit2 = detect_controller_edge((3199, 660), &mons, Edge::Right, 48).unwrap();
        assert_eq!(hit2.0, 1);
        assert!((hit2.1 - (360.0 / 720.0)).abs() < 1e-4);

        // Away from edge (> 2 px inside): no leap
        assert_eq!(detect_controller_edge((1917, 150), &mons, Edge::Right, 48), None);
    }

    #[test]
    fn controller_edge_detect_deadzone() {
        let mons = vec![mon("1", 0, 0, 1920, 1080, true)];
        // Top and bottom 48px deadzones block leap
        assert_eq!(detect_controller_edge((1919, 10), &mons, Edge::Right, 48), None);
        assert_eq!(detect_controller_edge((1919, 47), &mons, Edge::Right, 48), None);
        assert_eq!(detect_controller_edge((1919, 1032), &mons, Edge::Right, 48), None);
        assert_eq!(detect_controller_edge((1919, 1079), &mons, Edge::Right, 48), None);
        // Just inside active region
        assert!(detect_controller_edge((1919, 48), &mons, Edge::Right, 48).is_some());
        assert!(detect_controller_edge((1919, 1031), &mons, Edge::Right, 48).is_some());
    }

    // Verification 4.3: Receiver mapping of CursorEnter
    #[test]
    fn map_cursor_enter_picks_correct_monitor_and_offsets_two_px() {
        let mons = vec![
            mon("left", 0, 0, 1920, 1080, true),
            mon("right", 1920, 300, 1920, 1080, false),
        ];

        // Entering Left at relative_y=0.25 -> mapped_y ~ 345, both contain, leftmost chosen
        let p1 = map_cursor_enter(&mons, Edge::Left, 0.25);
        assert_eq!(p1, (2, 270));

        // Entering Left at relative_y=0.9 -> mapped_y ~ 1241, only "right" spans it -> enters "right"
        let p2 = map_cursor_enter(&mons, Edge::Left, 0.9);
        assert_eq!(p2.0, 1922);
        assert_eq!(p2.1, 300 + (0.9 * 1080.0_f32).round() as i32);

        // Entering Right at relative_y=0.5 -> rightmost monitor ("right")
        let p3 = map_cursor_enter(&mons, Edge::Right, 0.5);
        assert_eq!(p3, (3837, 840));
    }

    // Verification 4.4: Pressed-key tracker
    #[test]
    fn pressed_input_tracker_lifecycle_and_injected_ignored() {
        let mut tracker = PressedInputTracker::new();
        tracker.record_key(0xA2, 0x1D, true, false, false); // LCtrl down
        tracker.record_key(0x41, 0x1E, true, false, false); // 'A' down
        // Injected events must be ignored
        tracker.record_key(0x42, 0x30, true, false, true);
        tracker.record_key(0x41, 0x1E, false, false, true);
        assert_eq!(tracker.held_keys().len(), 2);

        // Real 'A' up removes only 'A'
        tracker.record_key(0x41, 0x1E, false, false, false);
        assert_eq!(
            tracker.held_keys(),
            &[TrackedKey {
                key_code: 0xA2,
                scan_code: 0x1D,
                extended: false
            }]
        );

        tracker.record_button(1, true);
        tracker.record_button(3, true);
        assert!(tracker.any_button_down());

        let (keys, buttons) = tracker.release_all();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].key_code, 0xA2);
        assert_eq!(buttons, vec![1, 3]);
        assert!(tracker.held_keys().is_empty());
        assert!(!tracker.any_button_down());
    }

    #[test]
    fn controller_return_pos_places_two_px_inside_edge() {
        let m = mon("1", 0, 0, 1920, 1080, true);
        assert_eq!(controller_return_pos(&m, Edge::Right, 0.5), (1917, 540));
        assert_eq!(controller_return_pos(&m, Edge::Left, 0.5), (2, 540));
        assert_eq!(detect_controller_edge((1917, 540), &[m.clone()], Edge::Right, 48), None);
        assert_eq!(detect_controller_edge((2, 540), &[m], Edge::Left, 48), None);
    }
}
