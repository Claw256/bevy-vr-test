//! Frame pacing.
//!
//! Without this, the app free-runs: it renders a frame in ~2 ms of CPU work,
//! queues it, renders another, and eventually blocks because the presentation
//! path will not take any more. Measured on this project that produced a
//! strongly bimodal frame time — most frames around 6.3 ms, one in five around
//! 16.8 ms — averaging out to the display's refresh interval but arriving
//! unevenly. An even *average* frame rate with uneven *spacing* is what
//! stuttering is, and because Bevy drives animation from `Time::delta`, the
//! unevenness goes straight into the motion.
//!
//! The cure is to stop outrunning the display. Each frame sleeps until an
//! absolute deadline that advances by exactly one interval, which paces
//! frame-to-frame spacing no matter where in the frame the sleep happens.
//!
//! Measured effect (debug build, GTX 960M, 119.93 Hz, 900 frames):
//!
//! | | p50 | p90 | frames over 15 ms | stdev |
//! | --- | --- | --- | --- | --- |
//! | off | 6.32 ms | 16.80 ms | 187 | 4.41 ms |
//! | on | 8.34 ms | 9.10 ms | 8 | 1.19 ms |
//! | on, v-sync off | 8.33 ms | 9.31 ms | 0 | 0.78 ms |
//!
//! Note what is *not* the cause: the physics step costs 0.55 ms, and the app's
//! whole `First..Last` CPU work is ~2 ms on fast and slow frames alike. The
//! difference between a 6 ms frame and a 17 ms one is entirely time blocked
//! outside the schedule. Pipelined rendering, present mode, swapchain queue
//! depth and fullscreen all left it unchanged.

use bevy::prelude::*;
use bevy::window::{Monitor, PrimaryMonitor};
use bevy_mod_xr::session::session_running;

use std::time::{Duration, Instant};

pub struct FramePacePlugin;

impl Plugin for FramePacePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FramePace>()
            .init_resource::<PaceTuning>()
            .add_systems(PostStartup, adopt_monitor_refresh)
            .add_systems(
                Last,
                // In VR the runtime paces the frame loop through `xrWaitFrame`,
                // and a second limiter would only fight it.
                hold_the_frame.run_if(not(session_running)),
            );
    }
}

/// How fast the flat-screen loop is allowed to run.
///
/// An enum rather than an `Option<f32>`, because "nobody has chosen a rate yet"
/// and "deliberately unpaced" need to be distinguishable — otherwise the
/// auto-detect cannot tell which it is looking at, and silently re-paces a
/// deliberately unpaced app.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub enum FramePace {
    /// Match the primary monitor, resolved once at startup.
    #[default]
    Auto,
    /// Pace to this many frames per second.
    Hz(f32),
    /// Run as fast as the machine allows. This is what stutters.
    Off,
}

impl FramePace {
    fn interval(&self) -> Option<Duration> {
        match *self {
            Self::Hz(hz) if hz > 0.0 => Some(Duration::from_secs_f64(1.0 / hz as f64)),
            _ => None,
        }
    }
}

/// Resolves [`FramePace::Auto`] against the monitor.
///
/// Runs in `PostStartup` because the monitor entities do not exist yet during
/// `Startup`.
fn adopt_monitor_refresh(
    mut pace: ResMut<FramePace>,
    primary: Query<&Monitor, With<PrimaryMonitor>>,
    any: Query<&Monitor>,
) {
    if *pace != FramePace::Auto {
        return;
    }

    let refresh = primary
        .iter()
        .chain(any.iter())
        .find_map(|monitor| monitor.refresh_rate_millihertz);

    *pace = match refresh {
        Some(millihertz) => {
            let hz = millihertz as f32 / 1000.0;
            info!("pacing frames to {hz:.2} Hz");
            FramePace::Hz(hz)
        }
        // Better unpaced than capped at an invented rate on a 240 Hz display.
        None => {
            warn!("no monitor refresh rate reported; frames will not be paced");
            FramePace::Off
        }
    };
}

/// State the limiter carries between frames.
#[derive(Default)]
struct Pacer {
    deadline: Option<Instant>,
    /// How late `thread::sleep` has been returning lately.
    ///
    /// The kernel wakes us when it feels like it, and under load that can be
    /// several milliseconds — most of a 120 Hz frame. This is what the last of
    /// the spikes were: measured over 2000 frames, the overshooting frames had
    /// ~2 ms of CPU work and no physics step, so the app was not running long,
    /// the sleep was coming back late.
    overshoot: Duration,
}

