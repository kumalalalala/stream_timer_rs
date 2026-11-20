//! Thread-safe drift-free monotonic stopwatch engine.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimerSnapshot {
    pub running: bool,
    pub elapsed_ms: u64,
    pub formatted: String,
}

#[derive(Debug)]
struct InnerState {
    accumulated: Duration,
    started_at: Option<Instant>,
    running: bool,
}

#[derive(Clone)]
pub struct TimerEngine {
    inner: Arc<Mutex<InnerState>>,
}

impl TimerEngine {
    pub fn new(auto_start: bool) -> Self {
        Self {
            inner: Arc::new(Mutex::new(InnerState {
                accumulated: Duration::ZERO,
                started_at: auto_start.then(Instant::now),
                running: auto_start,
            })),
        }
    }

    pub fn format_ms(elapsed_ms: u64) -> String {
        let total_secs = elapsed_ms / 1000;
        let hours = total_secs / 3600;
        let minutes = (total_secs % 3600) / 60;
        let seconds = total_secs % 60;
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    }

    fn elapsed_locked(st: &InnerState) -> Duration {
        if st.running
            && let Some(start) = st.started_at
        {
            return st.accumulated + start.elapsed();
        }
        st.accumulated
    }

    pub fn snapshot(&self) -> TimerSnapshot {
        let st = self.inner.lock().expect("timer mutex poisoned");
        let elapsed_ms = Self::elapsed_locked(&st).as_millis() as u64;
        TimerSnapshot {
            running: st.running,
            elapsed_ms,
            formatted: Self::format_ms(elapsed_ms),
        }
    }

    pub fn pause(&self) -> TimerSnapshot {
        let mut st = self.inner.lock().expect("timer mutex poisoned");
        if st.running {
            st.accumulated = Self::elapsed_locked(&st);
            st.started_at = None;
            st.running = false;
        }
        let elapsed_ms = st.accumulated.as_millis() as u64;
        TimerSnapshot {
            running: false,
            elapsed_ms,
            formatted: Self::format_ms(elapsed_ms),
        }
    }

    pub fn resume(&self) -> TimerSnapshot {
        let mut st = self.inner.lock().expect("timer mutex poisoned");
        if !st.running {
            st.started_at = Some(Instant::now());
            st.running = true;
        }
        let elapsed_ms = Self::elapsed_locked(&st).as_millis() as u64;
        TimerSnapshot {
            running: true,
            elapsed_ms,
            formatted: Self::format_ms(elapsed_ms),
        }
    }

    pub fn toggle(&self) -> TimerSnapshot {
        let mut st = self.inner.lock().expect("timer mutex poisoned");
        if st.running {
            st.accumulated = Self::elapsed_locked(&st);
            st.started_at = None;
            st.running = false;
        } else {
            st.started_at = Some(Instant::now());
            st.running = true;
        }
        let elapsed_ms = Self::elapsed_locked(&st).as_millis() as u64;
        TimerSnapshot {
            running: st.running,
            elapsed_ms,
            formatted: Self::format_ms(elapsed_ms),
        }
    }

    pub fn reset(&self, keep_running: bool) -> TimerSnapshot {
        let mut st = self.inner.lock().expect("timer mutex poisoned");
        st.accumulated = Duration::ZERO;
        if keep_running {
            st.started_at = Some(Instant::now());
            st.running = true;
        } else {
            st.started_at = None;
            st.running = false;
        }
        TimerSnapshot {
            running: st.running,
            elapsed_ms: 0,
            formatted: "00:00:00".to_owned(),
        }
    }

    pub fn set_elapsed_ms(&self, elapsed_ms: u64) -> TimerSnapshot {
        let mut st = self.inner.lock().expect("timer mutex poisoned");
        st.accumulated = Duration::from_millis(elapsed_ms);
        if st.running {
            st.started_at = Some(Instant::now());
        } else {
            st.started_at = None;
        }
        TimerSnapshot {
            running: st.running,
            elapsed_ms,
            formatted: Self::format_ms(elapsed_ms),
        }
    }
}
