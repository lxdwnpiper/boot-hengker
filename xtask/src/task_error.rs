use std::{io, path::PathBuf};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum TaskError {
    #[error(transparent)]
    Shell(#[from] xshell::Error),
    #[error("{}: {source}", path.display())]
    File { path: PathBuf, source: io::Error },
    #[error("no recordings directory here; run from the repository root")]
    NotInRepositoryRoot,
}
