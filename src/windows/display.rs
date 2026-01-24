//! Display configuration and coordinate conversion for Windows.

use serde::{Deserialize, Serialize};
use std::mem;
use windows::Win32::Foundation::{BOOL, LPARAM, RECT, TRUE};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFOEXW,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayInfo {
    pub id: u32,
    pub name: Option<String>,
    pub is_main: bool,
    pub bounds: DisplayBounds,
    pub backing_scale_factor: f64,
    pub pixel_width: u32,
    pub pixel_height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayBounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

struct MonitorEnumData {
    monitors: Vec<DisplayInfo>,
    index: u32,
}

pub fn get_displays() -> Result<Vec<DisplayInfo>, String> {
    let mut data = MonitorEnumData {
        monitors: Vec::new(),
        index: 0,
    };

    unsafe {
        let result = EnumDisplayMonitors(
            HDC::default(),
            None,
            Some(monitor_enum_callback),
            LPARAM(&mut data as *mut _ as isize),
        );

        if !result.as_bool() {
            return Err("EnumDisplayMonitors failed".to_string());
        }
    }

    if data.monitors.is_empty() {
        return Err("No displays found".to_string());
    }

    Ok(data.monitors)
}

unsafe extern "system" fn monitor_enum_callback(
    hmonitor: HMONITOR,
    _hdc: HDC,
    _rect: *mut RECT,
    lparam: LPARAM,
) -> BOOL {
    let data = &mut *(lparam.0 as *mut MonitorEnumData);

    let mut monitor_info: MONITORINFOEXW = mem::zeroed();
    monitor_info.monitorInfo.cbSize = mem::size_of::<MONITORINFOEXW>() as u32;

    if GetMonitorInfoW(hmonitor, &mut monitor_info.monitorInfo as *mut _).as_bool() {
        let rect = monitor_info.monitorInfo.rcMonitor;
        let width = (rect.right - rect.left) as f64;
        let height = (rect.bottom - rect.top) as f64;

        // Get DPI for this monitor
        let mut dpi_x: u32 = 96;
        let mut dpi_y: u32 = 96;
        let _ = GetDpiForMonitor(hmonitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
        let scale_factor = dpi_x as f64 / 96.0;

        // Check if this is the primary monitor
        let is_main = (monitor_info.monitorInfo.dwFlags & 0x1) != 0; // MONITORINFOF_PRIMARY

        // Get monitor name from device name
        let name = String::from_utf16_lossy(&monitor_info.szDevice)
            .trim_end_matches('\0')
            .to_string();

        // Use monitor handle as ID (cast to u32)
        let id = hmonitor.0 as u32;

        data.monitors.push(DisplayInfo {
            id,
            name: if name.is_empty() { None } else { Some(name) },
            is_main,
            bounds: DisplayBounds {
                x: rect.left as f64,
                y: rect.top as f64,
                width,
                height,
            },
            backing_scale_factor: scale_factor,
            // Pixel dimensions account for DPI scaling
            pixel_width: (width * scale_factor) as u32,
            pixel_height: (height * scale_factor) as u32,
        });

        data.index += 1;
    }

    TRUE
}

pub fn get_main_display() -> Result<DisplayInfo, String> {
    get_displays()?
        .into_iter()
        .find(|d| d.is_main)
        .ok_or_else(|| "No main display found".to_string())
}

#[derive(Debug, Clone)]
pub struct WindowBounds {
    pub x: f64,
    pub y: f64,
}

pub fn window_to_screen(bounds: &WindowBounds, x: f64, y: f64) -> (f64, f64) {
    (bounds.x + x, bounds.y + y)
}

pub fn screenshot_to_screen(bounds: &WindowBounds, scale: f64, px: f64, py: f64) -> (f64, f64) {
    (bounds.x + px / scale, bounds.y + py / scale)
}

/// Get virtual screen bounds (bounding rect of all monitors).
pub fn get_virtual_screen_bounds() -> (i32, i32, i32, i32) {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
        SM_YVIRTUALSCREEN,
    };

    unsafe {
        let x = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let y = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let width = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let height = GetSystemMetrics(SM_CYVIRTUALSCREEN);
        (x, y, width, height)
    }
}
