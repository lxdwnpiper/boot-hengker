use std::{fs, path::Path};

use xshell::{Shell, cmd};

use crate::task_error::TaskError;

const CONFIGURATION_DOC: &str = "docs/configuration.md";
const DEFAULTS_HEADING: &str = "## Defaults\n";
const TOML_FENCE_OPEN: &str = "```toml\n";
const FENCE_CLOSE: &str = "```\n";

pub fn sync_default_config() -> Result<(), TaskError> {
    let doc_path = Path::new(CONFIGURATION_DOC);
    let doc_file_error = |source| TaskError::File {
        path: doc_path.to_path_buf(),
        source,
    };
    let doc_text = fs::read_to_string(doc_path).map_err(doc_file_error)?;
    let shell = Shell::new()?;
    let printed_defaults =
        cmd!(shell, "cargo run --quiet --package bootvine -- defaults").read()?;
    let synced_text = replace_defaults_block(&doc_text, &printed_defaults)
        .ok_or(TaskError::DefaultsBlockMissing(CONFIGURATION_DOC))?;
    if synced_text != doc_text {
        fs::write(doc_path, synced_text).map_err(doc_file_error)?;
    }
    Ok(())
}

fn replace_defaults_block(doc_text: &str, printed_defaults: &str) -> Option<String> {
    let section_start = doc_text.find(DEFAULTS_HEADING)?;
    let block_start =
        section_start + doc_text[section_start..].find(TOML_FENCE_OPEN)? + TOML_FENCE_OPEN.len();
    let block_end = block_start + doc_text[block_start..].find(FENCE_CLOSE)?;
    Some(format!(
        "{}{}\n{}",
        &doc_text[..block_start],
        printed_defaults.trim_end(),
        &doc_text[block_end..]
    ))
}
