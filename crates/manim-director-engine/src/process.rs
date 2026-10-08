//! Bounded waits for the short helper processes the engine runs itself
//! (ffprobe, the Python syntax check).

use std::{
    io,
    process::{Child, ExitStatus},
    thread,
    time::{Duration, Instant},
};

const POLL: Duration = Duration::from_millis(10);

/// Waits for `child` until `timeout`, then kills it; `None` means it timed out.
pub(crate) fn wait_bounded(child: &mut Child, timeout: Duration) -> io::Result<Option<ExitStatus>> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(None);
        }
        thread::sleep(POLL);
    }
}
