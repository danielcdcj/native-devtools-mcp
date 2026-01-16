//! Display configuration and coordinate conversion for macOS.

use core_graphics::display::{CGDisplay, CGMainDisplayID};
use serde::{Deserialize, Serialize};

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

pub fn get_displays() -> Result<Vec<DisplayInfo>, String> {
    let mut display_ids: Vec<u32> = vec![0; 16];
    let mut count: u32 = 0;

    let result = unsafe {
        core_graphics::display::CGGetActiveDisplayList(16, display_ids.as_mut_ptr(), &mut count)
    };
    if result != 0 {
        return Err(format!("Failed to get display list: {}", result));
    }

    display_ids.truncate(count as usize);
    let main_id = unsafe { CGMainDisplayID() };

    Ok(display_ids
        .into_iter()
        .map(|id| {
            let display = CGDisplay::new(id);
            let bounds = display.bounds();
            DisplayInfo {
                id,
                name: None,
                is_main: id == main_id,
                bounds: DisplayBounds {
                    x: bounds.origin.x,
                    y: bounds.origin.y,
                    width: bounds.size.width,
                    height: bounds.size.height,
                },
                backing_scale_factor: get_backing_scale_factor(id),
                pixel_width: display.pixels_wide() as u32,
                pixel_height: display.pixels_high() as u32,
            }
        })
        .collect())
}

pub fn get_main_display() -> Result<DisplayInfo, String> {
    get_displays()?
        .into_iter()
        .find(|d| d.is_main)
        .ok_or_else(|| "No main display found".to_string())
}

fn get_backing_scale_factor(display_id: u32) -> f64 {
    unsafe {
        use cocoa::base::{id, nil};
        use cocoa::foundation::NSArray;
        use objc::{msg_send, sel, sel_impl};

        let screens: id = msg_send![objc::class!(NSScreen), screens];
        for i in 0..NSArray::count(screens) {
            let screen: id = NSArray::objectAtIndex(screens, i);
            if screen == nil {
                continue;
            }

            let desc: id = msg_send![screen, deviceDescription];
            if desc == nil {
                continue;
            }

            let key: id =
                msg_send![objc::class!(NSString), stringWithUTF8String: c"NSScreenNumber".as_ptr()];
            let num: id = msg_send![desc, objectForKey: key];

            if num != nil {
                let screen_id: u32 = msg_send![num, unsignedIntValue];
                if screen_id == display_id {
                    return msg_send![screen, backingScaleFactor];
                }
            }
        }
    }
    2.0 // Default for Retina
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

pub fn backing_scale_for_point(x: f64, y: f64) -> f64 {
    get_displays()
        .ok()
        .and_then(|displays| {
            displays.into_iter().find(|d| {
                x >= d.bounds.x
                    && x < d.bounds.x + d.bounds.width
                    && y >= d.bounds.y
                    && y < d.bounds.y + d.bounds.height
            })
        })
        .map(|d| d.backing_scale_factor)
        .unwrap_or(2.0)
}
