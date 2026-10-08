//! CPU-side timings. These are not GPU timestamp measurements.
use std::{collections::VecDeque, time::Instant};
pub struct Metrics {
    pub visible: bool,
    memory: Option<u64>,
    memory_sample: Instant,
    last: Instant,
    frames: VecDeque<f64>,
    pub stream_ms: f64,
    pub update_ms: f64,
    pub render_ms: f64,
}
impl Default for Metrics {
    fn default() -> Self {
        Self {
            visible: false,
            memory: None,
            memory_sample: Instant::now() - std::time::Duration::from_secs(2),
            last: Instant::now(),
            frames: VecDeque::with_capacity(120),
            stream_ms: 0.,
            update_ms: 0.,
            render_ms: 0.,
        }
    }
}
impl Metrics {
    pub fn frame(&mut self) {
        let now = Instant::now();
        if self.memory_sample.elapsed().as_secs() >= 1 {
            self.memory = resident_bytes();
            self.memory_sample = now;
        }
        self.frames
            .push_back((now - self.last).as_secs_f64() * 1000.);
        self.last = now;
        if self.frames.len() > 120 {
            self.frames.pop_front();
        }
    }
    pub fn text(&self, batches: usize, loading: bool, server: bool, retained: bool) -> String {
        let mut frames: Vec<_> = self.frames.iter().copied().collect();
        frames.sort_by(f64::total_cmp);
        let p99 = frames
            .get((frames.len() * 99).div_ceil(100).saturating_sub(1))
            .copied()
            .unwrap_or(0.);
        let mean = frames.iter().sum::<f64>() / frames.len().max(1) as f64;
        format!("F3 performance | frame {:.1} ms / p99 {:.1} ms\nCPU update {:.1} ms (stream {:.1}) | render/present {:.1} ms\n{} batches | {} | {}{}\nResident memory {} | GPU timestamps: not sampled",mean,p99,self.update_ms,self.stream_ms,self.render_ms,batches,if loading{"streaming"}else{"region ready"},if server{"server resources"}else{"local resources"},if retained{"; offline world retained"}else{""},self.memory.map(|b|format!("{:.0} MiB",b as f64/1048576.)).unwrap_or_else(||"unavailable".into()))
    }
}

/// Resident working set of this process, not total GPU allocation.
pub fn resident_bytes() -> Option<u64> {
    #[cfg(windows)]
    {
        #[repr(C)]
        struct Counters {
            size: u32,
            faults: u32,
            peak: usize,
            working: usize,
            pool_peak: usize,
            pool: usize,
            nonpaged_peak: usize,
            nonpaged: usize,
            pagefile: usize,
            pagefile_peak: usize,
        }
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetCurrentProcess() -> *mut std::ffi::c_void;
        }
        #[link(name = "psapi")]
        unsafe extern "system" {
            fn GetProcessMemoryInfo(
                process: *mut std::ffi::c_void,
                counters: *mut Counters,
                size: u32,
            ) -> i32;
        }
        let mut counters = Counters {
            size: std::mem::size_of::<Counters>() as u32,
            faults: 0,
            peak: 0,
            working: 0,
            pool_peak: 0,
            pool: 0,
            nonpaged_peak: 0,
            nonpaged: 0,
            pagefile: 0,
            pagefile_peak: 0,
        };
        // Both calls receive the current process pseudo-handle and an initialized ABI-sized buffer.
        (unsafe {
            GetProcessMemoryInfo(
                GetCurrentProcess(),
                &mut counters,
                std::mem::size_of::<Counters>() as u32,
            )
        } != 0)
            .then_some(counters.working as u64)
    }
    #[cfg(target_os = "linux")]
    {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        status.lines().find_map(|l| {
            l.strip_prefix("VmRSS:")?
                .split_whitespace()
                .next()?
                .parse::<u64>()
                .ok()
                .map(|kb| kb * 1024)
        })
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        None
    }
}
pub fn session_memory(phase: &str) {
    if let Some(bytes) = resident_bytes() {
        eprintln!(
            "Session memory ({phase}): {:.1} MiB resident",
            bytes as f64 / 1048576.
        );
    }
}
