//! Win32 stealth-minimize, Taskbar/Tray visibility, DWM dark styling, clipboard, and single-instance.

use std::ffi::c_void;
use std::sync::Arc;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::thread::{self, JoinHandle};

use windows::Win32::Foundation::{
    CloseHandle, ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HGLOBAL, HLOCAL,
    HWND, LPARAM, LocalFree, RECT, WAIT_OBJECT_0, WPARAM,
};
use windows::Win32::Graphics::Dwm::{
    DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DWMWA_CLOAKED, DWMWA_TEXT_COLOR,
    DWMWA_USE_IMMERSIVE_DARK_MODE, DwmGetWindowAttribute, DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR};
use windows::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;
use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::CF_UNICODETEXT;
use windows::Win32::System::Threading::{
    CreateEventW, CreateMutexW, EVENT_MODIFY_STATE, INFINITE, OpenEventW, SetEvent,
    WaitForMultipleObjects,
};
use windows::Win32::UI::HiDpi::{PROCESS_PER_MONITOR_DPI_AWARE, SetProcessDpiAwareness};
use windows::Win32::UI::WindowsAndMessaging::{
    ASFW_ANY, AllowSetForegroundWindow, BringWindowToTop, GW_HWNDNEXT, GWL_EXSTYLE, GetWindow,
    GetWindowLongPtrW, GetWindowRect, HWND_BOTTOM, HWND_NOTOPMOST, HWND_TOP, HWND_TOPMOST,
    IsIconic, IsWindowVisible, PostMessageW, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, SWP_NOZORDER, SWP_SHOWWINDOW, SetForegroundWindow, SetProcessDPIAware,
    SetWindowLongPtrW, SetWindowPos, WM_APP, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
};
use windows::core::{BOOL, PCWSTR};

pub const WM_APP_WAKE: u32 = WM_APP + 2;

const DEFAULT_MUTEX_NAME: &str = "Local\\StreamTimer_SingleInstance_Mutex_v1";
const DEFAULT_EVENT_NAME: &str = "Local\\StreamTimer_ShowEvent_v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MinimizeMode {
    BackgroundWgc,
    TrayWgc,
    TrayCornerPeek,
    CornerPeek,
    Native,
}

impl MinimizeMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BackgroundWgc => "background_wgc",
            Self::TrayWgc => "tray_wgc",
            Self::TrayCornerPeek => "tray_corner_peek",
            Self::CornerPeek => "corner_peek",
            Self::Native => "native",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "background_wgc" => Some(Self::BackgroundWgc),
            "tray_wgc" => Some(Self::TrayWgc),
            "tray_corner_peek" => Some(Self::TrayCornerPeek),
            "corner_peek" => Some(Self::CornerPeek),
            "native" => Some(Self::Native),
            _ => None,
        }
    }

    pub fn uses_tray(self) -> bool {
        matches!(self, Self::TrayWgc | Self::TrayCornerPeek)
    }

    pub fn uses_corner_peek(self) -> bool {
        matches!(self, Self::CornerPeek | Self::TrayCornerPeek)
    }
}

pub fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn enable_per_monitor_dpi() {
    unsafe {
        if SetProcessDpiAwareness(PROCESS_PER_MONITOR_DPI_AWARE).is_err() {
            let _ = SetProcessDPIAware();
        }
    }
}

pub fn apply_dark_titlebar(hwnd: HWND) {
    let dark: u32 = 1;
    let black: u32 = 0x0000_0000;
    let muted_text: u32 = 0x0088_8888;
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &dark as *const u32 as *const c_void,
            size_of::<u32>() as u32,
        );
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_CAPTION_COLOR,
            &black as *const u32 as *const c_void,
            size_of::<u32>() as u32,
        );
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            &black as *const u32 as *const c_void,
            size_of::<u32>() as u32,
        );
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_TEXT_COLOR,
            &muted_text as *const u32 as *const c_void,
            size_of::<u32>() as u32,
        );
    }
}

pub fn get_window_rect(hwnd: HWND) -> (i32, i32, i32, i32) {
    let mut rc = RECT::default();
    unsafe {
        if GetWindowRect(hwnd, &mut rc).is_ok() {
            return (rc.left, rc.top, rc.right, rc.bottom);
        }
    }
    (0, 0, 0, 0)
}

unsafe extern "system" fn enum_mon_proc(
    _hmon: HMONITOR,
    _hdc: HDC,
    lprc: *mut RECT,
    lparam: LPARAM,
) -> BOOL {
    let list = unsafe { &mut *(lparam.0 as *mut Vec<RECT>) };
    if !lprc.is_null() {
        list.push(unsafe { *lprc });
    }
    BOOL(1)
}

