//! Two-slot alternating (ping-pong) realtime session persistence for crash/close recovery.

use std::fs;
use std::path::{Path, PathBuf};

use crate::timer_engine::TimerEngine;

const FILE_NAMES: [&str; 2] = ["session_0.txt", "session_1.txt"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRecord {
    pub seq: u64,
    pub elapsed_ms: u64,
    pub formatted: String,
}

pub struct SessionStore {
    dir: PathBuf,
    active_slot: usize,
    active_seq: u64,
    previous: Option<SessionRecord>,
    resumed: bool,
    last_saved_sec: Option<u64>,
}

impl SessionStore {
    pub fn default_dir() -> PathBuf {
        if let Ok(local_app_data) = std::env::var("LOCALAPPDATA")
            && !local_app_data.trim().is_empty()
        {
            return PathBuf::from(local_app_data).join("StreamTimer");
        }
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf))
            .unwrap_or_else(|| PathBuf::from("."))
    }

    pub fn open(dir: PathBuf) -> Self {
        let _ = fs::create_dir_all(&dir);
        let rec0 = read_record(&dir.join(FILE_NAMES[0]));
        let rec1 = read_record(&dir.join(FILE_NAMES[1]));

        let (previous, active_slot, active_seq) = match (rec0, rec1) {
            (Some(r0), Some(r1)) => {
                if r0.seq >= r1.seq {
                    let seq = r0.seq.saturating_add(1);
                    (Some(r0), 1, seq)
                } else {
                    let seq = r1.seq.saturating_add(1);
                    (Some(r1), 0, seq)
                }
            }
            (Some(r0), None) => {
                let seq = r0.seq.saturating_add(1);
                (Some(r0), 1, seq)
            }
            (None, Some(r1)) => {
                let seq = r1.seq.saturating_add(1);
                (Some(r1), 0, seq)
            }
            (None, None) => (None, 0, 1),
        };

        let mut store = Self {
            dir,
            active_slot,
            active_seq,
            previous,
            resumed: false,
            last_saved_sec: None,
        };
        // Register the new session slot immediately without touching the previous slot.
        store.force_save(0);
        store
    }

    pub fn active_file_path(&self) -> PathBuf {
        self.dir.join(FILE_NAMES[self.active_slot])
    }

    pub fn previous_candidate(&self) -> Option<&SessionRecord> {
        if self.resumed {
            return None;
        }
        self.previous.as_ref().filter(|r| r.elapsed_ms >= 1000)
    }

    pub fn consume_resume_ms(&mut self) -> Option<u64> {
        if self.resumed {
            return None;
        }
        let prev = self.previous.as_ref()?;
        self.resumed = true;
        Some(prev.elapsed_ms)
    }

    pub fn is_resumed(&self) -> bool {
        self.resumed
    }

    /// Save realtime progress once per new second (or when forced).
    pub fn save_if_second_changed(&mut self, elapsed_ms: u64) {
        let sec = elapsed_ms / 1000;
        if self.last_saved_sec == Some(sec) {
            return;
        }
        self.force_save(elapsed_ms);
    }

    pub fn force_save(&mut self, elapsed_ms: u64) {
        self.last_saved_sec = Some(elapsed_ms / 1000);
        let formatted = TimerEngine::format_ms(elapsed_ms);
        let content = format!(
            "seq={}\nelapsed_ms={}\nformatted={}\n",
            self.active_seq, elapsed_ms, formatted
        );
        let target = self.active_file_path();
        let tmp = self.dir.join(format!("session_{}.tmp", self.active_slot));
        if fs::write(&tmp, content.as_bytes()).is_ok() {
            if fs::rename(&tmp, &target).is_err() {
                let _ = fs::write(&target, content.as_bytes());
                let _ = fs::remove_file(&tmp);
            }
        } else {
            let _ = fs::write(&target, content.as_bytes());
        }
    }
}

fn read_record(path: &Path) -> Option<SessionRecord> {
    let text = fs::read_to_string(path).ok()?;
    let mut seq: Option<u64> = None;
    let mut elapsed_ms: Option<u64> = None;
    let mut formatted: Option<String> = None;

    for line in text.lines() {
        if let Some((k, v)) = line.split_once('=') {
            match k.trim() {
                "seq" => seq = v.trim().parse::<u64>().ok(),
                "elapsed_ms" => elapsed_ms = v.trim().parse::<u64>().ok(),
                "formatted" => formatted = Some(v.trim().to_owned()),
                _ => {}
            }
        }
    }

    let seq = seq?;
    let elapsed_ms = elapsed_ms?;
    let formatted = formatted.unwrap_or_else(|| TimerEngine::format_ms(elapsed_ms));
    Some(SessionRecord {
        seq,
        elapsed_ms,
        formatted,
    })
}
