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
        let own = own_vt_number()?;
        let previous = active_vt().ok()?;
        if previous != own {
            activate(own).ok()?;
        }
        Some(Self { previous })
    }

    pub fn give_back(self) {
        let _ = activate(self.previous);
    }
}

fn own_vt_number() -> Option<u16> {
    let name = ttyname(stdin(), Vec::new()).ok()?;
    name.to_str()
        .ok()?
        .strip_prefix(VIRTUAL_TERMINAL_PREFIX)?
        .parse()
        .ok()
        .filter(|number| *number > 0)
}

fn active_vt() -> Result<u16, Errno> {
    let state: VtState = unsafe { ioctl(stdin(), Getter::<VT_GETSTATE, VtState>::new()) }?;
    Ok(state.active)
}

fn activate(number: u16) -> Result<(), Errno> {
    let number = usize::from(number);
    unsafe { ioctl(stdin(), IntegerSetter::<VT_ACTIVATE>::new_usize(number)) }?;
    unsafe { ioctl(stdin(), IntegerSetter::<VT_WAITACTIVE>::new_usize(number)) }?;
    Ok(())
}
