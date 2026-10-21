use std::{collections::HashSet, fs, ptr};

use rustix::{
    io::Errno,
    ioctl::{Getter, IntegerSetter, Opcode, Updater, ioctl},
    stdio::stdin,
    termios::ttyname,
};

use crate::{console_session::ask_password, screen_settings::font_glyphs::Glyphs};

const GIO_UNIMAP: Opcode = 0x4B66;
const VT_GETSTATE: Opcode = 0x5603;
const VT_ACTIVATE: Opcode = 0x5606;
const VIRTUAL_TERMINAL_PREFIX: &str = "/dev/tty";
const KERNEL_PRINTK: &str = "/proc/sys/kernel/printk";
const ALERTS_ONLY: &str = "1";
pub struct Console {
    quiet_kernel: Option<QuietKernelConsole>,
    foreground: Option<ForegroundVt>,
    for_system: bool,
    pub glyphs: Glyphs,
    pub bold_is_brightness: bool,
}

impl Console {
    pub fn take(for_system: bool) -> Self {
        let prompt_elsewhere = for_system && ask_password::requested();
        if prompt_elsewhere {
            eprintln!("bootvine: a password prompt is already showing, staying behind it");
        }
        Self {
            quiet_kernel: for_system.then(QuietKernelConsole::take).flatten(),
            foreground: if prompt_elsewhere {
                None
            } else {
                ForegroundVt::take()
            },
            for_system,
            glyphs: font_glyphs(),
            bold_is_brightness: own_vt().is_some(),
        }
    }

    #[must_use]
    pub fn answers_password_prompts(&self) -> bool {
        self.for_system && self.foreground.is_some()
    }

    pub fn come_forward_once_prompts_are_gone(&mut self) -> bool {
        if !self.for_system || self.foreground.is_some() || ask_password::requested() {
            return false;
        }
        self.foreground = ForegroundVt::take();
        if self.foreground.is_some() {
            eprintln!("bootvine: password prompt answered, taking the console back");
        }
        self.foreground.is_some()
    }

    pub fn keep_up(&mut self) {
        if let Some(quiet_kernel) = &mut self.quiet_kernel {
            quiet_kernel.keep_quiet();
        }
        if let Some(foreground) = &self.foreground {
            foreground.insist();
        }
        self.glyphs = font_glyphs();
    }
}

struct QuietKernelConsole {
    previous_level: String,
}

fn console_level() -> Option<String> {
    let levels = fs::read_to_string(KERNEL_PRINTK).ok()?;
    levels.split_whitespace().next().map(ToOwned::to_owned)
}

impl QuietKernelConsole {
    fn take() -> Option<Self> {
        let previous_level = console_level()?;
        fs::write(KERNEL_PRINTK, ALERTS_ONLY).ok()?;
        Some(Self { previous_level })
    }

    fn keep_quiet(&mut self) {
        let Some(level) = console_level() else {
            return;
        };
        if level != ALERTS_ONLY && fs::write(KERNEL_PRINTK, ALERTS_ONLY).is_ok() {
            self.previous_level = level;
        }
    }
}

fn font_glyphs() -> Glyphs {
    font_characters().map_or_else(Glyphs::default, |characters| {
        Glyphs::fitting(|symbol| symbol.is_ascii() || characters.contains(&symbol))
    })
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

fn font_characters() -> Option<HashSet<char>> {
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

struct ForegroundVt {
    own: u16,
    previous: u16,
}

fn active_vt() -> Option<u16> {
    let state: VtState = unsafe { ioctl(stdin(), Getter::<VT_GETSTATE, VtState>::new()) }.ok()?;
    Some(state.active)
}

fn own_vt() -> Option<u16> {
    let name = ttyname(stdin(), Vec::new()).ok()?;
    name.to_str()
        .ok()?
        .strip_prefix(VIRTUAL_TERMINAL_PREFIX)?
        .parse()
        .ok()
        .filter(|number| *number > 0)
}

impl ForegroundVt {
    fn take() -> Option<Self> {
        let own = own_vt()?;
        let previous = active_vt()?;
        if previous != own {
            activate(own).ok()?;
        }
        Some(Self { own, previous })
    }

    fn insist(&self) {
        if active_vt().is_some_and(|active| active != self.own) {
            let _ = activate(self.own);
        }
    }
}

impl Drop for ForegroundVt {
    fn drop(&mut self) {
        if ask_password::requested() {
            eprintln!("bootvine: leaving a password prompt to the console agent on this terminal");
            return;
        }
        let _ = activate(self.previous);
    }
}

fn activate(number: u16) -> Result<(), Errno> {
    let number = usize::from(number);
    unsafe { ioctl(stdin(), IntegerSetter::<VT_ACTIVATE>::new_usize(number)) }
}
