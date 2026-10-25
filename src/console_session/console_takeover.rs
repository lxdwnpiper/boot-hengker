use std::{collections::HashSet, fs, ptr};

use rustix::{
    io::Errno,
    ioctl::{Getter, IntegerSetter, Opcode, Updater, ioctl},
    stdio::stdin,
    termios::ttyname,
};

use crate::{
    console_session::{ask_password, console_palette::ConsolePalette},
    screen_settings::font_glyphs::Glyphs,
};

const GIO_UNIMAP: Opcode = 0x4B66;
const VT_GETSTATE: Opcode = 0x5603;
const VT_ACTIVATE: Opcode = 0x5606;
const VIRTUAL_TERMINAL_PREFIX: &str = "/dev/tty";
const KERNEL_PRINTK: &str = "/proc/sys/kernel/printk";
const ALERTS_ONLY: &str = "1";
const HIGHEST_LOW_GLYPH: u16 = 255;
pub struct Console {
    quiet_kernel: Option<QuietKernelConsole>,
    foreground: Option<ForegroundVt>,
    for_system: bool,
    own_vt: Option<u16>,
    pub glyphs: Glyphs,
    pub palette: ConsolePalette,
}

impl Console {
    pub fn take(for_system: bool) -> Self {
        let prompt_elsewhere = for_system && ask_password::requested();
        if prompt_elsewhere {
            eprintln!("bootvine: a password prompt is already showing, staying behind it");
        }
        let own_vt = vt_of_stdin();
        let font = loaded_font();
        Self {
            quiet_kernel: for_system.then(QuietKernelConsole::take).flatten(),
            foreground: own_vt
                .filter(|_| !prompt_elsewhere)
                .and_then(ForegroundVt::take),
            for_system,
            own_vt,
            glyphs: font_glyphs(font.as_ref()),
            palette: console_palette(own_vt, font.as_ref()),
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
        self.foreground = self.own_vt.and_then(ForegroundVt::take);
        if self.foreground.is_some() {
            eprintln!("bootvine: password prompt answered, taking the console back");
        }
        self.foreground.is_some()
    }

    pub fn keep_up(&mut self) -> bool {
        let kernel_got_loud = self
            .quiet_kernel
            .as_mut()
            .is_some_and(QuietKernelConsole::keep_quiet);
        let terminal_was_taken = self.foreground.as_ref().is_some_and(ForegroundVt::insist);
        let font = loaded_font();
        let glyphs = font_glyphs(font.as_ref());
        let palette = console_palette(self.own_vt, font.as_ref());
        let font_changed = glyphs != self.glyphs || palette != self.palette;
        self.glyphs = glyphs;
        self.palette = palette;
        kernel_got_loud || terminal_was_taken || font_changed
    }

    pub fn begin_handover(&mut self) {
        self.set_grip(TerminalGrip::Yielding);
    }

    #[must_use]
    pub fn console_was_taken(&self) -> bool {
        self.foreground
            .as_ref()
            .is_some_and(ForegroundVt::taken_away)
    }

    pub fn leave_console(&mut self) {
        self.set_grip(TerminalGrip::Surrendered);
    }

    fn set_grip(&mut self, grip: TerminalGrip) {
        if let Some(foreground) = &mut self.foreground {
            foreground.grip = grip;
        }
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

    fn keep_quiet(&mut self) -> bool {
        let Some(level) = console_level() else {
            return false;
        };
        if level == ALERTS_ONLY || fs::write(KERNEL_PRINTK, ALERTS_ONLY).is_err() {
            return false;
        }
        self.previous_level = level;
        true
    }
}

struct LoadedFont {
    characters: HashSet<char>,
    uses_high_glyphs: bool,
}

fn font_glyphs(font: Option<&LoadedFont>) -> Glyphs {
    font.map_or_else(Glyphs::default, |font| {
        Glyphs::fitting(|symbol| symbol.is_ascii() || font.characters.contains(&symbol))
    })
}

fn console_palette(own_vt: Option<u16>, font: Option<&LoadedFont>) -> ConsolePalette {
    if own_vt.is_none() {
        return ConsolePalette::Terminal;
    }
    ConsolePalette::of_virtual_terminal(font.is_some_and(|font| font.uses_high_glyphs))
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
    font_position: u16,
}

#[repr(C)]
struct UnicodeMap {
    count: u16,
    pairs: *mut UnicodePair,
}

fn read_unicode_map(map: &mut UnicodeMap) -> Result<(), Errno> {
    unsafe { ioctl(stdin(), Updater::<GIO_UNIMAP, UnicodeMap>::new(map)) }
}

fn loaded_font() -> Option<LoadedFont> {
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
    let mapped = &pairs[..usize::from(map.count).min(pairs.len())];
    Some(LoadedFont {
        characters: mapped
            .iter()
            .filter_map(|pair| char::from_u32(u32::from(pair.unicode)))
            .collect(),
        uses_high_glyphs: mapped
            .iter()
            .any(|pair| pair.font_position > HIGHEST_LOW_GLYPH),
    })
}

#[repr(C)]
struct VtState {
    active: u16,
    _signal: u16,
    _state: u16,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum TerminalGrip {
    Insisting,
    Yielding,
    Surrendered,
}

struct ForegroundVt {
    own: u16,
    previous: u16,
    grip: TerminalGrip,
}

fn active_vt() -> Option<u16> {
    let state: VtState = unsafe { ioctl(stdin(), Getter::<VT_GETSTATE, VtState>::new()) }.ok()?;
    Some(state.active)
}

fn vt_of_stdin() -> Option<u16> {
    let name = ttyname(stdin(), Vec::new()).ok()?;
    name.to_str()
        .ok()?
        .strip_prefix(VIRTUAL_TERMINAL_PREFIX)?
        .parse()
        .ok()
        .filter(|number| *number > 0)
}

impl ForegroundVt {
    fn take(own: u16) -> Option<Self> {
        let previous = active_vt()?;
        if previous != own {
            activate(own).ok()?;
        }
        Some(Self {
            own,
            previous,
            grip: TerminalGrip::Insisting,
        })
    }

    fn insist(&self) -> bool {
        self.grip == TerminalGrip::Insisting && self.taken_away() && activate(self.own).is_ok()
    }

    fn taken_away(&self) -> bool {
        active_vt().is_some_and(|active| active != self.own)
    }
}

impl Drop for ForegroundVt {
    fn drop(&mut self) {
        if ask_password::requested() {
            eprintln!("bootvine: leaving a password prompt to the console agent on this terminal");
            return;
        }
        if self.grip == TerminalGrip::Surrendered {
            return;
        }
        let _ = activate(self.previous);
    }
}

fn activate(number: u16) -> Result<(), Errno> {
    let number = usize::from(number);
    unsafe { ioctl(stdin(), IntegerSetter::<VT_ACTIVATE>::new_usize(number)) }
}
