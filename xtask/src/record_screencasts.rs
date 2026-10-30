use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};

use xshell::{Shell, cmd};

use crate::task_error::TaskError;

const TAPES: [&str; 2] = [
    "scripted-demos/tapes/scripted-boot.tape",
    "scripted-demos/tapes/scripted-shutdown.tape",
];
const RECORDING_FRAME_MILLISECONDS: &str = "500";
pub const RECORDINGS_DIRECTORY: &str = "recordings";

fn sorted_entries(directory: &Path) -> Result<Vec<PathBuf>, TaskError> {
    let read_failure = |source| TaskError::File {
        path: directory.to_path_buf(),
        source,
    };
    let mut entries = fs::read_dir(directory)
        .map_err(read_failure)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(read_failure)?;
    entries.sort();
    Ok(entries)
}

#[derive(PartialEq, Eq)]
struct FrameSize {
    width: u32,
    height: u32,
}

fn png_frame_size(frame: &Path) -> Result<FrameSize, TaskError> {
    let mut header = [0_u8; 24];
    File::open(frame)
        .and_then(|mut file| file.read_exact(&mut header))
        .map_err(|source| TaskError::File {
            path: frame.to_path_buf(),
            source,
        })?;
    let big_endian = |offset: usize| {
        u32::from_be_bytes([
            header[offset],
            header[offset + 1],
            header[offset + 2],
            header[offset + 3],
        ])
    };
    Ok(FrameSize {
        width: big_endian(16),
        height: big_endian(20),
    })
}

fn frames_at_final_size(frames: Vec<PathBuf>) -> Result<Vec<PathBuf>, TaskError> {
    let Some(final_frame) = frames.last() else {
        return Ok(frames);
    };
    let final_size = png_frame_size(final_frame)?;
    let sizes = frames
        .iter()
        .map(|frame| png_frame_size(frame))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(frames
        .into_iter()
        .zip(sizes)
        .filter(|(_, size)| *size == final_size)
        .map(|(frame, _)| frame)
        .collect())
}

fn encode_machine_recording(
    shell: &Shell,
    check_output: &Path,
    recordings: &Path,
) -> Result<(), TaskError> {
    let Some(machine_name) = check_output.file_name() else {
        return Ok(());
    };
    let frames = frames_at_final_size(
        sorted_entries(&check_output.join("recording"))?
            .into_iter()
            .filter(|frame| {
                frame
                    .extension()
                    .is_some_and(|extension| extension == "png")
            })
            .collect(),
    )?;
    if frames.is_empty() {
        return Err(TaskError::NoRecordingFrames(check_output.to_path_buf()));
    }
    let animation = recordings.join(machine_name).with_extension("webp");
    let frame_duration = RECORDING_FRAME_MILLISECONDS;
    cmd!(
        shell,
        "img2webp -loop 0 -min_size -mixed -d {frame_duration} {frames...} -o {animation}"
    )
    .quiet()
    .run()?;
    Ok(())
}

pub fn record_screencasts(vm_check_outputs: &Path) -> Result<(), TaskError> {
    if !TAPES.iter().all(|tape| Path::new(tape).is_file()) {
        return Err(TaskError::NotInRepositoryRoot);
    }
    let recordings = Path::new(RECORDINGS_DIRECTORY);
    fs::create_dir_all(recordings).map_err(|source| TaskError::File {
        path: recordings.to_path_buf(),
        source,
    })?;
    let shell = Shell::new()?;
    TAPES.iter().try_for_each(|tape| {
        let tape_path = Path::new(tape);
        cmd!(shell, "vhs {tape_path}").run()
    })?;
    sorted_entries(vm_check_outputs)?
        .iter()
        .try_for_each(|check_output| encode_machine_recording(&shell, check_output, recordings))
}
