//! Lightweight stdlib-only localhost HTTP server for OBS / streaming Browser Source.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::timer_engine::TimerEngine;

const HTML_PAGE: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>Stream Timer</title>
<style>
  html, body {
    margin: 0;
    padding: 0;
    width: 100%;
    height: 100%;
    overflow: hidden;
    background: __BG__;
    color: #ffffff;
    display: flex;
    align-items: center;
    justify-content: center;
    user-select: none;
  }
  #clock {
    font-family: "Consolas", "Cascadia Mono", "Courier New", monospace;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
    font-size: min(17vw, 72vh);
    line-height: 1;
    letter-spacing: 0.04em;
    color: #ffffff;
    white-space: nowrap;
  }
</style>
</head>
<body>
  <div id="clock">00:00:00</div>
  <script>
    const clockEl = document.getElementById("clock");
    let running = true;
    let baseElapsedMs = 0;
    let syncPerfMs = performance.now();
    let lastRendered = "";
    let lastCharLen = 8;

    function fitClockFont(charLen) {
      const n = Math.max(8, charLen);
      const maxVw = (86 / (0.62 * n)).toFixed(2);
      clockEl.style.fontSize = "min(" + maxVw + "vw, 72vh)";
    }

    function formatMs(ms) {
      const totalSec = Math.max(0, Math.floor(ms / 1000));
      const h = Math.floor(totalSec / 3600);
      const m = Math.floor((totalSec % 3600) / 60);
      const s = totalSec % 60;
      const pad = (n) => (n < 10 ? "0" + n : String(n));
      return pad(h) + ":" + pad(m) + ":" + pad(s);
    }

    function renderLoop() {
      const currentMs = running
        ? baseElapsedMs + Math.max(0, performance.now() - syncPerfMs)
        : baseElapsedMs;
      const text = formatMs(currentMs);
      if (text !== lastRendered) {
        if (text.length !== lastCharLen) {
          lastCharLen = text.length;
          fitClockFont(lastCharLen);
        }
        clockEl.textContent = text;
        lastRendered = text;
      }
      requestAnimationFrame(renderLoop);
    }

    async function syncState() {
      try {
        const res = await fetch("/state", { cache: "no-store" });
        if (!res.ok) return;
        const data = await res.json();
        running = Boolean(data.running);
        baseElapsedMs = Number(data.elapsed_ms) || 0;
        syncPerfMs = performance.now();
      } catch (_) {}
    }

    syncState();
    setInterval(syncState, 250);
    requestAnimationFrame(renderLoop);
  </script>
</body>
</html>
"#;

pub struct BrowserTimerServer {
    pub port: u16,
    stop_flag: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl BrowserTimerServer {
    pub fn start(engine: TimerEngine, preferred_port: u16) -> std::io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", preferred_port))
            .or_else(|_| TcpListener::bind(("127.0.0.1", 0)))?;
        let port = listener.local_addr()?.port();
        listener.set_nonblocking(true)?;

        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_clone = Arc::clone(&stop_flag);

        let handle = thread::Builder::new()
            .name("stream-timer-http".to_owned())
            .spawn(move || {
                while !stop_clone.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let eng = engine.clone();
                            thread::spawn(move || {
                                handle_client(stream, &eng);
                            });
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(20));
                        }
                        Err(_) => break,
                    }
                }
            })?;

        Ok(Self {
            port,
            stop_flag,
            handle: Some(handle),
        })
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}/", self.port)
    }

    pub fn stop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for BrowserTimerServer {
    fn drop(&mut self) {
        self.stop();
    }
}

fn handle_client(mut stream: TcpStream, engine: &TimerEngine) {
    let _ = stream.set_read_timeout(Some(Duration::from_millis(800)));
    let _ = stream.set_write_timeout(Some(Duration::from_millis(800)));
    let mut buf = [0u8; 2048];
    let n = match stream.read(&mut buf) {
        Ok(n) if n > 0 => n,
        _ => return,
    };
    let req = String::from_utf8_lossy(&buf[..n]);
    let first_line = req.lines().next().unwrap_or("");
    let mut parts = first_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("/");

    if method != "GET" {
        write_response(&mut stream, "405 Method Not Allowed", "text/plain", b"");
        return;
    }

    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    match path {
        "/state" => {
            let snap = engine.snapshot();
            let json = format!(
                "{{\"running\":{},\"elapsed_ms\":{},\"formatted\":\"{}\"}}",
                snap.running, snap.elapsed_ms, snap.formatted
            );
            write_response(
                &mut stream,
                "200 OK",
                "application/json; charset=utf-8",
                json.as_bytes(),
            );
        }
        "/" => {
            let transparent = query
                .split('&')
                .any(|kv| kv.eq_ignore_ascii_case("bg=transparent"));
            let bg = if transparent { "transparent" } else { "#000000" };
            let html = HTML_PAGE.replace("__BG__", bg);
            write_response(
                &mut stream,
                "200 OK",
                "text/html; charset=utf-8",
                html.as_bytes(),
            );
        }
        _ => {
            write_response(&mut stream, "404 Not Found", "text/plain", b"Not Found");
        }
    }
}

fn write_response(stream: &mut TcpStream, status: &str, content_type: &str, body: &[u8]) {
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nCache-Control: no-store\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(header.as_bytes());
    if !body.is_empty() {
        let _ = stream.write_all(body);
    }
}
