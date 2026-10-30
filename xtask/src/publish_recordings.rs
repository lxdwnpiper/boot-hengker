use std::path::Path;

use xshell::{Shell, cmd};

use crate::{record_screencasts::RECORDINGS_DIRECTORY, task_error::TaskError};

const RECORDINGS_BRANCH: &str = "refs/heads/recordings";

pub fn publish_recordings() -> Result<(), TaskError> {
    if !Path::new(RECORDINGS_DIRECTORY).is_dir() {
        return Err(TaskError::RecordingsMissing);
    }
    let shell = Shell::new()?;
    let git_directory = cmd!(shell, "git rev-parse --absolute-git-dir").read()?;
    let temporary_directory = shell.create_temp_dir()?;
    let _index_override = shell.push_env(
        "GIT_INDEX_FILE",
        temporary_directory.path().join("recordings-index"),
    );
    let _git_directory_override = shell.push_env("GIT_DIR", &git_directory);
    let _recordings_directory = shell.push_dir(RECORDINGS_DIRECTORY);
    cmd!(shell, "git --work-tree=. add --all --force .")
        .quiet()
        .run()?;
    let recordings_tree = cmd!(shell, "git write-tree").read()?;
    let recordings_commit = cmd!(
        shell,
        "git commit-tree {recordings_tree} -m 'chore: sample recordings [skip ci]'"
    )
    .read()?;
    let branch = RECORDINGS_BRANCH;
    cmd!(
        shell,
        "git push --force origin {recordings_commit}:{branch}"
    )
    .run()?;
    Ok(())
}
