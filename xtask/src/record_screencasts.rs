use std::{
    fs::{self, Permissions},
    os::unix::fs::PermissionsExt,
    path::Path,
};

use xshell::{Shell, cmd};

use crate::task_error::TaskError;

const TAPES: [&str; 2] = [
    "scripted-demos/tapes/scripted-boot.tape",
    "scripted-demos/tapes/scripted-shutdown.tape",
];

pub struct CheckScreenshots<'a> {
    pub passphrase_png: &'a Path,
    pub failures_png: &'a Path,
}

fn copy_screenshot(
    screenshot: &Path,
    recordings: &Path,
    published_name: &str,
) -> Result<(), TaskError> {
    let copied_png = recordings.join(published_name);
    fs::copy(screenshot, &copied_png)
        .and_then(|_| fs::set_permissions(&copied_png, Permissions::from_mode(0o644)))
        .map_err(|source| TaskError::File {
            path: copied_png,
            source,
        })
}

pub fn record_screencasts(screenshots: &CheckScreenshots) -> Result<(), TaskError> {
    let recordings = Path::new("recordings");
    if !recordings.is_dir() {
        return Err(TaskError::NotInRepositoryRoot);
    }
    let shell = Shell::new()?;
    TAPES.iter().try_for_each(|tape| {
        let tape_path = Path::new(tape);
        cmd!(shell, "vhs {tape_path}").run()
    })?;
    copy_screenshot(screenshots.passphrase_png, recordings, "passphrase.png")?;
    copy_screenshot(screenshots.failures_png, recordings, "failures.png")
}
