//! Tiny helpers for progress spinners and bars.
//!
//! We deliberately keep this small.  The full rattler binary wraps
//! `indicatif::MultiProgress` with a custom log writer so that tracing output
//! doesn't interleave with spinners; for a teaching project a simple spinner
//! per operation is sufficient.

use std::borrow::Cow;
use std::future::IntoFuture;
use std::time::Duration;

use indicatif::{ProgressBar, ProgressStyle};

/// Spinner style shared across the codebase.
pub fn spinner_style() -> ProgressStyle {
    ProgressStyle::with_template("{spinner:.green} {msg}")
        .unwrap()
        // braille dots feel snappy even at 10 fps
        .tick_strings(&["⠋", "⠙", "⠸", "⠴", "⠦", "⠇", "⠋"])
}

/// Run an async future while displaying a spinner with `msg`.
///
/// The spinner is cleared once the future resolves, regardless of whether it
/// succeeded or failed.
pub async fn with_spinner<T, F>(msg: impl Into<Cow<'static, str>>, fut: F) -> T
where
    F: IntoFuture<Output = T>,
{
    let pb = ProgressBar::new_spinner();
    pb.enable_steady_tick(Duration::from_millis(80));
    pb.set_style(spinner_style());
    pb.set_message(msg);
    let result = fut.into_future().await;
    pb.finish_and_clear();
    result
}

/// Run a synchronous closure while displaying a spinner with `msg`.
pub fn with_spinner_sync<T, F: FnOnce() -> T>(msg: impl Into<Cow<'static, str>>, f: F) -> T {
    let pb = ProgressBar::new_spinner();
    pb.enable_steady_tick(Duration::from_millis(80));
    pb.set_style(spinner_style());
    pb.set_message(msg);
    let result = f();
    pb.finish_and_clear();
    result
}