/// How much CPU the limiter may burn to hit its deadline.
///
/// Sleeping is cheap but imprecise; spinning is exact but burns a core. The
/// window between these two bounds is what the limiter is allowed to spin.
#[derive(Resource, Clone, Copy, Debug)]
pub struct PaceTuning {
    /// Always spin at least this long. Sub-millisecond wakeups are not reliable
    /// on a desktop kernel; a 600 us floor measured worse than the fixed 1.2 ms
    /// it replaced.
    pub min_spin: Duration,
    /// Never spin longer than this, however late the scheduler runs. On a
    /// laptop a busy core is heat and battery.
    pub max_spin: Duration,
}

impl Default for PaceTuning {
    fn default() -> Self {
        Self {
            min_spin: Duration::from_micros(1500),
            max_spin: Duration::from_micros(4000),
        }
    }
}

impl Pacer {
    fn margin(&self, tuning: &PaceTuning) -> Duration {
        self.overshoot.clamp(tuning.min_spin, tuning.max_spin)
    }

    /// Sleep until `deadline`, then spin out the remainder.
    ///
    /// The spin window tracks observed overshoot, growing the moment a wakeup
    /// is late and decaying while they are punctual, so a quiet machine barely
    /// spins and a busy one still makes its deadline.
    fn sleep_until(&mut self, deadline: Instant, tuning: &PaceTuning) {
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            return;
        };

        if let Some(coarse) = remaining.checked_sub(self.margin(tuning)) {
            let before = Instant::now();
            std::thread::sleep(coarse);
            let late = before.elapsed().saturating_sub(coarse);

            // Decaying maximum: jump to a late wakeup at once, forget it
            // slowly. A mean would sit under most overshoots and keep missing;
            // a plain maximum would never come back down after one bad frame.
            self.overshoot = late.max(self.overshoot.mul_f32(0.98));
        }

        while Instant::now() < deadline {
            std::hint::spin_loop();
        }
    }
}

fn hold_the_frame(
    pace: Res<FramePace>,
    tuning: Res<PaceTuning>,
    mut pacer: Local<Pacer>,
) {
    let Some(interval) = pace.interval() else {
        pacer.deadline = None;
        return;
    };

    let now = Instant::now();
    let next = match pacer.deadline {
        Some(previous) if previous + interval >= now => previous + interval,
        // Behind: restart from now, so the next frame gets a whole interval of
        // slack. The tidier-looking alternative — skipping whole intervals to
        // preserve phase — measured far worse, 163 spikes per 2000 frames
        // against 2, because it hands the next frame whatever fraction of an
        // interval happens to be left instead of a clean one.
        _ => {
            pacer.deadline = Some(now);
            return;
        }
    };
    pacer.deadline = Some(next);
    pacer.sleep_until(next, &tuning);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_and_off_both_mean_no_interval_but_are_not_the_same_thing() {
        assert!(FramePace::Auto.interval().is_none());
        assert!(FramePace::Off.interval().is_none());
        // The distinction is the point: auto-detect must re-pace Auto and must
        // leave Off alone.
        assert_ne!(FramePace::Auto, FramePace::Off);
    }

    #[test]
    fn a_nonsense_rate_is_ignored_rather_than_dividing_by_zero() {
        assert!(FramePace::Hz(0.0).interval().is_none());
        assert!(FramePace::Hz(-60.0).interval().is_none());
    }

    #[test]
    fn the_interval_is_the_reciprocal_of_the_rate() {
        let interval = FramePace::Hz(120.0)
            .interval()
            .expect("120 Hz should give an interval");
        assert!((interval.as_secs_f64() - 1.0 / 120.0).abs() < 1e-9);
    }

    #[test]
    fn sleeping_until_a_deadline_does_not_return_early() {
        let mut pacer = Pacer::default();
        let start = Instant::now();
        let deadline = start + Duration::from_millis(4);
        pacer.sleep_until(deadline, &PaceTuning::default());
        assert!(
            Instant::now() >= deadline,
            "returned {:?} early",
            deadline - Instant::now()
        );
    }

    #[test]
    fn a_deadline_already_past_returns_immediately() {
        let mut pacer = Pacer::default();
        let start = Instant::now();
        pacer.sleep_until(start - Duration::from_millis(5), &PaceTuning::default());
        assert!(start.elapsed() < Duration::from_millis(2));
    }

    #[test]
    fn the_spin_window_tracks_scheduler_overshoot() {
        let tuning = PaceTuning::default();
        let mut pacer = Pacer::default();
        for _ in 0..8 {
            pacer.sleep_until(Instant::now() + Duration::from_millis(3), &tuning);
        }
        // Whatever it learned, the spin window must stay inside the bounds:
        // never so small that a late wakeup blows the deadline, never so large
        // that the loop spins a core for the whole frame.
        let margin = pacer.margin(&tuning);
        assert!(margin >= tuning.min_spin && margin <= tuning.max_spin);
    }
}
