use std::{io, path::PathBuf, process::ExitStatus};

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("{}: {source}", path.display())]
    File { path: PathBuf, source: io::Error },
    #[error("could not run {program}: {source}")]
    Spawn { program: String, source: io::Error },
    #[error("{program} failed: {status}")]
    Failed { program: String, status: ExitStatus },
}
