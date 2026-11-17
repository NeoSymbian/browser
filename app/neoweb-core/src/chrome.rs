// SPDX-License-Identifier: Apache-2.0
//! The chrome's behaviour (`br2-spec-chrome`): hiding while reading (B02) and the loading line (B01).

/// Header and footer hide after 24 dp of scrolling down and show after 24 dp up (one gesture, summed since the last
/// change of direction); near the top they always show.
#[derive(Debug, Clone, Copy)]
pub struct ChromeScroll {
    shown: bool,
    last: f32,
    /// travel since the direction last changed: + down, − up
    run: f32,
}

pub const HIDE_DP: f32 = 24.0;
/// Under this scroll offset the chrome always shows.
pub const TOP_DP: f32 = 48.0;

impl Default for ChromeScroll {
    fn default() -> Self {
        Self {
            shown: true,
            last: 0.0,
            run: 0.0,
        }
    }
}

impl ChromeScroll {
    pub fn shown(&self) -> bool {
        self.shown
    }

    /// The page scrolled to `offset` dp from its top; true if the chrome changed.
    pub fn scrolled(&mut self, offset: f32) -> bool {
        let d = offset - self.last;
        self.last = offset;
        if d == 0.0 {
            return false;
        }
        if (d > 0.0) != (self.run > 0.0) {
            self.run = 0.0;
        }
        self.run += d;
        let want = if offset < TOP_DP {
            true
        } else if self.run >= HIDE_DP {
            false
        } else if self.run <= -HIDE_DP {
            true
        } else {
            self.shown
        };
        let changed = want != self.shown;
        self.shown = want;
        changed
    }

    /// Show it now (loading, editing, a dialog, after ← or →, a new page); scrolling starts afresh from `offset`.
    pub fn show(&mut self, offset: f32) {
        *self = Self {
            shown: true,
            last: offset,
            run: 0.0,
        };
    }
}

/// The stand-in engine's load: the progress line runs 0.1 → 1 over a few ticks (100 ms each), so the chrome's
/// loading state (✕ stop, the line) is real before Servo reports progress (WB4).
#[derive(Debug, Clone, Copy, Default)]
pub struct Loading {
    ticks: u32,
    active: bool,
}

const LOAD_TICKS: u32 = 6;

impl Loading {
    pub fn start(&mut self) {
        *self = Self {
            ticks: 0,
            active: true,
        };
    }
    pub fn stop(&mut self) {
        self.active = false;
    }
    pub fn active(&self) -> bool {
        self.active
    }
    pub fn progress(&self) -> f32 {
        0.1 + 0.9 * self.ticks as f32 / LOAD_TICKS as f32
    }
    /// One tick; true when the load just ended.
    pub fn tick(&mut self) -> bool {
        if !self.active {
            return false;
        }
        self.ticks += 1;
        if self.ticks >= LOAD_TICKS {
            self.active = false;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hides_after_24_down_and_shows_after_24_up() {
        let mut c = ChromeScroll::default();
        assert!(!c.scrolled(40.0) && c.shown(), "near the top it stays");
        assert!(
            c.scrolled(100.0) && !c.shown(),
            "60 dp down past the top hides it"
        );
        assert!(!c.scrolled(90.0) && !c.shown(), "10 dp up is not enough");
        assert!(
            c.scrolled(70.0) && c.shown(),
            "30 dp up in one run shows it"
        );
        c.scrolled(80.0);
        c.scrolled(90.0);
        assert!(c.shown(), "two 10 dp steps down: 20 dp, still shown");
        c.scrolled(95.0);
        assert!(!c.shown(), "25 dp summed: hidden");
        assert!(c.scrolled(30.0) && c.shown(), "back near the top");
    }

    #[test]
    fn show_resets_the_run() {
        let mut c = ChromeScroll::default();
        c.scrolled(200.0);
        assert!(!c.shown());
        c.show(200.0);
        assert!(c.shown() && !c.scrolled(210.0) && c.shown());
    }

    #[test]
    fn loading_runs_then_ends() {
        let mut l = Loading::default();
        l.start();
        assert!(l.active() && l.progress() < 0.2);
        let ends = (0..10).filter(|_| l.tick()).count();
        assert_eq!(ends, 1);
        assert!(!l.active());
        l.start();
        l.stop();
        assert!(!l.active() && !l.tick());
    }
}
