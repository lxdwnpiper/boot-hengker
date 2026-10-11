use boot_tree::model::Micros;
use rustix::{
    system::uname,
    time::{ClockId, clock_gettime},
};

pub fn monotonic_now() -> Micros {
    let now = clock_gettime(ClockId::Monotonic);
    now.tv_sec as Micros * 1_000_000 + now.tv_nsec as Micros / 1_000
}

pub fn host_name() -> String {
    uname().nodename().to_string_lossy().into_owned()
}
