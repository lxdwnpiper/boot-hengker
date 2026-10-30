use std::{io, path::PathBuf};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum TaskError {
    #[error(transparent)]
    Shell(#[from] xshell::Error),
    #[error("{}: {source}", path.display())]
    File { path: PathBuf, source: io::Error },
    #[error("{} has no recording frames", .0.display())]
    NoRecordingFrames(PathBuf),
    #[error("no VHS tapes here; run from the repository root")]
    NotInRepositoryRoot,
    #[error("no recordings/ directory here; run record-screencasts from the repository root first")]
    RecordingsMissing,
    #[error("{0} has no ```toml block under its Defaults heading")]
    DefaultsBlockMissing(&'static str),
}
