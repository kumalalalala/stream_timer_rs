//! Pure Win32 minimalist 00:00:00 stream timer window with DirectWrite, Tray, and Stealth.

use std::ffi::c_void;
use std::path::PathBuf;
use std::time::Instant;

use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateSolidBrush, EndPaint, HBRUSH, InvalidateRect, PAINTSTRUCT, UpdateWindow,
};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW};
use windows::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForSystem};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, RegisterHotKey, UnregisterHotKey,
    VK_CONTROL,
};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CW_USEDEFAULT, CheckMenuItem, CheckMenuRadioItem, CreateIcon, CreatePopupMenu,
    CreateWindowExW, DefWindowProcW, DestroyMenu, DispatchMessageW, GWLP_USERDATA, GetClientRect,
    GetCursorPos, GetMessageW, HICON, HMENU, IDC_ARROW, KillTimer, LoadCursorW, LoadIconW,
    MF_BYCOMMAND, MF_CHECKED, MF_POPUP, MF_SEPARATOR, MF_STRING, MF_UNCHECKED, MINMAXINFO, MSG,
    PostMessageW, PostQuitMessage, RegisterClassExW, SC_MINIMIZE, SW_MINIMIZE, SW_SHOW, SetTimer,
    SetWindowLongPtrW, SetWindowTextW, TPM_BOTTOMALIGN, TPM_LEFTALIGN, TPM_RIGHTBUTTON,
    TrackPopupMenu, TranslateMessage, WA_ACTIVE, WA_CLICKACTIVE, WM_ACTIVATE, WM_APP, WM_CLOSE,
    WM_COMMAND, WM_CONTEXTMENU, WM_CREATE, WM_DESTROY, WM_ERASEBKGND, WM_GETMINMAXINFO, WM_HOTKEY,
    WM_KEYDOWN, WM_LBUTTONUP, WM_NCCREATE, WM_NULL, WM_PAINT, WM_RBUTTONUP, WM_SIZE, WM_SYSCOMMAND,
    WM_TIMER, WNDCLASSEXW, WS_EX_APPWINDOW, WS_OVERLAPPEDWINDOW,
};
use windows::core::PCWSTR;

use crate::browser_server::BrowserTimerServer;
use crate::icon::render_stream_timer_rgba;
use crate::renderer::DirectWriteRenderer;
use crate::session_store::SessionStore;
use crate::timer_engine::TimerEngine;
use crate::win32_stealth::{
    MinimizeMode, SingleInstanceGuard, WM_APP_WAKE, apply_dark_titlebar, copy_to_clipboard,
    focus_next_window, get_corner_peek_position, get_window_rect, restore_from_background,
    send_to_background, set_always_on_top, set_taskbar_hidden, set_tool_window_style, to_wide,
};

const BASE_TITLE: &str = "Stream Timer";
const CLASS_NAME: &str = "StreamTimerWindowClass";

const WM_TRAYICON: u32 = WM_APP + 1;
const TIMER_TICK_ID: usize = 1;
const TIMER_TITLE_ID: usize = 2;
const GLOBAL_TRAY_HOTKEY_ID: i32 = 0x5354;

const IDM_PAUSE: usize = 1001;
const IDM_RESET: usize = 1002;
const IDM_MINIMIZE_SAFE: usize = 1003;
const IDM_MINIMIZE_TRAY: usize = 1004;
const IDM_SHOW_WINDOW: usize = 1005;

const IDM_MODE_BG_WGC: usize = 1010;
const IDM_MODE_TRAY_WGC: usize = 1011;
const IDM_MODE_TRAY_CORNER: usize = 1012;
const IDM_MODE_CORNER: usize = 1013;
const IDM_MODE_NATIVE: usize = 1014;

const IDM_ON_TOP: usize = 1020;
const IDM_COPY_URL: usize = 1021;
const IDM_COPY_URL_TRANSPARENT: usize = 1022;
const IDM_RESUME_SESSION: usize = 1023;
const IDM_HIDE_ALT_TAB: usize = 1024;
const IDM_QUIT: usize = 1099;

