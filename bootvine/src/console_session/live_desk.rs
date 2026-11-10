use std::{future::ready, io, mem, ops::ControlFlow, path::PathBuf, time::Duration};

use crossterm::event::Event;
use futures::{Stream, StreamExt};

use crate::{
    boot_model::BootChange,
    boot_view::PasswordPrompt,
    console_session::{
        ask_password::{self, PasswordEntry, RequestWatch},
        console_palette::ConsoleLook,
        console_takeover::{
            ForegroundVt, QuietKernelConsole, active_vt, console_look, vt_of_stdin,
        },
        passphrase_input::{asks_to_quit, presses_enter},
        resume_console::{RESUME_CONSOLE_MARKER, resume_console_is_open},
        stream_sources::items_when_readable,
        watch_screen::{ConsoleDesk, Refresh, ScreenInput, WatchEnding},
    },
};

const HOLD_HINT: &str = "Press Enter to continue";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Handover {
    NotYet,
    At(Duration),
    OnKey,
}

pub struct LiveDesk {
    quiet_kernel: Option<QuietKernelConsole>,
    foreground: Option<ForegroundVt>,
    for_system: bool,
    own_vt: Option<u16>,
    look: ConsoleLook,
    entry: Option<PasswordEntry>,
    handover: Handover,
    handover_vt: Option<u16>,
    resume_marker: PathBuf,
    console_was_lent: bool,
}

impl LiveDesk {
    #[must_use]
    pub fn take_console(for_system: bool) -> Self {
        let prompt_elsewhere = for_system && ask_password::requested();
        if prompt_elsewhere {
            eprintln!("bootvine: a password prompt is already showing, staying behind it");
        }
        let own_vt = vt_of_stdin();
        Self {
            quiet_kernel: for_system.then(QuietKernelConsole::take).flatten(),
            foreground: own_vt
                .filter(|_| !prompt_elsewhere)
                .and_then(ForegroundVt::take),
            for_system,
            own_vt,
            look: console_look(own_vt),
            entry: None,
            handover: Handover::NotYet,
            handover_vt: None,
            resume_marker: PathBuf::from(RESUME_CONSOLE_MARKER),
            console_was_lent: false,
        }
    }

    fn hold_on_key(&self) -> bool {
        self.handover == Handover::OnKey
    }

    fn keeps_the_console(&self) -> bool {
        matches!(self.handover, Handover::NotYet | Handover::OnKey)
    }

    fn remember_handover_vt(&mut self) {
        if self.handover_vt.is_some() || !self.hold_on_key() {
            return;
        }
        self.handover_vt = active_vt().filter(|vt| Some(*vt) != self.own_vt);
    }

    fn come_forward_once_prompts_are_gone(&mut self) -> bool {
        if !self.for_system || self.foreground.is_some() || ask_password::requested() {
            return false;
        }
        self.foreground = self.own_vt.and_then(ForegroundVt::take);
        if self.foreground.is_some() {
            eprintln!("bootvine: password prompt answered, taking the console back");
        }
        self.foreground.is_some()
    }

    fn follow_requests(&mut self, requests_changed: bool) -> Refresh {
        let came_forward = requests_changed && self.come_forward_once_prompts_are_gone();
        self.entry = if self.for_system && self.foreground.is_some() {
            PasswordEntry::follow(self.entry.take(), requests_changed)
        } else {
            None
        };
        if came_forward {
            Refresh::Clear
        } else {
            Refresh::Steady
        }
    }
}

impl ConsoleDesk for LiveDesk {
    fn look(&self) -> ConsoleLook {
        self.look
    }

