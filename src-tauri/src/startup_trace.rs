//! Where the time goes between launching Lume and showing the Orb. `LUME_STARTUP_TRACE=1` prints one
//! line per phase of `setup` to stderr; without it the trace costs nothing.

use std::time::Instant;

pub struct StartupTrace {
    enabled: bool,
    begin: Instant,
    last: Instant,
}

impl StartupTrace {
    pub fn new() -> Self {
        let now = Instant::now();
        Self { enabled: std::env::var_os("LUME_STARTUP_TRACE").is_some(), begin: now, last: now }
    }

    pub fn mark(&mut self, phase: &str) {
        if !self.enabled {
            return;
        }
        let now = Instant::now();
        eprintln!("[startup] {phase:<28} +{:>5} ms   total {:>5} ms", (now - self.last).as_millis(), (now - self.begin).as_millis());
        self.last = now;
    }
}