pub fn get_corner_peek_position() -> (i32, i32) {
    let mut monitors: Vec<RECT> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(enum_mon_proc),
            LPARAM(&mut monitors as *mut Vec<RECT> as isize),
        );
    }
    if let Some(best) = monitors.into_iter().max_by_key(|r| r.right + r.bottom) {
        (best.right - 2, best.bottom - 2)
    } else {
        (1918, 1078)
    }
}

pub fn set_taskbar_hidden(hwnd: HWND, hidden: bool) {
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let tool = WS_EX_TOOLWINDOW.0 as isize;
        let app = WS_EX_APPWINDOW.0 as isize;
        let new_ex = if hidden {
            (ex | tool) & !app
        } else {
            (ex & !tool) | app
        };
        let _ = SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new_ex);
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
    }
}

fn is_window_cloaked(hwnd: HWND) -> bool {
    let mut cloaked: u32 = 0;
    let hr = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            &mut cloaked as *mut u32 as *mut c_void,
            size_of::<u32>() as u32,
        )
    };
    hr.is_ok() && cloaked != 0
}

pub fn focus_next_window(exclude: HWND) {
    unsafe {
        let mut curr = GetWindow(exclude, GW_HWNDNEXT).unwrap_or_default();
        while !curr.is_invalid() {
            let ex = GetWindowLongPtrW(curr, GWL_EXSTYLE) as u32;
            if IsWindowVisible(curr).as_bool()
                && !IsIconic(curr).as_bool()
                && (ex & WS_EX_TOOLWINDOW.0) == 0
                && !is_window_cloaked(curr)
            {
                let mut rc = RECT::default();
                if GetWindowRect(curr, &mut rc).is_ok()
                    && (rc.right - rc.left) > 50
                    && (rc.bottom - rc.top) > 50
                {
                    let _ = SetForegroundWindow(curr);
                    return;
                }
            }
            curr = GetWindow(curr, GW_HWNDNEXT).unwrap_or_default();
        }
    }
}

pub fn send_to_background(hwnd: HWND, move_to: Option<(i32, i32)>) {
    unsafe {
        let (x, y, flags) = match move_to {
            Some((px, py)) => (px, py, SWP_NOSIZE | SWP_NOACTIVATE),
            None => (0, 0, SWP_NOSIZE | SWP_NOMOVE | SWP_NOACTIVATE),
        };
        let _ = SetWindowPos(hwnd, Some(HWND_BOTTOM), x, y, 0, 0, flags);
        focus_next_window(hwnd);
    }
}

pub fn restore_from_background(hwnd: HWND, move_to: Option<(i32, i32)>, always_on_top: bool) {
    unsafe {
        let insert_after = if always_on_top {
            HWND_TOPMOST
        } else {
            HWND_TOP
        };
        let (x, y, flags) = match move_to {
            Some((px, py)) => (px, py, SWP_NOSIZE | SWP_SHOWWINDOW),
            None => (0, 0, SWP_NOSIZE | SWP_NOMOVE | SWP_SHOWWINDOW),
        };
        let _ = SetWindowPos(hwnd, Some(insert_after), x, y, 0, 0, flags);
        let _ = BringWindowToTop(hwnd);
        let _ = SetForegroundWindow(hwnd);
    }
}

pub fn set_always_on_top(hwnd: HWND, enabled: bool) {
    unsafe {
        let insert_after = if enabled {
            HWND_TOPMOST
        } else {
            HWND_NOTOPMOST
        };
        let _ = SetWindowPos(
            hwnd,
            Some(insert_after),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}

pub fn copy_to_clipboard(hwnd: HWND, text: &str) -> bool {
    let wide = to_wide(text);
    let byte_len = wide.len() * size_of::<u16>();
    unsafe {
        if OpenClipboard(Some(hwnd)).is_err() {
            return false;
        }
        let _ = EmptyClipboard();
        let Ok(hmem): windows::core::Result<HGLOBAL> = GlobalAlloc(GMEM_MOVEABLE, byte_len) else {
            let _ = CloseClipboard();
            return false;
        };
        let ptr = GlobalLock(hmem) as *mut u16;
        if ptr.is_null() {
            let _ = CloseClipboard();
            return false;
        }
        std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
        let _ = GlobalUnlock(hmem);
        let ok = SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(hmem.0))).is_ok();
        let _ = CloseClipboard();
        ok
    }
}

struct SecurityDescGuard(PSECURITY_DESCRIPTOR);

impl Drop for SecurityDescGuard {
    fn drop(&mut self) {
        if !self.0.0.is_null() {
            unsafe {
                let _ = LocalFree(Some(HLOCAL(self.0.0)));
            }
        }
    }
}