pub struct AppConfig {
    pub minimize_mode: MinimizeMode,
    pub preferred_port: u16,
    pub width: i32,
    pub height: i32,
    pub session_dir: Option<PathBuf>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            minimize_mode: MinimizeMode::TrayWgc,
            preferred_port: 18080,
            width: 440,
            height: 140,
            session_dir: None,
        }
    }
}

pub struct StreamTimerState {
    pub hwnd: HWND,
    pub engine: TimerEngine,
    pub session_store: SessionStore,
    pub server: BrowserTimerServer,
    pub browser_url: String,
    pub renderer: DirectWriteRenderer,
    pub minimize_mode: MinimizeMode,
    pub always_on_top: bool,
    pub hide_from_alt_tab: bool,
    pub is_stealth_minimized: bool,
    pub in_tray: bool,
    pub was_corner_peek: bool,
    pub saved_pos: Option<(i32, i32)>,
    pub last_text: String,
    pub suppress_activate_until: Option<Instant>,
    pub tray_added: bool,
    pub hotkey_registered: bool,
    pub single_instance: Option<SingleInstanceGuard>,
    pub bg_brush: HBRUSH,
    pub hicon: HICON,
}

impl StreamTimerState {
    fn set_title(&self, title: &str) {
        let wide = to_wide(title);
        unsafe {
            let _ = SetWindowTextW(self.hwnd, PCWSTR(wide.as_ptr()));
        }
    }

    fn flash_title(&self, suffix: &str) {
        self.flash_title_ms(suffix, 1600);
    }

    fn flash_title_ms(&self, suffix: &str, duration_ms: u32) {
        self.set_title(&format!("{BASE_TITLE} - {suffix}"));
        unsafe {
            let _ = SetTimer(Some(self.hwnd), TIMER_TITLE_ID, duration_ms, None);
        }
    }

    fn restore_default_title(&self) {
        let snap = self.engine.snapshot();
        if snap.running {
            self.set_title(BASE_TITLE);
        } else {
            self.set_title(&format!("{BASE_TITLE} [Paused]"));
        }
    }

    fn sync_and_schedule_tick(&mut self, force_paint: bool) {
        let snap = self.engine.snapshot();
        self.session_store.save_if_second_changed(snap.elapsed_ms);
        if snap.formatted != self.last_text || force_paint {
            self.last_text = snap.formatted;
            unsafe {
                let _ = InvalidateRect(Some(self.hwnd), None, false);
                let _ = UpdateWindow(self.hwnd);
            }
        }
        let delay_ms = if snap.running {
            let ms_to_next = 1000 - (snap.elapsed_ms % 1000);
            (ms_to_next + 5).clamp(25, 250) as u32
        } else {
            200
        };
        unsafe {
            let _ = SetTimer(Some(self.hwnd), TIMER_TICK_ID, delay_ms, None);
        }
    }

    pub fn resume_previous_session(&mut self) {
        if let Some(prev_ms) = self.session_store.consume_resume_ms() {
            let snap = self.engine.set_elapsed_ms(prev_ms);
            self.session_store.force_save(snap.elapsed_ms);
            self.last_text = snap.formatted;
            unsafe {
                let _ = InvalidateRect(Some(self.hwnd), None, false);
                let _ = UpdateWindow(self.hwnd);
            }
            self.flash_title(&format!("Resumed {}", self.last_text));
        } else if self.session_store.is_resumed() {
            self.flash_title("Already Resumed");
        } else {
            self.flash_title("No Previous Session");
        }
    }

