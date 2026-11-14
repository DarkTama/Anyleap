#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct MonitorDisplay {
    pub id: String,
    #[serde(default)]
    pub device_name: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub is_primary: bool,
}

impl MonitorDisplay {
    /// Stable monitor identifier across reconnects (`device_name`, falling back to `id`).
    pub fn effective_id(&self) -> &str {
        if !self.device_name.is_empty() {
            &self.device_name
        } else {
            &self.id
        }
    }
}

pub fn get_local_monitors() -> Vec<MonitorDisplay> {
    #[cfg(windows)]
    unsafe {
        use windows::core::BOOL;
        use windows::Win32::Foundation::{LPARAM, RECT};
        use windows::Win32::Graphics::Gdi::*;

        let mut list: Vec<MonitorDisplay> = Vec::new();
        let list_ptr = &mut list as *mut Vec<MonitorDisplay> as isize;

        unsafe extern "system" fn monitor_proc(
            hmonitor: HMONITOR,
            _hdc: HDC,
            _rect: *mut RECT,
            lparam: LPARAM,
        ) -> BOOL {
            let list = &mut *(lparam.0 as *mut Vec<MonitorDisplay>);
            let mut info = MONITORINFOEXW::default();
            info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;

            if GetMonitorInfoW(hmonitor, &mut info.monitorInfo).as_bool() {
                let rc = info.monitorInfo.rcMonitor;
                let w = rc.right - rc.left;
                let h = rc.bottom - rc.top;
                let is_primary = (info.monitorInfo.dwFlags & 1) != 0; // MONITORINFOF_PRIMARY = 1
                let nul = info
                    .szDevice
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(info.szDevice.len());
                let raw_device = String::from_utf16_lossy(&info.szDevice[..nul]);
                let device_name = if raw_device.is_empty() {
                    format!("mon-{}", list.len() + 1)
                } else {
                    raw_device
                };
                list.push(MonitorDisplay {
                    id: device_name.clone(),
                    device_name,
                    x: rc.left,
                    y: rc.top,
                    width: w,
                    height: h,
                    is_primary,
                });
            }
            BOOL(1)
        }

        let _ = EnumDisplayMonitors(None, None, Some(monitor_proc), LPARAM(list_ptr));

        if !list.is_empty() {
            return list;
        }
    }

    // Fallback
    vec![MonitorDisplay {
        id: r"\\.\DISPLAY1".to_string(),
        device_name: r"\\.\DISPLAY1".to_string(),
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
        is_primary: true,
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monitor_display_deserializes_legacy_and_uses_device_name_as_id() {
        let legacy = r#"{"id":"mon-1","x":0,"y":0,"width":1920,"height":1080,"is_primary":true}"#;
        let m: MonitorDisplay = serde_json::from_str(legacy).expect("deserialize legacy");
        assert_eq!(m.device_name, "");
        assert_eq!(m.effective_id(), "mon-1");

        let modern = MonitorDisplay {
            id: r"\\.\DISPLAY1".into(),
            device_name: r"\\.\DISPLAY1".into(),
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
            is_primary: true,
        };
        assert_eq!(modern.effective_id(), r"\\.\DISPLAY1");

        let local = get_local_monitors();
        assert!(!local.is_empty());
        for mon in &local {
            assert!(!mon.device_name.is_empty());
            assert_eq!(mon.id, mon.device_name);
        }
    }
}
