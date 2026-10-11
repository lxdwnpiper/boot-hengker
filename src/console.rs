use std::fs;

use rustix::{
    io::Errno,
    ioctl::{Getter, IntegerSetter, Opcode, ioctl},
    stdio::stdin,
    termios::ttyname,
};

const VT_GETSTATE: Opcode = 0x5603;
const VT_ACTIVATE: Opcode = 0x5606;
const VT_WAITACTIVE: Opcode = 0x5607;
const VIRTUAL_TERMINAL_PREFIX: &str = "/dev/tty";
const KERNEL_PRINTK: &str = "/proc/sys/kernel/printk";
const ALERTS_ONLY: &str = "1";

pub struct QuietKernelConsole {
    previous_level: String,
}

impl QuietKernelConsole {
    pub fn take() -> Option<Self> {
        let levels = fs::read_to_string(KERNEL_PRINTK).ok()?;
        let previous_level = levels.split_whitespace().next()?.to_owned();
        fs::write(KERNEL_PRINTK, ALERTS_ONLY).ok()?;
        Some(Self { previous_level })
    }
}

impl Drop for QuietKernelConsole {
    fn drop(&mut self) {
        let _ = fs::write(KERNEL_PRINTK, &self.previous_level);
    }
}

#[repr(C)]
struct VtState {
    active: u16,
    _signal: u16,
    _state: u16,
}

pub struct ForegroundVt {
    previous: u16,
}

impl ForegroundVt {
    pub fn take() -> Option<Self> {
        let name = ttyname(stdin(), Vec::new()).ok()?;
        let own: u16 = name
            .to_str()
            .ok()?
            .strip_prefix(VIRTUAL_TERMINAL_PREFIX)?
            .parse()
            .ok()
            .filter(|number| *number > 0)?;
        let state: VtState =
            unsafe { ioctl(stdin(), Getter::<VT_GETSTATE, VtState>::new()) }.ok()?;
        if state.active != own {
            activate(own).ok()?;
        }
        Some(Self {
            previous: state.active,
        })
    }
}

impl Drop for ForegroundVt {
    fn drop(&mut self) {
        let _ = activate(self.previous);
    }
}

fn activate(number: u16) -> Result<(), Errno> {
    let number = usize::from(number);
    unsafe { ioctl(stdin(), IntegerSetter::<VT_ACTIVATE>::new_usize(number)) }?;
    unsafe { ioctl(stdin(), IntegerSetter::<VT_WAITACTIVE>::new_usize(number)) }?;
    Ok(())
}
