use std::{os::fd::OwnedFd, path::Path};

use rustix::{
    fs::{Mode, OFlags, open},
    io::Errno,
    process::{chdir, chroot, fchdir},
};

const SYSROOT: &str = "/sysroot";
const INITRD_RELEASE: &str = "/etc/initrd-release";

pub fn running_in_initrd() -> bool {
    Path::new(INITRD_RELEASE).exists()
}

pub struct Sysroot {
    directory: OwnedFd,
}

impl Sysroot {
    pub fn hold() -> Result<Self, Errno> {
        let directory = open(
            SYSROOT,
            OFlags::DIRECTORY | OFlags::RDONLY | OFlags::CLOEXEC,
            Mode::empty(),
        )?;
        Ok(Self { directory })
    }

    pub fn enter(self) -> Result<(), Errno> {
        fchdir(&self.directory)?;
        chroot(".")?;
        chdir("/")
    }
}
