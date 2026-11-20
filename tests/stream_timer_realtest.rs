//! Comprehensive unit & pixel-level DirectWrite RealTests for stream_timer_rs.

use std::ffi::c_void;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use stream_timer::browser_server::BrowserTimerServer;
use stream_timer::renderer::DirectWriteRenderer;
use stream_timer::timer_engine::TimerEngine;
use stream_timer::win32_stealth::{
    InstanceAcquisition, MinimizeMode, acquire_or_wake_named, enable_per_monitor_dpi,
    set_taskbar_hidden,
};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS,
    DeleteDC, DeleteObject, SelectObject,
};

fn http_get(port: u16, path: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect to local server");
    let req = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    stream.write_all(req.as_bytes()).expect("write request");
    let mut resp = String::new();
    stream.read_to_string(&mut resp).expect("read response");
    resp
}

#[test]
fn timer_engine_formatting_and_lifecycle() {
    assert_eq!(TimerEngine::format_ms(0), "00:00:00");
    assert_eq!(TimerEngine::format_ms(999), "00:00:00");
    assert_eq!(TimerEngine::format_ms(1000), "00:00:01");
    assert_eq!(TimerEngine::format_ms(3_723_000), "01:02:03");
    assert_eq!(TimerEngine::format_ms(360_000_000), "100:00:00");

    let engine = TimerEngine::new(true);
    thread::sleep(Duration::from_millis(45));
    let s1 = engine.snapshot();
    assert!(s1.running);
    assert!(s1.elapsed_ms >= 25);

    let paused = engine.pause();
    assert!(!paused.running);
    thread::sleep(Duration::from_millis(40));
    assert_eq!(paused.elapsed_ms, engine.snapshot().elapsed_ms);

    let resumed = engine.resume();
    assert!(resumed.running);
    thread::sleep(Duration::from_millis(35));
    assert!(engine.snapshot().elapsed_ms > paused.elapsed_ms);

    let reset = engine.reset(true);
    assert!(reset.running);
    assert_eq!(reset.formatted, "00:00:00");
}

#[test]
fn browser_server_serves_html_transparent_and_state_with_port_fallback() {
    let blocker = TcpListener::bind(("127.0.0.1", 0)).expect("bind blocker");
    let busy_port = blocker.local_addr().unwrap().port();

    let engine = TimerEngine::new(true);
    let mut server = BrowserTimerServer::start(engine, busy_port).expect("start server");
    assert_ne!(server.port, busy_port);

    let html = http_get(server.port, "/");
    assert!(html.contains("200 OK"));
    assert!(html.contains("background: #000000;"));
    assert!(html.contains("fitClockFont("));

    let html_t = http_get(server.port, "/?bg=transparent");
    assert!(html_t.contains("background: transparent;"));

    let state = http_get(server.port, "/state");
    assert!(state.contains("\"running\":true"));
    assert!(state.contains("\"formatted\":\""));

    server.stop();
    drop(blocker);
}

#[test]
fn directwrite_renderer_zero_overflow_and_antialiasing_across_sizes() {
    enable_per_monitor_dpi();
    let mut renderer = DirectWriteRenderer::new().expect("create DirectWriteRenderer");

    for (w, h) in [(440i32, 140i32), (220, 120), (720, 90), (180, 68)] {
        renderer.recompute_font_size(w, h);

        unsafe {
            let mem_dc = CreateCompatibleDC(None);
            assert!(!mem_dc.is_invalid());
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits_ptr: *mut c_void = std::ptr::null_mut();
            let dib = CreateDIBSection(Some(mem_dc), &bmi, DIB_RGB_COLORS, &mut bits_ptr, None, 0)
                .expect("CreateDIBSection");
            let old = SelectObject(mem_dc, dib.into());

            renderer.render_to_hdc(mem_dc, w, h, "88:88:88");

            let slice = std::slice::from_raw_parts(bits_ptr as *const u8, (w * h * 4) as usize);
            let mut min_x = w;
            let mut min_y = h;
            let mut max_x = -1;
            let mut max_y = -1;
            let mut pure_white = 0usize;
            let mut aa_pixels = 0usize;

            for y in 0..h {
                for x in 0..w {
                    let idx = ((y * w + x) * 4) as usize;
                    let r = slice[idx + 2];
                    if r > 15 {
                        min_x = min_x.min(x);
                        max_x = max_x.max(x);
                        min_y = min_y.min(y);
                        max_y = max_y.max(y);
                        if r >= 240 {
                            pure_white += 1;
                        } else {
                            aa_pixels += 1;
                        }
                    }
                }
            }

            let _ = SelectObject(mem_dc, old);
            let _ = DeleteObject(dib.into());
            let _ = DeleteDC(mem_dc);

            assert!(max_x >= 0, "No text pixels rendered at {w}x{h}");
            let margin_l = min_x;
            let margin_t = min_y;
            let margin_r = w - 1 - max_x;
            let margin_b = h - 1 - max_y;
            assert!(
                margin_l >= 4 && margin_t >= 4 && margin_r >= 4 && margin_b >= 4,
                "Overflow at {w}x{h}: margins=({margin_l},{margin_t},{margin_r},{margin_b})"
            );
            assert!(pure_white > 150, "Too few white pixels at {w}x{h}: {pure_white}");
            assert!(aa_pixels > 80, "Text appears jagged at {w}x{h}: aa={aa_pixels}");
        }
    }
}