    pub fn toggle_pause(&mut self) {
        let snap = self.engine.toggle();
        self.session_store.force_save(snap.elapsed_ms);
        self.last_text = snap.formatted;
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
            let _ = UpdateWindow(self.hwnd);
        }
        self.restore_default_title();
    }

    pub fn reset_timer(&mut self) {
        let snap = self.engine.reset(true);
        self.session_store.force_save(snap.elapsed_ms);
        self.last_text = snap.formatted;
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
            let _ = UpdateWindow(self.hwnd);
        }
        self.flash_title("Reset");
    }

    pub fn toggle_always_on_top(&mut self) {
        self.always_on_top = !self.always_on_top;
        set_always_on_top(self.hwnd, self.always_on_top);
        self.flash_title(if self.always_on_top {
            "On Top"
        } else {
            "Normal Z-Order"
        });
    }

    pub fn copy_url(&self, transparent: bool) {
        let url = if transparent {
            format!("{}?bg=transparent", self.browser_url)
        } else {
            self.browser_url.clone()
        };
        if copy_to_clipboard(self.hwnd, &url) {
            self.flash_title(&format!("Copied {url}"));
        }
    }

    pub fn trigger_minimize(&mut self) {
        if self.minimize_mode == MinimizeMode::Native {
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::ShowWindow(
                    self.hwnd,
                    SW_MINIMIZE,
                );
            }
            return;
        }
        self.enter_stealth_minimize();
    }

    pub fn minimize_to_tray(&mut self) {
        let use_corner = self.minimize_mode.uses_corner_peek();
        self.enter_stealth(true, use_corner);
    }

    pub fn enter_stealth_minimize(&mut self) {
        self.enter_stealth(
            self.minimize_mode.uses_tray(),
            self.minimize_mode.uses_corner_peek(),
        );
    }

    fn enter_stealth(&mut self, use_tray: bool, use_corner_peek: bool) {
        if self.is_stealth_minimized {
            if use_tray && !self.in_tray {
                self.in_tray = true;
                set_taskbar_hidden(self.hwnd, true, self.hide_from_alt_tab);
            }
            return;
        }
        let rect = get_window_rect(self.hwnd);
        self.saved_pos = Some((rect.0, rect.1));
        self.is_stealth_minimized = true;
        self.in_tray = use_tray;
        self.was_corner_peek = use_corner_peek;
        self.suppress_activate_until =
            Some(Instant::now() + std::time::Duration::from_millis(450));

        if use_tray {
            set_taskbar_hidden(self.hwnd, true, self.hide_from_alt_tab);
        }

        if use_corner_peek {
            send_to_background(self.hwnd, Some(get_corner_peek_position()));
        } else {
            send_to_background(self.hwnd, None);
        }
    }

    pub fn exit_stealth_minimize(&mut self) {
        if !self.is_stealth_minimized {
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::ShowWindow(self.hwnd, SW_SHOW);
                let _ = windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow(self.hwnd);
            }
            return;
        }
        let move_to = if self.was_corner_peek {
            self.saved_pos
        } else {
            None
        };
        let was_tray = self.in_tray;
        self.is_stealth_minimized = false;
        self.in_tray = false;
        self.was_corner_peek = false;
        self.suppress_activate_until =
            Some(Instant::now() + std::time::Duration::from_millis(200));

        if was_tray {
            set_taskbar_hidden(self.hwnd, false, self.hide_from_alt_tab);
        }
        restore_from_background(self.hwnd, move_to, self.always_on_top);

        let mut rc = RECT::default();
        unsafe {
            if GetClientRect(self.hwnd, &mut rc).is_ok() {
                self.renderer.recompute_font_size(rc.right - rc.left, rc.bottom - rc.top);
                let _ = InvalidateRect(Some(self.hwnd), None, false);
            }
        }
    }

    fn show_context_menu(&self, x: i32, y: i32, is_tray: bool) {
        unsafe {
            let Ok(menu) = CreatePopupMenu() else {
                return;
            };
            if is_tray {
                if self.is_stealth_minimized {
                    append_item(menu, IDM_SHOW_WINDOW, "Hiện lại cửa sổ");
                } else {
                    append_item(menu, IDM_MINIMIZE_TRAY, "Thu vào khay hệ thống (H)");
                }
                let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            }

            if let Some(prev) = self.session_store.previous_candidate() {
                append_item(
                    menu,
                    IDM_RESUME_SESSION,
                    &format!("Tiếp tục phiên trước ({})  (Ctrl+S)", prev.formatted),
                );
                let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            }

            append_item(menu, IDM_PAUSE, "Pause / Resume (Space)");
            append_item(menu, IDM_RESET, "Reset 00:00:00 (R)");
            append_item(menu, IDM_MINIMIZE_TRAY, "Thu vào khay hệ thống (H / Ctrl+Shift+H)");
            append_item(menu, IDM_MINIMIZE_SAFE, "Minimize Stream-Safe (M)");
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());

            if let Ok(mode_sub) = CreatePopupMenu() {
                append_item(mode_sub, IDM_MODE_BG_WGC, "Background (WGC - giữ Taskbar)");
                append_item(mode_sub, IDM_MODE_TRAY_WGC, "System Tray (WGC - ẩn khỏi Taskbar)");
                append_item(
                    mode_sub,
                    IDM_MODE_TRAY_CORNER,
                    "System Tray + Corner Peek 2px (BitBlt)",
                );
                append_item(
                    mode_sub,
                    IDM_MODE_CORNER,
                    "Corner Peek 2px (BitBlt - giữ Taskbar)",
                );
                append_item(
                    mode_sub,
                    IDM_MODE_NATIVE,
                    "Native Minimize (chỉ dùng Browser Source)",
                );
                let active_id = match self.minimize_mode {
                    MinimizeMode::BackgroundWgc => IDM_MODE_BG_WGC,
                    MinimizeMode::TrayWgc => IDM_MODE_TRAY_WGC,
                    MinimizeMode::TrayCornerPeek => IDM_MODE_TRAY_CORNER,
                    MinimizeMode::CornerPeek => IDM_MODE_CORNER,
                    MinimizeMode::Native => IDM_MODE_NATIVE,
                };
                let _ = CheckMenuRadioItem(
                    mode_sub,
                    IDM_MODE_BG_WGC as u32,
                    IDM_MODE_NATIVE as u32,
                    active_id as u32,
                    MF_BYCOMMAND.0,
                );
                let sub_title = to_wide("Minimize Mode");
                let _ = AppendMenuW(
                    menu,
                    MF_POPUP,
                    mode_sub.0 as usize,
                    PCWSTR(sub_title.as_ptr()),
                );
            }

            append_item(menu, IDM_ON_TOP, "Always on Top (T)");
            let _ = CheckMenuItem(
                menu,
                IDM_ON_TOP as u32,
                (MF_BYCOMMAND | if self.always_on_top { MF_CHECKED } else { MF_UNCHECKED }).0,
            );

            append_item(
                menu,
                IDM_HIDE_ALT_TAB,
                "Ẩn khỏi Alt+Tab khi ở khay (chọn nguồn OBS trước)",
            );
            let _ = CheckMenuItem(
                menu,
                IDM_HIDE_ALT_TAB as u32,
                (MF_BYCOMMAND
                    | if self.hide_from_alt_tab {
                        MF_CHECKED
                    } else {
                        MF_UNCHECKED
                    })
                .0,
            );

            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            append_item(
                menu,
                IDM_COPY_URL,
                &format!("Copy Browser Source URL ({})", self.browser_url),
            );
            append_item(menu, IDM_COPY_URL_TRANSPARENT, "Copy Transparent Browser URL");
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            append_item(menu, IDM_QUIT, "Quit (Esc)");

            if is_tray {
                let _ = windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow(self.hwnd);
            }
            let _ = TrackPopupMenu(
                menu,
                TPM_LEFTALIGN | TPM_BOTTOMALIGN | TPM_RIGHTBUTTON,
                x,
                y,
                None,
                self.hwnd,
                None,
            );
            if is_tray {
                let _ = PostMessageW(Some(self.hwnd), WM_NULL, WPARAM(0), LPARAM(0));
            }
            let _ = DestroyMenu(menu);
        }
    }

    fn handle_command(&mut self, cmd_id: usize) {
        match cmd_id {
            IDM_PAUSE => self.toggle_pause(),
            IDM_RESET => self.reset_timer(),
            IDM_RESUME_SESSION => self.resume_previous_session(),
            IDM_MINIMIZE_SAFE => self.trigger_minimize(),
            IDM_MINIMIZE_TRAY => self.minimize_to_tray(),
            IDM_SHOW_WINDOW => self.exit_stealth_minimize(),
            IDM_MODE_BG_WGC => {
                self.minimize_mode = MinimizeMode::BackgroundWgc;
                self.flash_title("Mode: background_wgc");
            }
            IDM_MODE_TRAY_WGC => {
                self.minimize_mode = MinimizeMode::TrayWgc;
                self.flash_title("Mode: tray_wgc");
            }
            IDM_MODE_TRAY_CORNER => {
                self.minimize_mode = MinimizeMode::TrayCornerPeek;
                self.flash_title("Mode: tray_corner_peek");
            }
            IDM_MODE_CORNER => {
                self.minimize_mode = MinimizeMode::CornerPeek;
                self.flash_title("Mode: corner_peek");
            }
            IDM_MODE_NATIVE => {
                self.minimize_mode = MinimizeMode::Native;
                self.flash_title("Mode: native");
            }
            IDM_ON_TOP => self.toggle_always_on_top(),
            IDM_HIDE_ALT_TAB => {
                self.hide_from_alt_tab = !self.hide_from_alt_tab;
                if self.in_tray {
                    set_tool_window_style(self.hwnd, self.hide_from_alt_tab);
                }
                self.flash_title(if self.hide_from_alt_tab {
                    "Alt+Tab: Hidden in Tray"
                } else {
                    "Alt+Tab: Always Capturable"
                });
            }
            IDM_COPY_URL => self.copy_url(false),
            IDM_COPY_URL_TRANSPARENT => self.copy_url(true),
            IDM_QUIT => unsafe {
                let _ = PostMessageW(Some(self.hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            },
            _ => {}
        }
    }
}

unsafe fn append_item(menu: HMENU, id: usize, text: &str) {
    let wide = to_wide(text);
    unsafe {
        let _ = AppendMenuW(menu, MF_STRING, id, PCWSTR(wide.as_ptr()));
    }
}

/// Enable Windows 10/11 dark context menus via uxtheme ordinal 135 (`SetPreferredAppMode`).
fn enable_dark_menus() {
    unsafe {
        let dll = to_wide("uxtheme.dll");
        if let Ok(module) = LoadLibraryW(PCWSTR(dll.as_ptr()))
            && let Some(proc) = GetProcAddress(
                module,
                windows::core::PCSTR(std::ptr::without_provenance::<u8>(135)),
            )
        {
            let set_mode: unsafe extern "system" fn(i32) -> i32 = std::mem::transmute(proc);
            let _ = set_mode(2); // ForceDark
        }
    }
}

fn load_app_icon(hinst: HINSTANCE) -> HICON {
    unsafe {
        if let Ok(icon) = LoadIconW(
            Some(hinst),
            PCWSTR(std::ptr::without_provenance::<u16>(1)),
        ) && !icon.is_invalid()
        {
            return icon;
        }
        // Fallback procedural icon (e.g. inside test harness without compiled .rc)
        let size = 32i32;
        let rgba = render_stream_timer_rgba(size as u32);
        let mut bgra = Vec::with_capacity(rgba.len());
        for chunk in rgba.as_chunks::<4>().0 {
            bgra.push(chunk[2]);
            bgra.push(chunk[1]);
            bgra.push(chunk[0]);
            bgra.push(chunk[3]);
        }
        let and_mask = vec![0u8; (size as usize * size as usize) / 8];
        CreateIcon(Some(hinst), size, size, 1, 32, and_mask.as_ptr(), bgra.as_ptr())
            .unwrap_or_default()
    }
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        if msg == WM_NCCREATE {
            let cs = &*(lparam.0
                as *const windows::Win32::UI::WindowsAndMessaging::CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        }

        let ptr = windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(
            hwnd,
            GWLP_USERDATA,
        ) as *mut StreamTimerState;
        if ptr.is_null() {
            return DefWindowProcW(hwnd, msg, wparam, lparam);
        }
        let state = &mut *ptr;

        match msg {
            WM_CREATE => {
                state.hwnd = hwnd;
                apply_dark_titlebar(hwnd);

                let mut nid = NOTIFYICONDATAW {
                    cbSize: size_of::<NOTIFYICONDATAW>() as u32,
                    hWnd: hwnd,
                    uID: 1,
                    uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
                    uCallbackMessage: WM_TRAYICON,
                    hIcon: state.hicon,
                    ..Default::default()
                };
                let tip = to_wide("Stream Timer — Click để hiện/ẩn");
                let copy_len = tip.len().min(nid.szTip.len());
                nid.szTip[..copy_len].copy_from_slice(&tip[..copy_len]);
                state.tray_added = Shell_NotifyIconW(NIM_ADD, &nid).as_bool();

                state.hotkey_registered = RegisterHotKey(
                    Some(hwnd),
                    GLOBAL_TRAY_HOTKEY_ID,
                    MOD_CONTROL | MOD_SHIFT | MOD_NOREPEAT,
                    b'H' as u32,
                )
                .is_ok();

                if let Some(guard) = state.single_instance.as_mut() {
                    guard.set_target_hwnd(hwnd);
                }

                state.sync_and_schedule_tick(true);
                if let Some(prev) = state.session_store.previous_candidate() {
                    let hint = format!("Ctrl+S: Resume {}", prev.formatted);
                    state.flash_title_ms(&hint, 3500);
                }
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_GETMINMAXINFO => {
                let mmi = &mut *(lparam.0 as *mut MINMAXINFO);
                mmi.ptMinTrackSize.x = 180;
                mmi.ptMinTrackSize.y = 68;
                LRESULT(0)
            }
            WM_SIZE => {
                let w = (lparam.0 & 0xFFFF) as i16 as i32;
                let h = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
                if w > 0 && h > 0 && !state.is_stealth_minimized {
                    state.renderer.recompute_font_size(w, h);
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
                LRESULT(0)
            }
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let hdc = BeginPaint(hwnd, &mut ps);
                let mut rc = RECT::default();
                if GetClientRect(hwnd, &mut rc).is_ok() {
                    let w = rc.right - rc.left;
                    let h = rc.bottom - rc.top;
                    let text = state.last_text.clone();
                    state.renderer.render_to_hdc(hdc, w, h, &text);
                }
                let _ = EndPaint(hwnd, &ps);
                LRESULT(0)
            }
            WM_TIMER => {
                match wparam.0 {
                    TIMER_TICK_ID => state.sync_and_schedule_tick(false),
                    TIMER_TITLE_ID => {
                        let _ = KillTimer(Some(hwnd), TIMER_TITLE_ID);
                        state.restore_default_title();
                    }
                    _ => {}
                }
                LRESULT(0)
            }
            WM_SYSCOMMAND => {
                if (wparam.0 & 0xFFF0) == SC_MINIMIZE as usize
                    && state.minimize_mode != MinimizeMode::Native
                {
                    state.enter_stealth_minimize();
                    return LRESULT(0);
                }
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            WM_ACTIVATE => {
                let active_state = (wparam.0 & 0xFFFF) as u32;
                if (active_state == WA_ACTIVE || active_state == WA_CLICKACTIVE)
                    && state.is_stealth_minimized
                {
                    if state.in_tray {
                        // Never pop out of System Tray on incidental Windows focus shifts.
                        let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowPos(
                            hwnd,
                            Some(windows::Win32::UI::WindowsAndMessaging::HWND_BOTTOM),
                            0,
                            0,
                            0,
                            0,
                            windows::Win32::UI::WindowsAndMessaging::SWP_NOMOVE
                                | windows::Win32::UI::WindowsAndMessaging::SWP_NOSIZE
                                | windows::Win32::UI::WindowsAndMessaging::SWP_NOACTIVATE,
                        );
                        focus_next_window(hwnd);
                    } else {
                        let allowed = state
                            .suppress_activate_until
                            .is_none_or(|t| Instant::now() >= t);
                        if allowed {
                            state.exit_stealth_minimize();
                        }
                    }
                }
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            WM_HOTKEY => {
                if wparam.0 as i32 == GLOBAL_TRAY_HOTKEY_ID {
                    if state.is_stealth_minimized {
                        state.exit_stealth_minimize();
                    } else {
                        state.minimize_to_tray();
                    }
                    return LRESULT(0);
                }
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            WM_APP_WAKE => {
                if state.is_stealth_minimized {
                    state.exit_stealth_minimize();
                } else {
                    let _ = windows::Win32::UI::WindowsAndMessaging::ShowWindow(hwnd, SW_SHOW);
                    let _ = windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow(hwnd);
                }
                LRESULT(0)
            }
            WM_KEYDOWN => {
                let vk = wparam.0 as u32;
                let ctrl_down = (GetKeyState(VK_CONTROL.0 as i32) as u16 & 0x8000) != 0;
                match vk {
                    0x20 => state.toggle_pause(),                  // Space
                    0x52 => state.reset_timer(),                   // R
                    0x48 => state.minimize_to_tray(),              // H
                    0x4D => {                                      // M
                        if ctrl_down {
                            state.minimize_to_tray();
                        } else {
                            state.trigger_minimize();
                        }
                    }
                    0x53 => {                                      // S
                        if ctrl_down {
                            state.resume_previous_session();
                        } else {
                            return DefWindowProcW(hwnd, msg, wparam, lparam);
                        }
                    }
                    0x54 => state.toggle_always_on_top(),          // T
                    0x43 => state.copy_url(false),                 // C
                    0x1B | 0x51 => {                               // Esc / Q
                        let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                    }
                    _ => return DefWindowProcW(hwnd, msg, wparam, lparam),
                }
                LRESULT(0)
            }
            WM_RBUTTONUP => {
                let mut pt = POINT::default();
                let _ = GetCursorPos(&mut pt);
                state.show_context_menu(pt.x, pt.y, false);
                LRESULT(0)
            }
            WM_CONTEXTMENU => {
                if lparam.0 != -1 {
                    let x = (lparam.0 & 0xFFFF) as i16 as i32;
                    let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
                    state.show_context_menu(x, y, false);
                } else {
                    let mut pt = POINT::default();
                    let _ = GetCursorPos(&mut pt);
                    state.show_context_menu(pt.x, pt.y, false);
                }
                LRESULT(0)
            }
            WM_TRAYICON => {
                let event = (lparam.0 & 0xFFFF) as u32;
                if event == WM_LBUTTONUP {
                    if state.is_stealth_minimized {
                        state.exit_stealth_minimize();
                    } else {
                        state.minimize_to_tray();
                    }
                } else if event == WM_RBUTTONUP || event == WM_CONTEXTMENU {
                    let mut pt = POINT::default();
                    let _ = GetCursorPos(&mut pt);
                    state.show_context_menu(pt.x, pt.y, true);
                }
                LRESULT(0)
            }
            WM_COMMAND => {
                state.handle_command(wparam.0 & 0xFFFF);
                LRESULT(0)
            }
            WM_CLOSE => {
                let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_DESTROY => {
                let _ = KillTimer(Some(hwnd), TIMER_TICK_ID);
                let _ = KillTimer(Some(hwnd), TIMER_TITLE_ID);
                if state.hotkey_registered {
                    let _ = UnregisterHotKey(Some(hwnd), GLOBAL_TRAY_HOTKEY_ID);
                    state.hotkey_registered = false;
                }
                if state.tray_added {
                    let nid = NOTIFYICONDATAW {
                        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
                        hWnd: hwnd,
                        uID: 1,
                        ..Default::default()
                    };
                    let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
                    state.tray_added = false;
                }
                let final_ms = state.engine.snapshot().elapsed_ms;
                state.session_store.force_save(final_ms);
                state.single_instance = None;
                state.server.stop();
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                if !state.bg_brush.is_invalid() {
                    let _ = windows::Win32::Graphics::Gdi::DeleteObject(state.bg_brush.into());
                }
                drop(Box::from_raw(ptr));
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
}

pub fn run_app(config: AppConfig, single_instance: Option<SingleInstanceGuard>) -> windows::core::Result<()> {
    let com_init = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok() };
    enable_dark_menus();

    let engine = TimerEngine::new(true);
    let session_dir = config
        .session_dir
        .unwrap_or_else(SessionStore::default_dir);
    let session_store = SessionStore::open(session_dir);
    let server = BrowserTimerServer::start(engine.clone(), config.preferred_port)
        .expect("failed to bind local Browser Source HTTP server");
    let browser_url = server.url();
    let renderer = DirectWriteRenderer::new()?;

    unsafe {
        let hinst: HINSTANCE = GetModuleHandleW(None)?.into();
        let hicon = load_app_icon(hinst);
        let bg_brush = CreateSolidBrush(COLORREF(0x0000_0000));
        let class_w = to_wide(CLASS_NAME);
        let title_w = to_wide(BASE_TITLE);

        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wnd_proc),
            hInstance: hinst,
            hIcon: hicon,
            hIconSm: hicon,
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            hbrBackground: bg_brush,
            lpszClassName: PCWSTR(class_w.as_ptr()),
            ..Default::default()
        };
        RegisterClassExW(&wc);

        let dpi = GetDpiForSystem().max(96);
        let scale = dpi as f32 / 96.0;
        let client_w = (config.width as f32 * scale).round() as i32;
        let client_h = (config.height as f32 * scale).round() as i32;
        let mut adj = RECT {
            left: 0,
            top: 0,
            right: client_w,
            bottom: client_h,
        };
        let _ = AdjustWindowRectExForDpi(&mut adj, WS_OVERLAPPEDWINDOW, false, WS_EX_APPWINDOW, dpi);
        let outer_w = adj.right - adj.left;
        let outer_h = adj.bottom - adj.top;

        let state = Box::new(StreamTimerState {
            hwnd: HWND::default(),
            engine,
            session_store,
            server,
            browser_url,
            renderer,
            minimize_mode: config.minimize_mode,
            always_on_top: false,
            hide_from_alt_tab: false,
            is_stealth_minimized: false,
            in_tray: false,
            was_corner_peek: false,
            saved_pos: None,
            last_text: "00:00:00".to_owned(),
            suppress_activate_until: None,
            tray_added: false,
            hotkey_registered: false,
            single_instance,
            bg_brush,
            hicon,
        });
        let raw_state = Box::into_raw(state);

        let hwnd = CreateWindowExW(
            WS_EX_APPWINDOW,
            PCWSTR(class_w.as_ptr()),
            PCWSTR(title_w.as_ptr()),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            outer_w,
            outer_h,
            None,
            None,
            Some(hinst),
            Some(raw_state as *const c_void),
        )?;

        let _ = windows::Win32::UI::WindowsAndMessaging::ShowWindow(hwnd, SW_SHOW);
        let _ = UpdateWindow(hwnd);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    if com_init {
        unsafe {
            CoUninitialize();
        }
    }
    Ok(())
}
