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

pub fn is_edge_monitor(mon: &MonitorDisplay, union: &ScreenRect, edge: Edge) -> bool {
    if mon.width <= 0 || mon.height <= 0 {
        return false;
    }
    match edge {
        Edge::Left => mon.x == union.x,
        Edge::Right => mon.x + mon.width == union.x + union.width,
        Edge::Top => mon.y == union.y,
        Edge::Bottom => mon.y + mon.height == union.y + union.height,
    }
}

fn is_in_edge_deadzone(mon: &MonitorDisplay, edge: Edge, perp_coord: i32, deadzone_px: i32) -> bool {
    match edge {
        Edge::Left | Edge::Right => {
            let dz = deadzone_px.max(0).min((mon.height - 1).max(0) / 2);
            perp_coord < mon.y + dz || perp_coord >= mon.y + mon.height - dz
        }
        Edge::Top | Edge::Bottom => {
            let dz = deadzone_px.max(0).min((mon.width - 1).max(0) / 2);
            perp_coord < mon.x + dz || perp_coord >= mon.x + mon.width - dz
        }
    }
}

/// Pure controller-side per-monitor edge detection.
/// Triggers only from monitors touching the union's `link_edge` when `pt` is
/// within 2 px of `link_edge`, there is no local monitor immediately beyond
/// that edge at `pt`'s perpendicular coordinate, and `pt` is outside the corner
/// deadzone of that edge monitor.
pub fn detect_controller_edge(
    pt: (i32, i32),
    monitors: &[MonitorDisplay],
    link_edge: Edge,
    deadzone_px: i32,
) -> Option<(usize, f32)> {
    // A low-level hook reports the *proposed* position, which lies past the
    // screen edge when the user pushes against it. Resolve it to the nearest
    // monitor and clamp, otherwise pushes past the edge are never detected.
    let (idx, mon) = monitor_at_point(monitors, pt.0, pt.1)
        .or_else(|| nearest_monitor(monitors, pt.0, pt.1))?;
    let union = union_rect(monitors);
    if !is_edge_monitor(mon, &union, link_edge) {
        return None;
    }
    let right = mon.x + mon.width;
    let bottom = mon.y + mon.height;
    let pt = (pt.0.clamp(mon.x, right - 1), pt.1.clamp(mon.y, bottom - 1));

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

    let (perp, span_len) = match link_edge {
        Edge::Left | Edge::Right => (pt.1, mon.height),
        Edge::Top | Edge::Bottom => (pt.0, mon.width),
    };
    if is_in_edge_deadzone(mon, link_edge, perp, deadzone_px) {
        return None;
    }
    let origin = match link_edge {
        Edge::Left | Edge::Right => mon.y,
        Edge::Top | Edge::Bottom => mon.x,
    };
    let rel = ((perp - origin) as f32 / span_len.max(1) as f32).clamp(0.0, 1.0);

    Some((idx, rel))
}

/// Picks the edge monitor touching the union's `edge` whose perpendicular span
/// contains `perp_coord` (or the nearest edge monitor if `perp_coord` lies
/// outside every edge monitor's span). Never returns a non-edge monitor.
pub fn pick_edge_monitor(
    monitors: &[MonitorDisplay],
    edge: Edge,
    perp_coord: i32,
) -> Option<&MonitorDisplay> {
    let union = union_rect(monitors);
    let edge_mons: Vec<&MonitorDisplay> = monitors
        .iter()
        .filter(|m| is_edge_monitor(m, &union, edge))
        .collect();
    if edge_mons.is_empty() {
        return None;
    }

    match edge {
        Edge::Left | Edge::Right => edge_mons
            .iter()
            .copied()
            .find(|m| perp_coord >= m.y && perp_coord < m.y + m.height)
            .or_else(|| {
                edge_mons.into_iter().min_by_key(|m| {
                    let cy = perp_coord.clamp(m.y, m.y + m.height - 1);
                    (perp_coord - cy).abs()
                })
            }),
        Edge::Top | Edge::Bottom => edge_mons
            .iter()
            .copied()
            .find(|m| perp_coord >= m.x && perp_coord < m.x + m.width)
            .or_else(|| {
                edge_mons.into_iter().min_by_key(|m| {
                    let cx = perp_coord.clamp(m.x, m.x + m.width - 1);
                    (perp_coord - cx).abs()
                })
            }),
    }
}