#[test]
fn minimize_mode_parsing_and_single_instance_lock() {
    assert_eq!(MinimizeMode::parse("tray_wgc"), Some(MinimizeMode::TrayWgc));
    assert_eq!(
        MinimizeMode::parse("background_wgc"),
        Some(MinimizeMode::BackgroundWgc)
    );
    assert!(MinimizeMode::TrayCornerPeek.uses_tray());
    assert!(MinimizeMode::TrayCornerPeek.uses_corner_peek());

    let pid = std::process::id();
    let m = format!("Local\\StreamTimer_TestM_{pid}");
    let e = format!("Local\\StreamTimer_TestE_{pid}");
    let g = match acquire_or_wake_named(&m, &e) {
        InstanceAcquisition::Primary(g) => g,
        InstanceAcquisition::SecondarySignaled => panic!("expected Primary"),
    };
    match acquire_or_wake_named(&m, &e) {
        InstanceAcquisition::SecondarySignaled => {}
        InstanceAcquisition::Primary(_) => panic!("expected SecondarySignaled"),
    }
    drop(g);
}

#[test]
fn session_store_alternates_two_files_and_preserves_previous() {
    use stream_timer::session_store::SessionStore;

    let tmp_dir = std::env::temp_dir().join(format!(
        "stream_timer_session_test_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&tmp_dir);

    // Launch 1 -> uses session_0.txt (seq=1), saves 125_000ms (00:02:05)
    {
        let mut s1 = SessionStore::open(tmp_dir.clone());
        assert!(s1.previous_candidate().is_none());
        assert!(s1.active_file_path().ends_with("session_0.txt"));
        s1.save_if_second_changed(125_000);
    }
    let s0_content_after_1 =
        std::fs::read_to_string(tmp_dir.join("session_0.txt")).expect("session_0 exists");
    assert!(s0_content_after_1.contains("elapsed_ms=125000"));

    // Launch 2 -> reads session_0 as previous, writes to session_1.txt (seq=2), never touches session_0
    {
        let mut s2 = SessionStore::open(tmp_dir.clone());
        assert!(s2.active_file_path().ends_with("session_1.txt"));
        let cand = s2.previous_candidate().expect("previous session candidate");
        assert_eq!(cand.elapsed_ms, 125_000);
        assert_eq!(cand.formatted, "00:02:05");

        s2.save_if_second_changed(45_000);
        assert_eq!(
            std::fs::read_to_string(tmp_dir.join("session_0.txt")).unwrap(),
            s0_content_after_1,
            "Launch 2 must NOT modify Launch 1's file"
        );

        // Ctrl+S resume returns 125_000 once, then None on accidental second press
        assert_eq!(s2.consume_resume_ms(), Some(125_000));
        assert_eq!(s2.consume_resume_ms(), None);
        s2.force_save(130_000);
    }

    // Launch 3 -> now session_1.txt (seq=2, 130_000ms) is newest, rotates back to session_0.txt (seq=3)
    {
        let s3 = SessionStore::open(tmp_dir.clone());
        assert!(s3.active_file_path().ends_with("session_0.txt"));
        let cand3 = s3.previous_candidate().expect("Launch 2 is now newest");
        assert_eq!(cand3.elapsed_ms, 130_000);
        assert_eq!(cand3.formatted, "00:02:10");
    }

    let file_count = std::fs::read_dir(&tmp_dir)
        .unwrap()
        .filter_map(Result::ok)
        .count();
    assert_eq!(file_count, 2, "Must maintain strictly 2 alternating files");
    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn directwrite_infinite_hours_autoscale_never_overflows() {
    enable_per_monitor_dpi();
    let mut renderer = DirectWriteRenderer::new().expect("create DirectWriteRenderer");

    for (w, h) in [(440i32, 140i32), (240, 80)] {
        renderer.recompute_font_size(w, h);
        for sample in [
            "88:88:88",
            "888:88:88",
            "88888:88:88",
            "88888888:88:88",
            "888888888888:88:88",
        ] {
            unsafe {
                let mem_dc = CreateCompatibleDC(None);
                let bmi = BITMAPINFO {
                    bmiHeader: BITMAPINFOHEADER {
                        biSize: size_of::<BITMAPINFOHEADER>() as u32,
                        biWidth: w,
                        biHeight: -h,
                        biPlanes: 1,
                        biBitCount: 32,
                        biCompression: BI_RGB.0,
                        ..Default::default()
                    },
                    ..Default::default()
                };
                let mut bits_ptr: *mut c_void = std::ptr::null_mut();
                let dib =
                    CreateDIBSection(Some(mem_dc), &bmi, DIB_RGB_COLORS, &mut bits_ptr, None, 0)
                        .expect("CreateDIBSection");
                let old = SelectObject(mem_dc, dib.into());

                renderer.render_to_hdc(mem_dc, w, h, sample);

                let slice =
                    std::slice::from_raw_parts(bits_ptr as *const u8, (w * h * 4) as usize);
                let (mut min_x, mut min_y, mut max_x, mut max_y) = (w, h, -1, -1);
                for y in 0..h {
                    for x in 0..w {
                        let r = slice[((y * w + x) * 4 + 2) as usize];
                        if r > 15 {
                            min_x = min_x.min(x);
                            max_x = max_x.max(x);
                            min_y = min_y.min(y);
                            max_y = max_y.max(y);
                        }
                    }
                }

                let _ = SelectObject(mem_dc, old);
                let _ = DeleteObject(dib.into());
                let _ = DeleteDC(mem_dc);

                assert!(max_x >= 0, "No pixels for '{sample}' at {w}x{h}");
                assert!(
                    min_x >= 4 && min_y >= 4 && (w - 1 - max_x) >= 4 && (h - 1 - max_y) >= 4,
                    "Overflow for '{sample}' at {w}x{h}: ({min_x},{min_y},{},{})",
                    w - 1 - max_x,
                    h - 1 - max_y
                );
            }
        }
    }
}

#[test]
fn tray_mode_keeps_window_capturable_unless_alt_tab_hide_requested() {
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, GWL_EXSTYLE, GetWindowLongPtrW, IsIconic,
        IsWindowVisible, WINDOW_EX_STYLE, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW, WS_OVERLAPPEDWINDOW,
        WS_VISIBLE,
    };
    use windows::core::w;

    unsafe {
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("STATIC"),
            w!("StreamTimerCaptureFlagTest"),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            100,
            100,
            320,
            140,
            None,
            None,
            None,
            None,
        )
        .expect("CreateWindowExW");

        // Hide taskbar button only (also_hide_alt_tab = false) -> WS_EX_TOOLWINDOW must stay 0
        set_taskbar_hidden(hwnd, true, false);
        let ex_capturable = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        assert_eq!(
            ex_capturable & WS_EX_TOOLWINDOW.0,
            0,
            "Capturable tray mode must keep WS_EX_TOOLWINDOW == 0 so OBS/TikTok pickers see it"
        );
        assert!(IsWindowVisible(hwnd).as_bool(), "must remain visible");
        assert!(!IsIconic(hwnd).as_bool(), "must not be iconic");

        // Optional Alt+Tab hide (also_hide_alt_tab = true) -> WS_EX_TOOLWINDOW set
        set_taskbar_hidden(hwnd, true, true);
        let ex_alt_hidden = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        assert_ne!(ex_alt_hidden & WS_EX_TOOLWINDOW.0, 0);
        assert_eq!(ex_alt_hidden & WS_EX_APPWINDOW.0, 0);

        // Restoring resets WS_EX_TOOLWINDOW cleanly
        set_taskbar_hidden(hwnd, false, false);
        let ex_restored = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        assert_eq!(ex_restored & WS_EX_TOOLWINDOW.0, 0);

        let _ = DestroyWindow(hwnd);
    }
}
