use std::{
    fs::{self, Permissions},
    os::unix::fs::PermissionsExt,
    path::Path,
};

use xshell::{Shell, cmd};

use crate::task_error::TaskError;

const TAPES: [&str; 2] = ["boot.tape", "shutdown.tape"];

pub fn record_screencasts(passphrase_png: &Path) -> Result<(), TaskError> {
    let recordings = Path::new("recordings");
    if !recordings.is_dir() {
        return Err(TaskError::NotInRepositoryRoot);
    }
    let shell = Shell::new()?;
    TAPES.iter().try_for_each(|tape| {
        let tape_path = recordings.join(tape);
        cmd!(shell, "vhs {tape_path}").run()
    })?;
    let copied_png = recordings.join("passphrase.png");
    fs::copy(passphrase_png, &copied_png)
        .and_then(|_| fs::set_permissions(&copied_png, Permissions::from_mode(0o644)))
        .map_err(|source| TaskError::File {
            path: copied_png,
            source,
        })
}