/// Computes `CursorEnter.relative_y` normalised over the receiver union (0..1)
/// after applying the link's pixel offset along the shared edge.
pub fn map_leap_relative_y(
    pt: (i32, i32),
    local_union: &ScreenRect,
    peer_union: &ScreenRect,
    link_edge: Edge,
    offset_x: i32,
    offset_y: i32,
) -> f32 {
    match link_edge {
        Edge::Left | Edge::Right => {
            let denom = if peer_union.height > 0 {
                peer_union.height
            } else {
                local_union.height.max(1)
            };
            ((pt.1 - local_union.y - offset_y) as f32 / denom as f32).clamp(0.0, 1.0)
        }
        Edge::Top | Edge::Bottom => {
            let denom = if peer_union.width > 0 {
                peer_union.width
            } else {
                local_union.width.max(1)
            };
            ((pt.0 - local_union.x - offset_x) as f32 / denom as f32).clamp(0.0, 1.0)
        }
    }
}

/// Maps `CursorEnter { entry_edge, relative_y }` (normalised over the receiver
/// union) onto a receiver coordinate 2 px inside the chosen monitor's `entry_edge`.
pub fn map_cursor_enter(monitors: &[MonitorDisplay], entry_edge: Edge, relative_y: f32) -> (i32, i32) {
    let rel = relative_y.clamp(0.0, 1.0);
    let union = union_rect(monitors);
    if union.width <= 0 || union.height <= 0 {
        return (2, 2);
    }

    match entry_edge {
        Edge::Left | Edge::Right => {
            let mapped_y = (union.y + (rel * union.height as f32).round() as i32)
                .clamp(union.y, union.y + union.height - 1);
            let Some(chosen) = pick_edge_monitor(monitors, entry_edge, mapped_y) else {
                return (2, 2);
            };
            let x = if entry_edge == Edge::Left {
                (chosen.x + 2).min(chosen.x + chosen.width - 1)
            } else {
                (chosen.x + chosen.width - 3).max(chosen.x)
            };
            let y = mapped_y.clamp(chosen.y, chosen.y + chosen.height - 1);
            (x, y)
        }
        Edge::Top | Edge::Bottom => {
            let mapped_x = (union.x + (rel * union.width as f32).round() as i32)
                .clamp(union.x, union.x + union.width - 1);
            let Some(chosen) = pick_edge_monitor(monitors, entry_edge, mapped_x) else {
                return (2, 2);
            };
            let x = mapped_x.clamp(chosen.x, chosen.x + chosen.width - 1);
            let y = if entry_edge == Edge::Top {
                (chosen.y + 2).min(chosen.y + chosen.height - 1)
            } else {
                (chosen.y + chosen.height - 3).max(chosen.y)
            };
            (x, y)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ReceiverStep {
    Move((i32, i32)),
    Return { edge: Edge, relative_y: f32 },
}

/// Pure receiver-side authoritative cursor step with push-resistance dwell and
/// corner deadzone.
///
/// Only monitors touching the union's `entry_edge` can initiate a return. When
/// pushing past `entry_edge` outside the corner deadzone, `dwell_ms == 0`
/// returns immediately; otherwise the cursor is held at the edge until it has
/// pushed continuously for `dwell_ms`. Moving away or entering the deadzone
/// resets `dwell_start_ms`.
#[allow(clippy::too_many_arguments)]
pub fn receiver_step_with_dwell(
    cursor: (i32, i32),
    delta: (i32, i32),
    monitors: &[MonitorDisplay],
    entry_edge: Edge,
    dwell_ms: u32,
    deadzone_px: i32,
    dwell_start_ms: &mut Option<u64>,
    now_ms: u64,
) -> ReceiverStep {
    let next = (cursor.0.saturating_add(delta.0), cursor.1.saturating_add(delta.1));
    let Some((_, cur_mon)) = monitor_at_point(monitors, cursor.0, cursor.1)
        .or_else(|| nearest_monitor(monitors, cursor.0, cursor.1))
    else {
        *dwell_start_ms = None;
        return ReceiverStep::Move(next);
    };

    let union = union_rect(monitors);
    let cur_right = cur_mon.x + cur_mon.width;
    let cur_bottom = cur_mon.y + cur_mon.height;

    let pushing_and_rel = if is_edge_monitor(cur_mon, &union, entry_edge) {
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
                (!has_beyond).then(|| {
                    let rel = ((check_y - union.y) as f32 / union.height.max(1) as f32).clamp(0.0, 1.0);
                    (check_y, rel)
                })
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
                (!has_beyond).then(|| {
                    let rel = ((check_y - union.y) as f32 / union.height.max(1) as f32).clamp(0.0, 1.0);
                    (check_y, rel)
                })
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
                (!has_beyond).then(|| {
                    let rel = ((check_x - union.x) as f32 / union.width.max(1) as f32).clamp(0.0, 1.0);
                    (check_x, rel)
                })
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
                (!has_beyond).then(|| {
                    let rel = ((check_x - union.x) as f32 / union.width.max(1) as f32).clamp(0.0, 1.0);
                    (check_x, rel)
                })
            }
            _ => None,
        }
    } else {
        None
    };

    if let Some((perp, rel)) = pushing_and_rel {
        if !is_in_edge_deadzone(cur_mon, entry_edge, perp, deadzone_px) {
            if dwell_ms == 0 {
                *dwell_start_ms = None;
                return ReceiverStep::Return {
                    edge: entry_edge,
                    relative_y: rel,
                };
            }
            let start = *dwell_start_ms.get_or_insert(now_ms);
            if now_ms.saturating_sub(start) >= dwell_ms as u64 {
                *dwell_start_ms = None;
                return ReceiverStep::Return {
                    edge: entry_edge,
                    relative_y: rel,
                };
            }
        } else {
            *dwell_start_ms = None;
        }
    } else {
        *dwell_start_ms = None;
    }

    let Some((_, target_mon)) = nearest_monitor(monitors, next.0, next.1) else {
        return ReceiverStep::Move(next);
    };
    let clamped_x = next.0.clamp(target_mon.x, target_mon.x + target_mon.width - 1);
    let clamped_y = next.1.clamp(target_mon.y, target_mon.y + target_mon.height - 1);
    ReceiverStep::Move((clamped_x, clamped_y))
}

/// Immediate (0 ms dwell, 0 px deadzone) receiver step helper.
pub fn receiver_step(
    cursor: (i32, i32),
    delta: (i32, i32),
    monitors: &[MonitorDisplay],
    entry_edge: Edge,
) -> ReceiverStep {
    receiver_step_with_dwell(cursor, delta, monitors, entry_edge, 0, 0, &mut None, 0)
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

/// Converts a receiver-union `CursorReturn.relative_y` using the link's pixel
/// offset into controller coordinates and clamps to the appropriate edge monitor.
pub fn map_return_position(
    local_monitors: &[MonitorDisplay],
    fallback_mon: &MonitorDisplay,
    peer_union: &ScreenRect,
    link_edge: Edge,
    offset_x: i32,
    offset_y: i32,
    relative_y: f32,
) -> (i32, i32) {
    let rel = relative_y.clamp(0.0, 1.0);
    let local_union = if local_monitors.is_empty() {
        ScreenRect {
            x: fallback_mon.x,
            y: fallback_mon.y,
            width: fallback_mon.width,
            height: fallback_mon.height,
        }
    } else {
        union_rect(local_monitors)
    };

    match link_edge {
        Edge::Right | Edge::Left => {
            let peer_h = if peer_union.height > 0 {
                peer_union.height
            } else {
                local_union.height.max(1)
            };
            let target_y = local_union.y + offset_y + (rel * peer_h as f32).round() as i32;
            let mon = pick_edge_monitor(local_monitors, link_edge, target_y).unwrap_or(fallback_mon);
            let w = mon.width.max(1);
            let h = mon.height.max(1);
            let x = if link_edge == Edge::Right {
                (mon.x + w - 3).max(mon.x)
            } else {
                (mon.x + 2).min(mon.x + w - 1)
            };
            let y = target_y.clamp(mon.y, mon.y + h - 1);
            (x, y)
        }
        Edge::Top | Edge::Bottom => {
            let peer_w = if peer_union.width > 0 {
                peer_union.width
            } else {
                local_union.width.max(1)
            };
            let target_x = local_union.x + offset_x + (rel * peer_w as f32).round() as i32;
            let mon = pick_edge_monitor(local_monitors, link_edge, target_x).unwrap_or(fallback_mon);
            let w = mon.width.max(1);
            let h = mon.height.max(1);
            let x = target_x.clamp(mon.x, mon.x + w - 1);
            let y = if link_edge == Edge::Top {
                (mon.y + 2).min(mon.y + h - 1)
            } else {
                (mon.y + h - 3).max(mon.y)
            };
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

        // Moving left from Mon2 below Mon1's bottom (y=1150): Mon2 does NOT touch
        // the left union edge (x=1920 != 0), so it clamps and must NOT return.
        assert_eq!(
            receiver_step((1925, 1150), (-15, 0), &mons, Edge::Left),
            ReceiverStep::Move((1920, 1150))
        );

        // Moving left from Mon1 (which touches x=0) returns normally.
        match receiver_step((5, 500), (-15, 0), &mons, Edge::Left) {
            ReceiverStep::Return { edge, relative_y } => {
                assert_eq!(edge, Edge::Left);
                let expected = 500.0 / 1280.0;
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

        // At right edge of Mon1 ABOVE Mon2 (y=150): Mon1 is NOT a right-edge monitor (1920 != 3200) -> must NOT leap!
        assert_eq!(detect_controller_edge((1919, 150), &mons, Edge::Right, 48), None);

        // At right edge of Mon2 (x=3199, y=660): leaps with relative_y of Mon2!
        let hit2 = detect_controller_edge((3199, 660), &mons, Edge::Right, 48).unwrap();
        assert_eq!(hit2.0, 1);
        assert!((hit2.1 - (360.0 / 720.0)).abs() < 1e-4);

        // Away from edge (> 2 px inside): no leap
        assert_eq!(detect_controller_edge((3197, 660), &mons, Edge::Right, 48), None);
    }

    #[test]
    fn controller_edge_detects_proposed_point_past_the_screen_edge() {
        // Low-level hooks report the unclamped proposed position when pushing past the edge.
        let mons = vec![mon("1", 0, 0, 1920, 1080, true)];
        let hit = detect_controller_edge((1935, 540), &mons, Edge::Right, 48).unwrap();
        assert_eq!(hit.0, 0);
        assert!((hit.1 - 0.5).abs() < 1e-3);
        assert!(detect_controller_edge((-12, 540), &mons, Edge::Left, 48).is_some());
        // Past the edge but inside the corner deadzone: still no leap.
        assert_eq!(detect_controller_edge((1935, 5), &mons, Edge::Right, 48), None);
        // Past the bottom while linked on the right: not an edge hit.
        assert_eq!(detect_controller_edge((900, 1100), &mons, Edge::Right, 48), None);
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

        // Entering Left at relative_y=0.25 -> mapped_y = 345, only "left" touches left edge
        let p1 = map_cursor_enter(&mons, Edge::Left, 0.25);
        assert_eq!(p1, (2, 345));

        // Entering Left at relative_y=0.9 -> mapped_y = 1242, clamps to "left" bottom (1079), never "right"
        let p2 = map_cursor_enter(&mons, Edge::Left, 0.9);
        assert_eq!(p2, (2, 1079));

        // Entering Right at relative_y=0.5 -> mapped_y = 690, rightmost monitor ("right")
        let p3 = map_cursor_enter(&mons, Edge::Right, 0.5);
        assert_eq!(p3, (3837, 690));
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

    #[test]
    fn offset_mapping_leap_and_return_left_right_top_bottom() {
        let ctrl_mons = vec![mon("c1", 0, 0, 1920, 1080, true)];
        let ctrl_union = union_rect(&ctrl_mons);
        let peer_mons = vec![mon("p1", 0, 0, 1920, 1080, true)];
        let peer_union = union_rect(&peer_mons);

        // 1) Right edge with offset_y = +300: controller y=540 aligns with peer y=240.
        let rel_r = map_leap_relative_y((1919, 540), &ctrl_union, &peer_union, Edge::Right, 0, 300);
        let enter_r = map_cursor_enter(&peer_mons, Edge::Left, rel_r);
        assert_eq!(enter_r, (2, 240));
        let ReceiverStep::Return { relative_y: ret_rel_r, .. } =
            receiver_step((2, 240), (-10, 0), &peer_mons, Edge::Left)
        else {
            panic!("expected Return");
        };
        let back_r = map_return_position(&ctrl_mons, &ctrl_mons[0], &peer_union, Edge::Right, 0, 300, ret_rel_r);
        assert_eq!(back_r, (1917, 540));

        // 2) Left edge with offset_y = -200: controller y=400 aligns with peer y=600.
        let rel_l = map_leap_relative_y((0, 400), &ctrl_union, &peer_union, Edge::Left, 0, -200);
        let enter_l = map_cursor_enter(&peer_mons, Edge::Right, rel_l);
        assert_eq!(enter_l, (1917, 600));
        let ReceiverStep::Return { relative_y: ret_rel_l, .. } =
            receiver_step((1917, 600), (10, 0), &peer_mons, Edge::Right)
        else {
            panic!("expected Return");
        };
        let back_l = map_return_position(&ctrl_mons, &ctrl_mons[0], &peer_union, Edge::Left, 0, -200, ret_rel_l);
        assert_eq!(back_l, (2, 400));

        // 3) Top edge with offset_x = +400: controller x=1000 aligns with peer x=600.
        let rel_t = map_leap_relative_y((1000, 0), &ctrl_union, &peer_union, Edge::Top, 400, 0);
        let enter_t = map_cursor_enter(&peer_mons, Edge::Bottom, rel_t);
        assert_eq!(enter_t, (600, 1077));
        let ReceiverStep::Return { relative_y: ret_rel_t, .. } =
            receiver_step((600, 1077), (0, 10), &peer_mons, Edge::Bottom)
        else {
            panic!("expected Return");
        };
        let back_t = map_return_position(&ctrl_mons, &ctrl_mons[0], &peer_union, Edge::Top, 400, 0, ret_rel_t);
        assert_eq!(back_t, (1000, 2));

        // 4) Bottom edge with offset_x = -300: controller x=500 aligns with peer x=800.
        let rel_b = map_leap_relative_y((500, 1079), &ctrl_union, &peer_union, Edge::Bottom, -300, 0);
        let enter_b = map_cursor_enter(&peer_mons, Edge::Top, rel_b);
        assert_eq!(enter_b, (800, 2));
        let ReceiverStep::Return { relative_y: ret_rel_b, .. } =
            receiver_step((800, 2), (0, -10), &peer_mons, Edge::Top)
        else {
            panic!("expected Return");
        };
        let back_b = map_return_position(&ctrl_mons, &ctrl_mons[0], &peer_union, Edge::Bottom, -300, 0, ret_rel_b);
        assert_eq!(back_b, (500, 1077));

        // 5) Out-of-span return clamps to controller edge monitor.
        // With offset_y = +500, peer bottom (rel=1.0 -> 1580) clamps to 1079 on ctrl_mons[0].
        assert_eq!(
            map_return_position(&ctrl_mons, &ctrl_mons[0], &peer_union, Edge::Right, 0, 500, 1.0),
            (1917, 1079)
        );
        // With offset_x = -500, peer left (rel=0.0 -> -500) clamps to 0 on ctrl_mons[0].
        assert_eq!(
            map_return_position(&ctrl_mons, &ctrl_mons[0], &peer_union, Edge::Bottom, -500, 0, 0.0),
            (0, 1077)
        );
    }

    #[test]
    fn offset_mapping_multi_monitor_controller_picks_outer_edge_monitor() {
        // Controller has [0..1920, 0..1080] and shorter [1920..3200, 300..1020]
        let ctrl_mons = vec![
            mon("c1", 0, 0, 1920, 1080, true),
            mon("c2", 1920, 300, 1280, 720, false),
        ];
        let peer_union = ScreenRect { x: 0, y: 0, width: 1920, height: 1080 };
        // Returning at y=600 (offset_y=0) hits c2 (right-edge monitor) -> x = 3197.
        assert_eq!(
            map_return_position(&ctrl_mons, &ctrl_mons[0], &peer_union, Edge::Right, 0, 0, 600.0 / 1080.0),
            (3197, 600)
        );
        // Returning at y=150 (above c2) clamps to c2's top (300) and never lands on non-edge c1.
        assert_eq!(
            map_return_position(&ctrl_mons, &ctrl_mons[0], &peer_union, Edge::Right, 0, 0, 150.0 / 1080.0),
            (3197, 300)
        );
    }

    // Fix 1 tests: return dwell and corner deadzone
    #[test]
    fn receiver_step_dwell_zero_returns_immediately() {
        let mons = vec![mon("1", 0, 0, 1920, 1080, true)];
        let mut dwell_start = None;
        assert_eq!(
            receiver_step_with_dwell((5, 540), (-10, 0), &mons, Edge::Left, 0, 48, &mut dwell_start, 1000),
            ReceiverStep::Return {
                edge: Edge::Left,
                relative_y: 0.5,
            }
        );
        assert_eq!(dwell_start, None);
    }

    #[test]
    fn receiver_step_dwell_holds_at_edge_until_elapsed_then_returns() {
        let mons = vec![mon("1", 0, 0, 1920, 1080, true)];
        let mut dwell_start = None;

        // First push against Left edge starts timer and holds cursor at x=0.
        assert_eq!(
            receiver_step_with_dwell((5, 540), (-10, 0), &mons, Edge::Left, 120, 48, &mut dwell_start, 1000),
            ReceiverStep::Move((0, 540))
        );
        assert_eq!(dwell_start, Some(1000));

        // Still pushing before 120 ms elapsed: held at edge.
        assert_eq!(
            receiver_step_with_dwell((0, 540), (-8, 0), &mons, Edge::Left, 120, 48, &mut dwell_start, 1080),
            ReceiverStep::Move((0, 540))
        );
        assert_eq!(dwell_start, Some(1000));

        // At 120 ms elapsed: returns and clears timer.
        assert_eq!(
            receiver_step_with_dwell((0, 540), (-4, 0), &mons, Edge::Left, 120, 48, &mut dwell_start, 1120),
            ReceiverStep::Return {
                edge: Edge::Left,
                relative_y: 0.5,
            }
        );
        assert_eq!(dwell_start, None);
    }

    #[test]
    fn receiver_step_dwell_moving_away_resets_timer() {
        let mons = vec![mon("1", 0, 0, 1920, 1080, true)];
        let mut dwell_start = None;

        assert_eq!(
            receiver_step_with_dwell((2, 540), (-10, 0), &mons, Edge::Left, 120, 48, &mut dwell_start, 1000),
            ReceiverStep::Move((0, 540))
        );
        assert_eq!(dwell_start, Some(1000));

        // Moving away from the edge resets timer.
        assert_eq!(
            receiver_step_with_dwell((0, 540), (6, 0), &mons, Edge::Left, 120, 48, &mut dwell_start, 1050),
            ReceiverStep::Move((6, 540))
        );
        assert_eq!(dwell_start, None);

        // Pushing again at t=1150 starts a fresh timer instead of returning immediately.
        assert_eq!(
            receiver_step_with_dwell((6, 540), (-12, 0), &mons, Edge::Left, 120, 48, &mut dwell_start, 1150),
            ReceiverStep::Move((0, 540))
        );
        assert_eq!(dwell_start, Some(1150));

        // Returns 120 ms after the new push started (t=1270).
        assert_eq!(
            receiver_step_with_dwell((0, 540), (-5, 0), &mons, Edge::Left, 120, 48, &mut dwell_start, 1270),
            ReceiverStep::Return {
                edge: Edge::Left,
                relative_y: 0.5,
            }
        );
    }

    #[test]
    fn receiver_step_deadzone_blocks_return_and_resets_dwell() {
        let mons = vec![mon("1", 0, 0, 1920, 1080, true)];
        let mut dwell_start = Some(500);

        // Top 48px deadzone on Left edge blocks even with dwell=0 or elapsed dwell.
        assert_eq!(
            receiver_step_with_dwell((2, 20), (-10, 0), &mons, Edge::Left, 120, 48, &mut dwell_start, 1000),
            ReceiverStep::Move((0, 20))
        );
        assert_eq!(dwell_start, None);

        // Bottom 48px deadzone on Right edge blocks even with dwell=0.
        assert_eq!(
            receiver_step_with_dwell((1918, 1050), (10, 0), &mons, Edge::Right, 0, 48, &mut dwell_start, 1000),
            ReceiverStep::Move((1919, 1050))
        );

        // Left/Right 48px deadzones on Top and Bottom edges block return.
        assert_eq!(
            receiver_step_with_dwell((30, 2), (0, -10), &mons, Edge::Top, 0, 48, &mut dwell_start, 1000),
            ReceiverStep::Move((30, 0))
        );
        assert_eq!(
            receiver_step_with_dwell((1900, 1078), (0, 10), &mons, Edge::Bottom, 0, 48, &mut dwell_start, 1000),
            ReceiverStep::Move((1900, 1079))
        );

        // Just outside deadzone on Top edge works after dwell.
        assert_eq!(
            receiver_step_with_dwell((48, 2), (0, -10), &mons, Edge::Top, 80, 48, &mut dwell_start, 2000),
            ReceiverStep::Move((48, 0))
        );
        assert_eq!(dwell_start, Some(2000));
        assert!(matches!(
            receiver_step_with_dwell((48, 0), (0, -5), &mons, Edge::Top, 80, 48, &mut dwell_start, 2080),
            ReceiverStep::Return { edge: Edge::Top, .. }
        ));
    }

    // Fix 3 tests: edge monitors only
    #[test]
    fn shared_edge_landscape_plus_taller_portrait_left_link() {
        // Main PC: landscape 1920x1080 at (0,0) touching left edge + taller portrait 1080x1920 at (1920,-420)
        let main_mons = vec![
            mon("landscape", 0, 0, 1920, 1080, true),
            mon("portrait", 1920, -420, 1080, 1920, false),
        ];
        // Linked PC on the left: single 1920x1920 tall monitor or 1920x1080 at offset_y=0
        let peer_union = ScreenRect { x: 0, y: 0, width: 1920, height: 1920 };

        // Return to Main PC at heights outside landscape (y = -200 above, y = 1300 below)
        // must clamp to landscape (x = 2), NEVER land on portrait (x = 1922).
        // With offset_y = -420, rel=0.0 -> target_y = -420 + (-420) = -840; with offset_y = 0, rel=0.1 -> -228.
        let ret_above = map_return_position(
            &main_mons,
            &main_mons[0],
            &peer_union,
            Edge::Left,
            0,
            0,
            0.0, // target_y = -420 + 0 = -420 (portrait spans -420..1500, landscape spans 0..1080)
        );
        assert_eq!(ret_above, (2, 0));

        let ret_below = map_return_position(
            &main_mons,
            &main_mons[0],
            &peer_union,
            Edge::Left,
            0,
            0,
            1720.0 / 1920.0, // target_y = -420 + 1720 = 1300
        );
        assert_eq!(ret_below, (2, 1079));

        // Return inside landscape span (target_y = 540 -> rel = 960/1920 = 0.5) lands directly at (2, 540).
        let ret_mid = map_return_position(
            &main_mons,
            &main_mons[0],
            &peer_union,
            Edge::Left,
            0,
            0,
            0.5,
        );
        assert_eq!(ret_mid, (2, 540));

        // Leap from Main PC: portrait's left edge (x=1920) above/below landscape must NOT leap.
        assert_eq!(detect_controller_edge((1920, -200), &main_mons, Edge::Left, 48), None);
        assert_eq!(detect_controller_edge((1920, 1300), &main_mons, Edge::Left, 48), None);
        // Landscape's left edge (x=0) leaps normally.
        assert!(detect_controller_edge((-5, 540), &main_mons, Edge::Left, 48).is_some());

        // Receiver with the same multi-monitor layout entering on Left: clamps to landscape, never portrait.
        assert_eq!(map_cursor_enter(&main_mons, Edge::Left, 0.0), (2, 0));
        assert_eq!(map_cursor_enter(&main_mons, Edge::Left, 1.0), (2, 1079));

        // Receiver step on portrait's left edge (x=1925, y=1300) must clamp, never return.
        assert_eq!(
            receiver_step((1925, 1300), (-15, 0), &main_mons, Edge::Left),
            ReceiverStep::Move((1920, 1300))
        );
    }

    #[test]
    fn shared_edge_top_and_bottom_variants() {
        // Top-edge layout: MonA [0..1920, 0..1080] touches Top (y=0),
        // wider MonB [-400..2400, 1080..2160] is below and does NOT touch Top.
        let top_mons = vec![
            mon("top", 0, 0, 1920, 1080, true),
            mon("bottom", -400, 1080, 2800, 1080, false),
        ];
        let peer_union = ScreenRect { x: 0, y: 0, width: 2800, height: 1080 };

        // Return outside MonA's X span (target_x = -400 + 100 = -300, target_x = 2200) clamps to MonA [0..1919], y=2.
        assert_eq!(
            map_return_position(&top_mons, &top_mons[0], &peer_union, Edge::Top, 0, 0, 100.0 / 2800.0),
            (0, 2)
        );
        assert_eq!(
            map_return_position(&top_mons, &top_mons[0], &peer_union, Edge::Top, 0, 0, 2600.0 / 2800.0),
            (1919, 2)
        );
        // Entry on receiver clamps to MonA.
        assert_eq!(map_cursor_enter(&top_mons, Edge::Top, 0.0), (0, 2));
        assert_eq!(map_cursor_enter(&top_mons, Edge::Top, 1.0), (1919, 2));
        // Non-edge MonB top edge at x=-200 or x=2100 must NOT leap on Edge::Top.
        assert_eq!(detect_controller_edge((-200, 1080), &top_mons, Edge::Top, 48), None);
        assert_eq!(detect_controller_edge((2100, 1080), &top_mons, Edge::Top, 48), None);
        // MonA top edge leaps.
        assert!(detect_controller_edge((960, -5), &top_mons, Edge::Top, 48).is_some());

        // Bottom-edge layout: MonC [0..1920, 1080..2160] touches Bottom (y+h=2160),
        // wider MonD [-400..2400, 0..1080] is above and does NOT touch Bottom.
        let bottom_mons = vec![
            mon("upper", -400, 0, 2800, 1080, false),
            mon("lower", 0, 1080, 1920, 1080, true),
        ];
        // Return outside MonC's X span clamps to MonC [0..1919], y=2157.
        assert_eq!(
            map_return_position(&bottom_mons, &bottom_mons[1], &peer_union, Edge::Bottom, 0, 0, 0.0),
            (0, 2157)
        );
        assert_eq!(
            map_return_position(&bottom_mons, &bottom_mons[1], &peer_union, Edge::Bottom, 0, 0, 1.0),
            (1919, 2157)
        );
        // Entry on receiver clamps to MonC.
        assert_eq!(map_cursor_enter(&bottom_mons, Edge::Bottom, 0.0), (0, 2157));
        assert_eq!(map_cursor_enter(&bottom_mons, Edge::Bottom, 1.0), (1919, 2157));
        // Non-edge MonD bottom edge at x=-200 or x=2100 must NOT leap on Edge::Bottom.
        assert_eq!(detect_controller_edge((-200, 1079), &bottom_mons, Edge::Bottom, 48), None);
        assert_eq!(detect_controller_edge((2100, 1079), &bottom_mons, Edge::Bottom, 48), None);
        // MonC bottom edge leaps.
        assert!(detect_controller_edge((960, 2165), &bottom_mons, Edge::Bottom, 48).is_some());
    }
}
