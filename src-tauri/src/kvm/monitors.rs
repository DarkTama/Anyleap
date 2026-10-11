#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct MonitorDisplay {
    pub id: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub is_primary: bool,
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
            let mut info = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                rcMonitor: RECT::default(),
                rcWork: RECT::default(),
                dwFlags: 0,
            };

            if GetMonitorInfoW(hmonitor, &mut info).as_bool() {
                let w = info.rcMonitor.right - info.rcMonitor.left;
                let h = info.rcMonitor.bottom - info.rcMonitor.top;
                let is_primary = (info.dwFlags & 1) != 0; // MONITORINFOF_PRIMARY = 1
                list.push(MonitorDisplay {
                    id: format!("mon-{}", list.len() + 1),
                    x: info.rcMonitor.left,
                    y: info.rcMonitor.top,
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
        id: "mon-1".to_string(),
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
        is_primary: true,
    }]
}
