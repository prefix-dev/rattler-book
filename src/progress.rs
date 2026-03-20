// ~/~ begin <<book/src/ch04-search.md#src/progress.rs>>[init]
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

// ~/~ begin <<book/src/ch04-search.md#with-spinner>>[init]
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
// ~/~ end

// ~/~ begin <<book/src/ch06-install.md#with-spinner-sync>>[init]
pub fn with_spinner_sync<T, F: FnOnce() -> T>(msg: impl Into<Cow<'static, str>>, f: F) -> T {
    let pb = ProgressBar::new_spinner();
    pb.enable_steady_tick(Duration::from_millis(80));
    pb.set_style(spinner_style());
    pb.set_message(msg);
    let result = f();
    pb.finish_and_clear();
    result
}
// ~/~ end
// ~/~ end
