//! Build Mode frame pacing. A game must draw every frame; an editor only
//! needs to draw when something changes. While the mouse, keyboard or an
//! animation is busy the editor draws at up to `ACTIVE_FPS`; when nothing
//! happens it drops to a few frames a second (the clock, loading and
//! autosave keep going), and to one a second in the background.
use std::time::{Duration, Instant};

/// Most frames a second while the editor is in use (the frame rate limit
/// setting can make it lower).
pub const ACTIVE_FPS: u32 = 60;
/// How long after the last input the editor keeps drawing at full rate.
const ACTIVE_AFTER_INPUT: Duration = Duration::from_millis(500);
/// Between frames when nothing happens and the window has focus.
const IDLE: Duration = Duration::from_millis(500);
/// Between frames when the window is in the background, minimised or covered.
const BACKGROUND: Duration = Duration::from_secs(1);

pub struct Pacing {
    last_input: Instant,
    last_frame: Instant,
    /// When egui wants its next frame (tooltip delays, animations,
    /// thumbnails still to make). None: egui is happy to wait.
    pub repaint: Option<Instant>,
    pub focused: bool,
    /// Minimised or fully covered: nothing on screen to update.
    pub hidden: bool,
}

impl Default for Pacing {
    fn default() -> Self {
        let now = Instant::now();
        Self {
            last_input: now,
            last_frame: now,
            repaint: None,
            focused: true,
            hidden: false,
        }
    }
}

/// Time between frames while the editor is in use.
pub fn active_interval(fps_limit: u32) -> Duration {
    let fps = match fps_limit {
        0 => ACTIVE_FPS,
        limit => limit.min(ACTIVE_FPS),
    };
    Duration::from_secs_f64(1.0 / f64::from(fps))
}

impl Pacing {
    /// Mouse, keyboard or window input arrived: the earliest time the next
    /// frame may be drawn (at once, unless that would beat the frame cap).
    pub fn input(&mut self, now: Instant, fps_limit: u32) -> Instant {
        self.last_input = now;
        (self.last_frame + active_interval(fps_limit)).max(now)
    }
    /// A frame was just drawn at `now`; when the next one is due. `busy`:
    /// the editor itself is moving something (flying the camera, dragging
    /// an object or a gizmo) or the world is still loading.
    pub fn frame_drawn(&mut self, now: Instant, fps_limit: u32, busy: bool) -> Instant {
        self.last_frame = now;
        let fast = now + active_interval(fps_limit);
        if self.hidden {
            return now + BACKGROUND;
        }
        if busy || now.duration_since(self.last_input) < ACTIVE_AFTER_INPUT {
            return fast;
        }
        let idle = now + if self.focused { IDLE } else { BACKGROUND };
        match self.repaint {
            Some(at) => at.clamp(fast, idle),
            None => idle,
        }
    }
}

/// When egui asks for its next frame, from the delay it reports.
pub fn repaint_at(now: Instant, delay: Duration) -> Option<Instant> {
    now.checked_add(delay)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pacing(now: Instant) -> Pacing {
        Pacing {
            last_input: now,
            last_frame: now,
            repaint: None,
            focused: true,
            hidden: false,
        }
    }

    #[test]
    fn full_rate_right_after_input_then_idle() {
        let start = Instant::now();
        let mut p = pacing(start);
        let next = p.frame_drawn(start, 0, false);
        assert_eq!(next, start + active_interval(0));
        let later = start + Duration::from_secs(2);
        assert_eq!(p.frame_drawn(later, 0, false), later + IDLE);
    }

    #[test]
    fn busy_keeps_full_rate() {
        let start = Instant::now();
        let mut p = pacing(start);
        let later = start + Duration::from_secs(5);
        assert_eq!(p.frame_drawn(later, 0, true), later + active_interval(0));
    }

    #[test]
    fn cap_is_sixty_or_the_lower_limit() {
        assert_eq!(active_interval(0), Duration::from_secs_f64(1.0 / 60.0));
        assert_eq!(active_interval(240), Duration::from_secs_f64(1.0 / 60.0));
        assert_eq!(active_interval(30), Duration::from_secs_f64(1.0 / 30.0));
    }

    #[test]
    fn background_and_hidden_are_slow() {
        let start = Instant::now();
        let mut p = pacing(start);
        p.focused = false;
        let later = start + Duration::from_secs(2);
        assert_eq!(p.frame_drawn(later, 0, false), later + BACKGROUND);
        p.hidden = true;
        // Even busy: a minimised window has nothing to show.
        assert_eq!(p.frame_drawn(later, 0, true), later + BACKGROUND);
    }

    #[test]
    fn egui_repaint_is_kept_within_cap_and_idle() {
        let start = Instant::now();
        let mut p = pacing(start);
        let later = start + Duration::from_secs(2);
        p.repaint = Some(later);
        assert_eq!(p.frame_drawn(later, 0, false), later + active_interval(0));
        p.repaint = Some(later + Duration::from_millis(200));
        assert_eq!(
            p.frame_drawn(later, 0, false),
            later + Duration::from_millis(200)
        );
        p.repaint = repaint_at(later, Duration::MAX);
        assert_eq!(p.frame_drawn(later, 0, false), later + IDLE);
    }

    #[test]
    fn input_wakes_at_once_but_respects_the_cap() {
        let start = Instant::now();
        let mut p = pacing(start);
        p.frame_drawn(start, 0, false);
        let soon = start + Duration::from_millis(5);
        assert_eq!(p.input(soon, 0), start + active_interval(0));
        let late = start + Duration::from_secs(1);
        assert_eq!(p.input(late, 0), late);
    }
}
