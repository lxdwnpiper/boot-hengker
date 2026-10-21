use std::time::Duration;

use rustix::time::{ClockId, clock_gettime};

pub fn monotonic_now() -> Duration {
    let now = clock_gettime(ClockId::Monotonic);
    Duration::new(
        now.tv_sec.cast_unsigned(),
        u32::try_from(now.tv_nsec).unwrap_or_default(),
    )
}
