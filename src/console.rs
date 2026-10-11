use std::{collections::HashSet, fs, ptr};

use bootvine::glyphs::Glyphs;
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
const PASSWORD_REQUESTS: &str = "/run/systemd/ask-password";
const PASSWORD_REQUEST_PREFIX: &str = "ask.";

pub struct Console {
    quiet_kernel: Option<QuietKernelConsole>,
    foreground: Option<ForegroundVt>,
    yields_to_password_prompts: bool,
    pub glyphs: Glyphs,
}

impl Console {
    pub fn take(for_system: bool) -> Self {
        let prompt_showing = for_system && password_requested();
        Self {
            quiet_kernel: for_system.then(QuietKernelConsole::take).flatten(),
            foreground: if prompt_showing {
                None
            } else {
                ForegroundVt::take()
            },
            yields_to_password_prompts: for_system,
            glyphs: font_glyphs(),
        }
    }

    pub fn make_way_for_password_prompts(&mut self) {
        if !self.yields_to_password_prompts {
            return;
        }
        if password_requested() {
            self.step_aside();
        } else {
            self.come_forward();
        }
    }

    fn step_aside(&mut self) {
        if self.foreground.take().is_some() {
            eprintln!("bootvine: stepping aside for a password prompt");
        }
    }

    fn come_forward(&mut self) {
        if self.foreground.is_some() {
            return;
        }
        self.foreground = ForegroundVt::take();
        if self.foreground.is_some() {
            eprintln!("bootvine: password prompt answered, taking the console back");
        }
    }

    pub fn keep_up(&mut self) {
        if let Some(quiet_kernel) = &mut self.quiet_kernel {
            quiet_kernel.keep_quiet();
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

fn password_requested() -> bool {
    fs::read_dir(PASSWORD_REQUESTS).is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(PASSWORD_REQUEST_PREFIX)
        })
    })
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
    previous: u16,
}

impl ForegroundVt {
    fn take() -> Option<Self> {
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
