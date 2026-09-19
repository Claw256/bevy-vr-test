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

fn hold_the_frame(pace: Res<FramePace>, mut deadline: Local<Option<Instant>>) {
    let Some(interval) = pace.interval() else {
        *deadline = None;
        return;
    };

    let now = Instant::now();
    let next = match *deadline {
        // More than a whole interval behind — a hitch, a breakpoint, or the
        // rate just changed. Resync rather than catching up through a burst of
        // zero-length frames.
        Some(previous) if previous + interval >= now => previous + interval,
        _ => {
            *deadline = Some(now);
            return;
        }
    };
    *deadline = Some(next);

    if let Some(remaining) = next.checked_duration_since(now) {
        spin_sleep(remaining);
    }
}

/// Sleep most of the way, then spin.
///
/// `thread::sleep` routinely overshoots by a millisecond or more, which is most
/// of the budget at 120 Hz; spinning the last stretch keeps the deadline exact
/// without burning a core for the whole wait.
fn spin_sleep(duration: Duration) {
    const SPIN_FOR: Duration = Duration::from_micros(1200);

    let deadline = Instant::now() + duration;
    if let Some(coarse) = duration.checked_sub(SPIN_FOR) {
        std::thread::sleep(coarse);
    }
    while Instant::now() < deadline {
        std::hint::spin_loop();
    }
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
    fn spin_sleep_does_not_return_early() {
        let want = Duration::from_millis(3);
        let start = Instant::now();
        spin_sleep(want);
        assert!(start.elapsed() >= want, "slept {:?}", start.elapsed());
    }
}
