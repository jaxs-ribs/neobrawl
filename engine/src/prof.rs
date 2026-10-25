//! A tiny profiler: where each game tick and each rendered frame spends its
//! time, by phase. Read (and reset) by the host for the perf overlay.

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
extern "C" {
    // `performance.now` must be called as a method on `performance`
    type Performance;
    #[wasm_bindgen(thread_local, js_name = performance)]
    static PERFORMANCE: Performance;
    #[wasm_bindgen(method, js_name = now)]
    fn now_on(this: &Performance) -> f64;
}

#[cfg(target_arch = "wasm32")]
fn now() -> f64 {
    PERFORMANCE.with(|p| p.now_on())
}

#[cfg(not(target_arch = "wasm32"))]
fn now() -> f64 {
    use std::time::Instant;
    thread_local!(static START: Instant = Instant::now());
    START.with(|s| s.elapsed().as_secs_f64() * 1000.0)
}

/// Phases, in the order they run.
pub const PHASES: [&str; 16] = [
    "ai", "enables", "fixed", "physics", "shards", "tasks", "update", "layout+ui", "bodies", "fx",
    "r:nodes", "r:effects", "r:sort", "r:overlay", "r:shards", "r:hum",
];
pub const N: usize = PHASES.len();

#[derive(Default)]
pub struct Prof {
    mark: f64,
    /// this tick / this frame, per phase
    cur: [f64; N],
    /// totals and worst single tick or frame since the last read
    sum: [f64; N],
    max: [f64; N],
    pub ticks: u32,
    pub frames: u32,
}

impl Prof {
    pub fn begin(&mut self) {
        self.mark = now();
        self.cur = [0.0; N];
    }

    /// Charge the time since the last mark to phase `i`.
    pub fn lap(&mut self, i: usize) {
        let t = now();
        self.cur[i] += t - self.mark;
        self.mark = t;
    }

    /// Fold the current tick (or frame) into the totals.
    pub fn end(&mut self, range: std::ops::Range<usize>, tick: bool) {
        for i in range {
            self.sum[i] += self.cur[i];
            self.max[i] = self.max[i].max(self.cur[i]);
        }
        if tick {
            self.ticks += 1;
        } else {
            self.frames += 1;
        }
    }

    /// [ticks, frames, then per phase: mean ms per tick/frame, worst ms], and reset.
    pub fn take(&mut self) -> Vec<f32> {
        let mut out = vec![self.ticks as f32, self.frames as f32];
        for i in 0..N {
            let n = if i < 10 { self.ticks } else { self.frames }.max(1) as f64;
            out.push((self.sum[i] / n) as f32);
            out.push(self.max[i] as f32);
        }
        *self = Prof::default();
        out
    }
}