fn build_shared_sa() -> Option<(SecurityDescGuard, SECURITY_ATTRIBUTES)> {
    let sddl = to_wide("D:(A;;GA;;;AU)S:(ML;;NW;;;LW)");
    let mut sd = PSECURITY_DESCRIPTOR::default();
    unsafe {
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl.as_ptr()),
            1,
            &mut sd,
            None,
        )
        .is_ok()
        {
            let sa = SECURITY_ATTRIBUTES {
                nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: sd.0,
                bInheritHandle: false.into(),
            };
            return Some((SecurityDescGuard(sd), sa));
        }
    }
    None
}

pub enum InstanceAcquisition {
    Primary(SingleInstanceGuard),
    SecondarySignaled,
}

pub struct SingleInstanceGuard {
    mutex_handle: HANDLE,
    show_event: HANDLE,
    stop_event: HANDLE,
    target_hwnd: Arc<AtomicIsize>,
    listener: Option<JoinHandle<()>>,
}

impl SingleInstanceGuard {
    pub fn set_target_hwnd(&mut self, hwnd: HWND) {
        self.target_hwnd.store(hwnd.0 as isize, Ordering::Release);
        if self.listener.is_some() || self.show_event.is_invalid() || self.stop_event.is_invalid() {
            return;
        }
        let raw_show = self.show_event.0 as isize;
        let raw_stop = self.stop_event.0 as isize;
        let hwnd_ref = Arc::clone(&self.target_hwnd);
        self.listener = Some(thread::spawn(move || {
            let handles = [
                HANDLE(raw_show as *mut c_void),
                HANDLE(raw_stop as *mut c_void),
            ];
            loop {
                let res = unsafe { WaitForMultipleObjects(&handles, false, INFINITE) };
                if res == WAIT_OBJECT_0 {
                    let h = hwnd_ref.load(Ordering::Acquire);
                    if h != 0 {
                        unsafe {
                            let _ = PostMessageW(
                                Some(HWND(h as *mut c_void)),
                                WM_APP_WAKE,
                                WPARAM(0),
                                LPARAM(0),
                            );
                        }
                    }
                } else {
                    break;
                }
            }
        }));
    }
}

impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        if !self.stop_event.is_invalid() {
            unsafe {
                let _ = SetEvent(self.stop_event);
            }
        }
        if let Some(h) = self.listener.take() {
            let _ = h.join();
        }
        unsafe {
            if !self.show_event.is_invalid() {
                let _ = CloseHandle(self.show_event);
            }
            if !self.stop_event.is_invalid() {
                let _ = CloseHandle(self.stop_event);
            }
            if !self.mutex_handle.is_invalid() {
                let _ = CloseHandle(self.mutex_handle);
            }
        }
    }
}

pub fn acquire_or_wake_existing() -> InstanceAcquisition {
    acquire_or_wake_named(DEFAULT_MUTEX_NAME, DEFAULT_EVENT_NAME)
}

pub fn acquire_or_wake_named(mutex_name: &str, event_name: &str) -> InstanceAcquisition {
    let mutex_w = to_wide(mutex_name);
    let event_w = to_wide(event_name);
    let sec = build_shared_sa();
    let sa_ptr = sec.as_ref().map(|(_, sa)| sa as *const SECURITY_ATTRIBUTES);

    unsafe {
        let mutex_res = CreateMutexW(sa_ptr, false, PCWSTR(mutex_w.as_ptr()));
        let last_err = GetLastError();
        let already = match &mutex_res {
            Ok(_) => last_err == ERROR_ALREADY_EXISTS,
            Err(_) => last_err == ERROR_ACCESS_DENIED || last_err == ERROR_ALREADY_EXISTS,
        };

        if already {
            if let Ok(h) = mutex_res
                && !h.is_invalid()
            {
                let _ = CloseHandle(h);
            }
            let _ = AllowSetForegroundWindow(ASFW_ANY);
            if let Ok(ev) = OpenEventW(EVENT_MODIFY_STATE, false, PCWSTR(event_w.as_ptr())) {
                let _ = SetEvent(ev);
                let _ = CloseHandle(ev);
            }
            return InstanceAcquisition::SecondarySignaled;
        }

        let mutex_handle = mutex_res.unwrap_or_default();
        let show_event =
            CreateEventW(sa_ptr, false, false, PCWSTR(event_w.as_ptr())).unwrap_or_default();
        let stop_event = CreateEventW(None, true, false, PCWSTR::null()).unwrap_or_default();

        InstanceAcquisition::Primary(SingleInstanceGuard {
            mutex_handle,
            show_event,
            stop_event,
            target_hwnd: Arc::new(AtomicIsize::new(0)),
            listener: None,
        })
    }
}
