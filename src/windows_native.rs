use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::{Mutex, OnceLock};

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Point {
    x: i32,
    y: i32,
}
#[repr(C)]
struct MinMax {
    reserved: Point,
    max_size: Point,
    max_position: Point,
    min_track: Point,
    max_track: Point,
}
type Hwnd = *mut c_void;
type Subclass = unsafe extern "system" fn(Hwnd, u32, usize, isize, usize, usize) -> isize;

#[link(name = "user32")]
extern "system" {
    fn GetWindowRect(window: Hwnd, rect: *mut Rect) -> i32;
    fn GetClientRect(window: Hwnd, rect: *mut Rect) -> i32;
    fn GetDpiForWindow(window: Hwnd) -> u32;
}
#[link(name = "comctl32")]
extern "system" {
    fn SetWindowSubclass(window: Hwnd, callback: Subclass, id: usize, data: usize) -> i32;
    fn RemoveWindowSubclass(window: Hwnd, callback: Subclass, id: usize) -> i32;
    fn DefSubclassProc(window: Hwnd, message: u32, wparam: usize, lparam: isize) -> isize;
}

#[derive(Clone, Copy)]
struct Target {
    height: f64,
    min_width: f64,
    top: Option<i32>,
}

fn targets() -> &'static Mutex<HashMap<isize, Target>> {
    static TARGETS: OnceLock<Mutex<HashMap<isize, Target>>> = OnceLock::new();
    TARGETS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn set_target(window: &tauri::WebviewWindow, height: f64, min_width: f64) {
    if let Ok(handle) = window.hwnd() {
        if let Ok(mut targets) = targets().lock() {
            targets.insert(
                handle.0 as isize,
                Target {
                    height,
                    min_width,
                    top: None,
                },
            );
        }
    }
}

pub fn install(window: &tauri::WebviewWindow) -> Result<(), String> {
    let handle = window
        .hwnd()
        .map_err(|_| "Could not initialize window sizing".to_owned())?;
    let installed = unsafe { SetWindowSubclass(handle.0 as Hwnd, resize_proc, 0x4d595354, 0) };
    if installed == 0 {
        return Err("Could not initialize window sizing".into());
    }
    Ok(())
}

fn physical_height(height: f64, dpi: u32, frame_height: i32) -> i32 {
    (height * f64::from(dpi.max(96)) / 96.0).round() as i32 + frame_height
}

fn constrain_drag(rect: &mut Rect, top: i32, height: i32, min_width: i32, edge: usize) {
    rect.top = top;
    rect.bottom = top + height;
    if rect.right - rect.left < min_width {
        if matches!(edge, 1 | 4 | 7) {
            rect.left = rect.right - min_width;
        } else {
            rect.right = rect.left + min_width;
        }
    }
}

unsafe extern "system" fn resize_proc(
    window: Hwnd,
    message: u32,
    wparam: usize,
    lparam: isize,
    _id: usize,
    _data: usize,
) -> isize {
    if message == 0x82 {
        if let Ok(mut targets) = targets().lock() {
            targets.remove(&(window as isize));
        }
        RemoveWindowSubclass(window, resize_proc, 0x4d595354);
        return DefSubclassProc(window, message, wparam, lparam);
    }
    let result = DefSubclassProc(window, message, wparam, lparam);
    let target = targets()
        .lock()
        .ok()
        .and_then(|targets| targets.get(&(window as isize)).copied());
    let Some(target) = target else {
        return result;
    };
    let mut outer = Rect::default();
    let mut client = Rect::default();
    if GetWindowRect(window, &mut outer) == 0 || GetClientRect(window, &mut client) == 0 {
        return result;
    }
    let dpi = GetDpiForWindow(window);
    let frame_height = outer.bottom - outer.top - (client.bottom - client.top);
    let frame_width = outer.right - outer.left - (client.right - client.left);
    let height = physical_height(target.height, dpi, frame_height);
    match message {
        0x231 => {
            if let Ok(mut targets) = targets().lock() {
                if let Some(target) = targets.get_mut(&(window as isize)) {
                    target.top = Some(outer.top);
                }
            }
        }
        0x214 if lparam != 0 => {
            let rect = &mut *(lparam as *mut Rect);
            let min_width =
                (target.min_width * f64::from(dpi.max(96)) / 96.0).round() as i32 + frame_width;
            constrain_drag(
                rect,
                target.top.unwrap_or(outer.top),
                height,
                min_width,
                wparam,
            );
            return 1;
        }
        0x24 if lparam != 0 => {
            let limits = &mut *(lparam as *mut MinMax);
            limits.min_track.y = height;
            limits.max_track.y = height;
            limits.min_track.x =
                (target.min_width * f64::from(dpi.max(96)) / 96.0).round() as i32 + frame_width;
        }
        0x84 => {
            return match result {
                13 | 16 => 10,
                14 | 17 => 11,
                12 | 15 => 18,
                _ => result,
            }
        }
        _ => {}
    }
    result
}

pub fn replace_file(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" {
        fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32;
    }
    let from: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 1 | 8) } == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn four_corners_and_lateral_edges_keep_client_height_at_each_dpi() {
        for dpi in [96, 120, 144] {
            for logical_height in [156.0, 220.0, 248.0] {
                for edge in [1, 2, 4, 5, 7, 8] {
                    let height = physical_height(logical_height, dpi, 9);
                    let mut rect = Rect {
                        left: 25,
                        top: -400,
                        right: 225,
                        bottom: 1200,
                    };
                    constrain_drag(&mut rect, 100, height, 320, edge);
                    assert_eq!(rect.top, 100);
                    assert_eq!(rect.bottom - rect.top, height);
                    assert_eq!(rect.right - rect.left, 320);
                }
            }
        }
    }
    #[test]
    fn client_heights_include_native_frame_at_each_dpi() {
        for (dpi, frame) in [(96, 9), (120, 11), (144, 14)] {
            for height in [156.0, 220.0, 264.0] {
                assert_eq!(
                    physical_height(height, dpi, frame) - frame,
                    (height * f64::from(dpi) / 96.0).round() as i32
                );
            }
        }
    }
}