    fn prompt(&self, _now: Duration) -> Option<PasswordPrompt<'_>> {
        self.entry.as_ref().map(PasswordEntry::prompt)
    }

    fn press_key(&mut self, event: &Event, _now: Duration) -> ControlFlow<WatchEnding> {
        if self.hold_on_key() && (presses_enter(event) || asks_to_quit(event, false)) {
            eprintln!("bootvine: key pressed, handing the console over");
            if let (Some(foreground), Some(vt)) = (self.foreground.as_mut(), self.handover_vt) {
                foreground.return_to(vt);
            }
            return ControlFlow::Break(WatchEnding::BootCompleted);
        }
        let Some(entry) = &mut self.entry else {
            return if asks_to_quit(event, false) {
                ControlFlow::Break(WatchEnding::ViewerLeft)
            } else {
                ControlFlow::Continue(())
            };
        };
        if let Err(error) = entry.type_key(event) {
            eprintln!("bootvine: cannot answer the password prompt: {error}");
        }
        ControlFlow::Continue(())
    }

    fn hold_hint(&self) -> Option<&'static str> {
        self.hold_on_key().then_some(HOLD_HINT)
    }

    fn begin_handover(&mut self, reached: &str, deadline: Option<Duration>) {
        if self.handover != Handover::NotYet {
            return;
        }
        match deadline {
            Some(_) => eprintln!("bootvine: {reached} reached, handing the console over"),
            None => {
                eprintln!("bootvine: {reached} reached, holding the tree until a key is pressed");
            }
        }
        self.handover = deadline.map_or(Handover::OnKey, Handover::At);
    }

    fn frame_passed(&mut self, now: Duration) -> ControlFlow<WatchEnding, Vec<BootChange>> {
        let Handover::At(deadline) = self.handover else {
            return ControlFlow::Continue(Vec::new());
        };
        if self
            .foreground
            .as_ref()
            .is_some_and(ForegroundVt::taken_away)
        {
            eprintln!("bootvine: another program took the console, leaving it there");
            if let Some(foreground) = self.foreground.take() {
                foreground.stay_switched();
            }
            ControlFlow::Break(WatchEnding::BootCompleted)
        } else if now >= deadline {
            eprintln!("bootvine: nothing took the console, switching back");
            ControlFlow::Break(WatchEnding::BootCompleted)
        } else {
            ControlFlow::Continue(Vec::new())
        }
    }

    fn password_requests_changed(&mut self) -> Refresh {
        self.follow_requests(true)
    }

    fn check_prompts(&mut self) {
        self.follow_requests(false);
    }

    fn lends_console(&mut self) -> bool {
        let lent = resume_console_is_open(&self.resume_marker);
        self.console_was_lent |= lent;
        lent
    }

    fn keep_console_up(&mut self) -> Refresh {
        if self.lends_console() {
            return Refresh::Steady;
        }
        let console_returned = mem::take(&mut self.console_was_lent);
        let kernel_got_loud = self
            .quiet_kernel
            .as_mut()
            .is_some_and(QuietKernelConsole::keep_quiet);
        if self.keeps_the_console()
            && self
                .foreground
                .as_ref()
                .is_some_and(ForegroundVt::taken_away)
        {
            self.remember_handover_vt();
        }
        let terminal_was_taken =
            self.keeps_the_console() && self.foreground.as_ref().is_some_and(ForegroundVt::insist);
        let look = console_look(self.own_vt);
        let restyled = look != self.look;
        self.look = look;
        if console_returned || kernel_got_loud || terminal_was_taken || restyled {
            Refresh::Repaint
        } else {
            Refresh::Steady
        }
    }
}

pub fn password_request_inputs(watch: RequestWatch) -> io::Result<impl Stream<Item = ScreenInput>> {
    Ok(items_when_readable(watch, RequestWatch::changed)?
        .filter(|changed| ready(*changed))
        .map(|_| ScreenInput::PasswordRequests))
}

#[cfg(test)]
mod tests {
    use std::{env, fs, process};

    use crossterm::event::{KeyCode, KeyEvent};

    use super::*;

    const HANDOFF: &str = "display-manager.service";

    fn desk_watching(resume_marker: PathBuf) -> LiveDesk {
        LiveDesk {
            quiet_kernel: None,
            foreground: None,
            for_system: true,
            own_vt: None,
            look: console_look(None),
            entry: None,
            handover: Handover::NotYet,
            handover_vt: None,
            resume_marker,
            console_was_lent: false,
        }
    }

    fn marker_path() -> PathBuf {
        env::temp_dir().join(format!("bootvine-hold-marker-{}", process::id()))
    }

    #[test]
    fn the_console_is_repainted_only_after_it_was_lent_for_resume() {
        let marker = env::temp_dir().join(format!("bootvine-resume-marker-{}", process::id()));
        let mut desk = desk_watching(marker.clone());
        assert!(!desk.lends_console());

        fs::write(&marker, "1").expect("create the marker");
        assert!(desk.lends_console());
        assert_eq!(desk.keep_console_up(), Refresh::Steady);

        fs::remove_file(&marker).expect("remove the marker");
        assert!(!desk.lends_console());
        assert_eq!(desk.keep_console_up(), Refresh::Repaint);
        assert_eq!(desk.keep_console_up(), Refresh::Steady);
    }

    #[test]
    fn a_console_held_for_a_key_outlives_every_deadline() {
        let mut desk = desk_watching(marker_path());
        desk.begin_handover(HANDOFF, None);
        assert_eq!(desk.hold_hint(), Some(HOLD_HINT));
        assert!(matches!(
            desk.frame_passed(Duration::from_secs(3600)),
            ControlFlow::Continue(_)
        ));
    }

    #[test]
    fn enter_or_escape_releases_a_held_console() {
        let mut desk = desk_watching(marker_path());
        desk.begin_handover(HANDOFF, None);
        assert!(matches!(
            desk.press_key(
                &Event::Key(KeyEvent::from(KeyCode::Char('a'))),
                Duration::ZERO
            ),
            ControlFlow::Continue(())
        ));
        assert!(matches!(
            desk.press_key(&Event::Key(KeyEvent::from(KeyCode::Enter)), Duration::ZERO),
            ControlFlow::Break(WatchEnding::BootCompleted)
        ));
    }

    #[test]
    fn a_console_handed_over_on_a_deadline_shows_no_hint_and_ignores_enter() {
        let mut desk = desk_watching(marker_path());
        desk.begin_handover(HANDOFF, Some(Duration::from_secs(1)));
        assert_eq!(desk.hold_hint(), None);
        assert!(matches!(
            desk.press_key(&Event::Key(KeyEvent::from(KeyCode::Enter)), Duration::ZERO),
            ControlFlow::Continue(())
        ));
        assert!(matches!(
            desk.frame_passed(Duration::from_secs(2)),
            ControlFlow::Break(WatchEnding::BootCompleted)
        ));
    }
}
