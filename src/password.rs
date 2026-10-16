use std::{
    fs, io,
    os::unix::net::UnixDatagram,
    path::{Path, PathBuf},
    time::Duration,
};

use bootvine::{passphrase, view::PasswordPrompt};
use crossterm::event::Event;
use rustix::process::{Pid, test_kill_process};
use zeroize::Zeroizing;

use crate::clock::monotonic_now;

const REQUESTS: &str = "/run/systemd/ask-password";
const REQUEST_PREFIX: &str = "ask.";
const ASK_SECTION: &str = "[Ask]";

#[derive(Debug, PartialEq, Eq)]
pub struct PasswordRequest {
    pub file: PathBuf,
    pub message: String,
    socket: PathBuf,
    echo: bool,
    silent: bool,
    not_after: Option<Duration>,
    asker: Option<Pid>,
}

pub fn requested() -> bool {
    !request_files().is_empty()
}

fn request_files() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(REQUESTS)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(REQUEST_PREFIX)
        })
        .map(|entry| entry.path())
        .collect();
    files.sort();
    files
}

impl PasswordRequest {
    fn first_live() -> Option<Self> {
        request_files()
            .iter()
            .filter_map(|file| Self::read(file))
            .find(Self::is_live)
    }

    fn read(file: &Path) -> Option<Self> {
        Self::parse(file, &fs::read_to_string(file).ok()?)
    }

    fn parse(file: &Path, text: &str) -> Option<Self> {
        let fields: Vec<(&str, &str)> = text
            .lines()
            .map(str::trim)
            .skip_while(|line| *line != ASK_SECTION)
            .skip(1)
            .take_while(|line| !line.starts_with('['))
            .filter_map(|line| line.split_once('='))
            .collect();
        let field = |key: &str| {
            fields
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| *value)
        };
        let flag = |key: &str| field(key).is_some_and(is_true);
        Some(Self {
            file: file.to_owned(),
            message: field("Message").unwrap_or_default().to_owned(),
            socket: PathBuf::from(field("Socket")?),
            echo: flag("Echo"),
            silent: flag("Silent"),
            not_after: field("NotAfter")
                .and_then(|value| value.parse().ok())
                .filter(|micros| *micros > 0)
                .map(Duration::from_micros),
            asker: field("PID")
                .and_then(|value| value.parse().ok())
                .and_then(Pid::from_raw),
        })
    }

    fn is_live(&self) -> bool {
        self.file.exists()
            && self
                .not_after
                .is_none_or(|deadline| monotonic_now() <= deadline)
            && self
                .asker
                .is_none_or(|asker| test_kill_process(asker).is_ok())
    }

    fn answer(&self, password: &str) -> io::Result<()> {
        let mut packet = Zeroizing::new(Vec::with_capacity(password.len() + 1));
        packet.push(b'+');
        packet.extend_from_slice(password.as_bytes());
        UnixDatagram::unbound()?.send_to(&packet, &self.socket)?;
        Ok(())
    }
}

fn is_true(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "1" | "yes" | "y" | "true" | "t" | "on"
    )
}

pub struct PasswordEntry {
    request: PasswordRequest,
    typed: Zeroizing<String>,
    sent: bool,
}

impl PasswordEntry {
    pub fn follow(current: Option<Self>) -> Option<Self> {
        if let Some(entry) = current.filter(|entry| entry.request.is_live()) {
            return Some(entry);
        }
        let request = PasswordRequest::first_live()?;
        eprintln!(
            "bootvine: asking for a password: {}",
            request.file.display()
        );
        Some(Self {
            request,
            typed: passphrase::empty(),
            sent: false,
        })
    }

    pub fn prompt(&self) -> PasswordPrompt<'_> {
        let input = if self.request.silent {
            String::new()
        } else if self.request.echo {
            self.typed.to_string()
        } else {
            passphrase::masked(&self.typed)
        };
        PasswordPrompt {
            message: &self.request.message,
            input,
            sent: self.sent,
        }
    }

    pub fn type_key(&mut self, event: &Event) -> io::Result<()> {
        if self.sent {
            return Ok(());
        }
        let Some(submitted) = passphrase::type_key(&mut self.typed, event) else {
            return Ok(());
        };
        self.request.answer(&submitted)?;
        self.sent = true;
        eprintln!("bootvine: answered a password prompt");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent};

    use super::*;

    const CRYPTSETUP_ASK: &str = concat!(
        "[Ask]\n",
        "PID=1\n",
        "Socket=/run/systemd/ask-password/sck.1\n",
        "AcceptCached=1\n",
        "Echo=0\n",
        "NotAfter=0\n",
        "Message=Please enter passphrase for disk secret:\n",
        "Icon=drive-harddisk\n",
        "Id=cryptsetup:/dev/vdb\n",
    );

    #[test]
    fn reads_what_cryptsetup_asks() {
        let request = PasswordRequest::parse(Path::new("ask.x"), CRYPTSETUP_ASK).expect("parsed");
        assert_eq!(
            request,
            PasswordRequest {
                file: PathBuf::from("ask.x"),
                message: "Please enter passphrase for disk secret:".to_owned(),
                socket: PathBuf::from("/run/systemd/ask-password/sck.1"),
                echo: false,
                silent: false,
                not_after: None,
                asker: Pid::from_raw(1),
            }
        );
    }

    #[test]
    fn ignores_requests_without_a_socket() {
        assert_eq!(
            PasswordRequest::parse(Path::new("ask.x"), "[Ask]\nMessage=hi\n"),
            None
        );
    }

    #[test]
    fn hides_what_is_typed() {
        let mut entry = PasswordEntry {
            request: PasswordRequest::parse(Path::new("ask.x"), CRYPTSETUP_ASK).expect("parsed"),
            typed: passphrase::empty(),
            sent: false,
        };
        "secret".chars().for_each(|character| {
            entry
                .type_key(&Event::Key(KeyEvent::from(KeyCode::Char(character))))
                .expect("typed");
        });
        entry
            .type_key(&Event::Key(KeyEvent::from(KeyCode::Backspace)))
            .expect("erased");
        assert_eq!(entry.prompt().input, "*****");
    }
}
