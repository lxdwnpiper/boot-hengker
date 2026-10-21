use std::{future::ready, io, mem, ops::ControlFlow, time::Duration};

use crossterm::event::Event;
use futures::{Stream, StreamExt};

use crate::{
    boot_model::{BootState, boot_origin::BootOrigin},
    boot_view::PasswordPrompt,
    console_session::{
        ask_password::{PasswordEntry, RequestWatch},
        console_takeover::Console,
        passphrase_input::asks_to_quit,
        stream_sources::{CarriedItem, periodic_inputs, readable_source, unfold_carrying},
        watch_screen::{ConsoleDesk, ScreenInput, WatchEnding},
    },
    screen_settings::font_glyphs::Glyphs,
};

const CONSOLE_UPKEEP_INTERVAL: Duration = Duration::from_secs(1);
const FULL_REPAINT_INTERVAL: Duration = Duration::from_secs(5);

pub struct LiveDesk {
    console: Console,
    entry: Option<PasswordEntry>,
    requests_watched: bool,
    clear_requested: bool,
}

impl LiveDesk {
    #[must_use]
    pub fn new(console: Console, requests_watched: bool) -> Self {
        Self {
            console,
            entry: None,
            requests_watched,
            clear_requested: false,
        }
    }

    fn follow_requests(&mut self, requests_changed: bool) {
        self.clear_requested |=
            requests_changed && self.console.come_forward_once_prompts_are_gone();
        self.entry = if self.console.answers_password_prompts() {
            PasswordEntry::follow(self.entry.take(), requests_changed)
        } else {
            None
        };
    }
}

impl ConsoleDesk for LiveDesk {
    fn glyphs(&self) -> &Glyphs {
        &self.console.glyphs
    }

    fn drops_bold(&self) -> bool {
        self.console.bold_is_brightness
    }

    fn prompt(&self, _now: Duration) -> Option<PasswordPrompt<'_>> {
        self.entry.as_ref().map(PasswordEntry::prompt)
    }

    fn press_key(&mut self, event: &Event, _now: Duration) -> ControlFlow<WatchEnding> {
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

    fn frame_passed(&mut self, _state: &mut BootState, _now: Duration) -> ControlFlow<WatchEnding> {
        self.follow_requests(!self.requests_watched);
        ControlFlow::Continue(())
    }

    fn password_requests_changed(&mut self) {
        self.follow_requests(true);
    }

    fn keep_console_up(&mut self) -> bool {
        self.console.keep_up()
    }

    fn take_clear_request(&mut self) -> bool {
        mem::take(&mut self.clear_requested)
    }

    fn refreshed_origin(&self) -> Option<BootOrigin> {
        Some(BootOrigin::read())
    }
}

pub fn console_upkeep_inputs() -> impl Stream<Item = ScreenInput> {
    periodic_inputs(CONSOLE_UPKEEP_INTERVAL, || ScreenInput::ConsoleUpkeep)
}

pub fn full_repaint_inputs() -> impl Stream<Item = ScreenInput> {
    periodic_inputs(FULL_REPAINT_INTERVAL, || ScreenInput::FullRepaint)
}

pub fn password_request_inputs(watch: RequestWatch) -> io::Result<impl Stream<Item = ScreenInput>> {
    let readiness = readable_source(watch)?;
    Ok(unfold_carrying(readiness, |readiness| async move {
        readiness.readable().await.ok()?.clear_ready();
        Some(CarriedItem {
            item: readiness.get_ref().changed(),
            state: readiness,
        })
    })
    .filter(|changed| ready(*changed))
    .map(|_| ScreenInput::PasswordRequests))
}
