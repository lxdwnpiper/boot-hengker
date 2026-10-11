use std::{collections::HashSet, fs, ptr};

use rustix::{
    io::Errno,
    ioctl::{Getter, IntegerSetter, Opcode, Updater, ioctl},
    stdio::stdin,
    termios::ttyname,
};

const GIO_UNIMAP: Opcode = 0x4B66;
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
#[derive(Clone, Copy, Default)]
struct UnicodePair {
    unicode: u16,
    _font_position: u16,
}

#[repr(C)]
struct UnicodeMap {
    count: u16,
    pairs: *mut UnicodePair,
}

fn read_unicode_map(map: &mut UnicodeMap) -> Result<(), Errno> {
    unsafe { ioctl(stdin(), Updater::<GIO_UNIMAP, UnicodeMap>::new(map)) }
}

pub fn font_characters() -> Option<HashSet<char>> {
    let mut probe = UnicodeMap {
        count: 0,
        pairs: ptr::null_mut(),
    };
    let _ = read_unicode_map(&mut probe);
    if probe.count == 0 {
        return None;
    }
    let mut pairs = vec![UnicodePair::default(); usize::from(probe.count)];
    let mut map = UnicodeMap {
        count: probe.count,
        pairs: pairs.as_mut_ptr(),
    };
    read_unicode_map(&mut map).ok()?;
    Some(
        pairs
            .iter()
            .take(usize::from(map.count))
            .filter_map(|pair| char::from_u32(u32::from(pair.unicode)))
            .collect(),
    )
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
