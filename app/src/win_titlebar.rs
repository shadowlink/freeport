//! Windows-only fine polish for the frameless window:
//!  - Win11 rounded corners (DWM).
//!  - Drop shadow for the borderless window (DWM frame extension).
//!  - Snap Layouts: report `HTMAXBUTTON` over our custom maximize button via a
//!    chained window subclass, so hovering it shows Windows 11's snap flyout and
//!    clicking it maximizes/restores.
//!
//! Everything is done through a non-destructive `SetWindowSubclass` that chains
//! to winit's own window procedure via `DefSubclassProc`, so winit keeps working.
//!
//! Keep the button geometry in sync with the titlebar in `ui/app.slint`:
//! titlebar height 40, buttons 40×28 vertically centered (top = 6), right
//! padding 6, spacing 6, order [minimize, maximize, close]. So, from the right
//! edge: close = [W-46, W-6], maximize = [W-92, W-52], y = [6, 34] (logical px).

use core::ffi::c_void;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::{
    DwmExtendFrameIntoClientArea, DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE,
    DWMWCP_ROUND,
};
use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
use windows_sys::Win32::UI::Controls::MARGINS;
use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetClientRect, IsZoomed, ShowWindow, HTMAXBUTTON, SW_MAXIMIZE, SW_RESTORE, WM_NCLBUTTONDOWN,
    WM_NCLBUTTONUP,
};

// WM_NCHITTEST isn't re-exported under every windows-sys version's messaging
// feature set; define it locally (its value is stable Win32 ABI).
const WM_NCHITTEST: u32 = 0x0084;

const SUBCLASS_ID: usize = 0xF00D;

/// Apply DWM polish and install the snap-layout subclass. `hwnd_isize` is the
/// raw Win32 HWND (as delivered by raw-window-handle's `NonZeroIsize`).
pub fn setup(hwnd_isize: isize) {
    let hwnd = hwnd_isize as HWND;
    if hwnd.is_null() {
        return;
    }
    unsafe {
        // Win11 rounded corners.
        let pref: i32 = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
            &pref as *const i32 as *const c_void,
            core::mem::size_of::<i32>() as u32,
        );
        // Drop shadow on the borderless window (1px frame extension).
        let margins = MARGINS {
            cxLeftWidth: 0,
            cxRightWidth: 0,
            cyTopHeight: 1,
            cyBottomHeight: 0,
        };
        let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);
        // Chain a subclass for the snap-layout hit-test.
        let _ = SetWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID, 0);
    }
}

/// True if the given screen point falls inside our maximize button.
unsafe fn over_maximize_button(hwnd: HWND, lparam: LPARAM) -> bool {
    // NCHITTEST packs screen coords in lparam (x = low word, y = high word).
    let raw = lparam as i32;
    let mut pt = POINT {
        x: (raw & 0xFFFF) as i16 as i32,
        y: ((raw >> 16) & 0xFFFF) as i16 as i32,
    };
    if ScreenToClient(hwnd, &mut pt) == 0 {
        return false;
    }
    let mut rc = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    if GetClientRect(hwnd, &mut rc) == 0 {
        return false;
    }
    let dpi = GetDpiForWindow(hwnd);
    let scale = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };
    let w = rc.right; // client width in physical px
    let x0 = w - (92.0 * scale) as i32;
    let x1 = w - (52.0 * scale) as i32;
    let y0 = (6.0 * scale) as i32;
    let y1 = (34.0 * scale) as i32;
    pt.x >= x0 && pt.x < x1 && pt.y >= y0 && pt.y < y1
}

unsafe extern "system" fn subclass_proc(
    hwnd: HWND,
    umsg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    match umsg {
        WM_NCHITTEST => {
            let hit = DefSubclassProc(hwnd, umsg, wparam, lparam);
            if over_maximize_button(hwnd, lparam) {
                return HTMAXBUTTON as LRESULT;
            }
            hit
        }
        WM_NCLBUTTONDOWN if wparam == HTMAXBUTTON as WPARAM => {
            // Swallow the press so Windows lets the snap flyout drive; act on up.
            0
        }
        WM_NCLBUTTONUP if wparam == HTMAXBUTTON as WPARAM => {
            if IsZoomed(hwnd) != 0 {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            } else {
                let _ = ShowWindow(hwnd, SW_MAXIMIZE);
            }
            0
        }
        _ => DefSubclassProc(hwnd, umsg, wparam, lparam),
    }
}
