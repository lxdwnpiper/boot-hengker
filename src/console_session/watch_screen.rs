use std::{future::ready, ops::ControlFlow, time::Duration};

use crossterm::event::Event;
use futures::{Stream, StreamExt};
use ratatui::{
    Terminal,
    backend::Backend,
    buffer::{Buffer, CellDiffOption},
    style::Modifier,
};

use crate::{
    boot_model::{BootChange, BootProjection, BootState, boot_origin::BootOrigin},
    boot_view::{BootView, PasswordPrompt, project_boot},
    screen_settings::{color_scheme::Colors, config_file::Timing, font_glyphs::Glyphs},
    systemd_link::{link_error::Error, monotonic_clock::monotonic_now},
};

const INPUT_BATCH: usize = 64;
const PROJECTION_REFRESH: Duration = Duration::from_millis(100);

pub enum ScreenInput {
    Boot(Vec<BootChange>),
    Key(Event),
    Frame,
    PromptCheck,
    ConsoleUpkeep,
    FullRepaint,
    PasswordRequests,
    Terminate,
    Failed(Error),
}

pub enum WatchEnding {
    BootCompleted,
    ViewerLeft,
    Failed(Error),
}

pub trait ConsoleDesk {
    fn glyphs(&self) -> &Glyphs;
    fn drops_bold(&self) -> bool;
    fn prompt(&self, now: Duration) -> Option<PasswordPrompt<'_>>;
    fn press_key(&mut self, event: &Event, now: Duration) -> ControlFlow<WatchEnding>;
    fn frame_passed(&mut self, state: &mut BootState, now: Duration) -> ControlFlow<WatchEnding>;

    fn password_requests_changed(&mut self) {}

    fn check_prompts(&mut self) {}

    fn keep_console_up(&mut self) -> bool {
        false
    }

    fn take_clear_request(&mut self) -> bool {
        false
    }

    fn refreshed_origin(&self) -> Option<BootOrigin> {
        None
    }

    fn begin_handover(&mut self, _reached: &str, _deadline: Duration) {}
}

struct CachedProjection {
    projection: BootProjection,
    built_at: Duration,
}

pub struct WatchScreen<Desk> {
    pub state: BootState,
    pub desk: Desk,
    origin: BootOrigin,
    full_repaint_due: bool,
    colors: Colors,
    timing: Timing,
    epoch: Duration,
    projection: Option<CachedProjection>,
}

impl<Desk: ConsoleDesk> WatchScreen<Desk> {
    pub fn new(
        state: BootState,
        desk: Desk,
        origin: BootOrigin,
        colors: Colors,
        timing: Timing,
        epoch: Duration,
    ) -> Self {
        Self {
            state,
            desk,
            origin,
            full_repaint_due: false,
            colors,
            timing,
            epoch,
            projection: None,
        }
    }

    #[must_use]
    pub fn now(&self) -> Duration {
        monotonic_now().saturating_sub(self.epoch)
    }

    fn absorb(&mut self, input: ScreenInput) -> ControlFlow<WatchEnding> {
        let now = self.now();
        match input {
            ScreenInput::Boot(changes) => self.apply_boot_changes(changes, now),
            ScreenInput::Key(event) => self.desk.press_key(&event, now),
            ScreenInput::Frame => self.desk.frame_passed(&mut self.state, now),
            ScreenInput::PromptCheck => {
                self.desk.check_prompts();
                ControlFlow::Continue(())
            }
            ScreenInput::ConsoleUpkeep => {
                self.full_repaint_due |= self.desk.keep_console_up();
                ControlFlow::Continue(())
            }
            ScreenInput::FullRepaint => {
                self.full_repaint_due = true;
                ControlFlow::Continue(())
            }
            ScreenInput::PasswordRequests => {
                self.desk.password_requests_changed();
                ControlFlow::Continue(())
            }
            ScreenInput::Terminate => ControlFlow::Break(WatchEnding::ViewerLeft),
            ScreenInput::Failed(error) => ControlFlow::Break(WatchEnding::Failed(error)),
        }
    }

    fn apply_boot_changes(
        &mut self,
        changes: Vec<BootChange>,
        now: Duration,
    ) -> ControlFlow<WatchEnding> {
        let phase_before = self.state.phase;
        changes
            .into_iter()
            .for_each(|change| self.state.apply(change));
        self.projection = None;
        if self.state.phase != phase_before
            && let Some(origin) = self.desk.refreshed_origin()
        {
            self.origin = origin;
        }
        if self.state.display_manager_started() {
            let display_manager = self
                .state
                .handoff_unit
                .as_deref()
                .unwrap_or("display manager");
            self.desk
                .begin_handover(display_manager, now + self.timing.wait_for_display_manager);
        } else if self.state.is_complete() {
            self.desk.begin_handover(
                self.state.root.unit().unwrap_or("boot"),
                now + self.timing.keep_final_frame_for,
            );
        }
        ControlFlow::Continue(())
    }

    fn absorb_batch<B: Backend>(
        &mut self,
        batch: Vec<ScreenInput>,
        terminal: &mut Terminal<B>,
    ) -> ControlFlow<WatchEnding>
    where
        Error: From<B::Error>,
    {
        batch.into_iter().try_for_each(|input| self.absorb(input))?;
        match self.draw(terminal) {
            Ok(()) => ControlFlow::Continue(()),
            Err(error) => ControlFlow::Break(WatchEnding::Failed(error)),
        }
    }

    fn draw<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<(), Error>
    where
        Error: From<B::Error>,
    {
        if self.desk.take_clear_request() {
            terminal.clear()?;
        }
        let now = self.now();
        if self
            .projection
            .as_ref()
            .is_some_and(|cached| now.saturating_sub(cached.built_at) >= PROJECTION_REFRESH)
        {
            self.projection = None;
        }
        let projection = &self
            .projection
            .get_or_insert_with(|| CachedProjection {
                projection: project_boot(&self.state, &self.timing, now),
                built_at: now,
            })
            .projection;
        let completed = terminal.draw(|canvas| {
            canvas.render_widget(
                BootView {
                    state: &self.state,
                    projection,
                    colors: &self.colors,
                    timing: &self.timing,
                    glyphs: self.desk.glyphs(),
                    now,
                    origin: &self.origin,
                    password_prompt: self.desk.prompt(now),
                },
                canvas.area(),
            );
            if self.desk.drops_bold() {
                drop_bold(canvas.buffer_mut());
            }
        })?;
        if self.full_repaint_due {
            let painted = completed.buffer.clone();
            overwrite_every_cell(terminal.backend_mut(), &painted)?;
            self.full_repaint_due = false;
        }
        Ok(())
    }
}

pub async fn watch_until_ending<Desk: ConsoleDesk, B: Backend>(
    screen: &mut WatchScreen<Desk>,
    terminal: &mut Terminal<B>,
    inputs: impl Stream<Item = ScreenInput>,
) -> WatchEnding
where
    Error: From<B::Error>,
{
    inputs
        .ready_chunks(INPUT_BATCH)
        .filter_map(|batch| ready(screen.absorb_batch(batch, terminal).break_value()))
        .take(1)
        .fold(WatchEnding::ViewerLeft, |_, ending| ready(ending))
        .await
}

fn overwrite_every_cell<B: Backend>(backend: &mut B, frame: &Buffer) -> Result<(), B::Error> {
    backend.draw(
        frame
            .content
            .iter()
            .enumerate()
            .filter(|(_, cell)| cell.diff_option != CellDiffOption::Skip)
            .map(|(index, cell)| {
                let (x, y) = frame.pos_of(index);
                (x, y, cell)
            }),
    )?;
    backend.flush()
}

fn drop_bold(frame: &mut Buffer) {
    frame
        .content
        .iter_mut()
        .for_each(|cell| cell.modifier.remove(Modifier::BOLD));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        boot_model::{ActiveState, Activity, HANDOFF_UNIT},
        demo_script::boot_test_fixtures::{MS, nested_boot_sample, queue_unit, waiting},
    };

    #[derive(Default)]
    struct HandoverRecorder {
        glyphs: Glyphs,
        handovers: Vec<String>,
    }

    impl ConsoleDesk for HandoverRecorder {
        fn glyphs(&self) -> &Glyphs {
            &self.glyphs
        }

        fn drops_bold(&self) -> bool {
            false
        }

        fn prompt(&self, _now: Duration) -> Option<PasswordPrompt<'_>> {
            None
        }

        fn press_key(&mut self, _event: &Event, _now: Duration) -> ControlFlow<WatchEnding> {
            ControlFlow::Continue(())
        }

        fn frame_passed(
            &mut self,
            _state: &mut BootState,
            _now: Duration,
        ) -> ControlFlow<WatchEnding> {
            ControlFlow::Continue(())
        }

        fn begin_handover(&mut self, reached: &str, _deadline: Duration) {
            self.handovers.push(reached.to_owned());
        }
    }

    fn screen_watching(state: BootState) -> WatchScreen<HandoverRecorder> {
        let origin = BootOrigin {
            host: "test".to_owned(),
            system: None,
            generation: None,
            kernel: "Linux".to_owned(),
        };
        WatchScreen::new(
            state,
            HandoverRecorder::default(),
            origin,
            Colors::default(),
            Timing::default(),
            Duration::ZERO,
        )
    }

    #[test]
    fn display_manager_start_hands_over_before_the_boot_completes() {
        let mut state = nested_boot_sample();
        queue_unit(&mut state, 20, waiting(HANDOFF_UNIT, &[]), 500 * MS);
        let mut screen = screen_watching(state);
        let _ = screen.apply_boot_changes(Vec::new(), 1000 * MS);
        assert!(screen.desk.handovers.is_empty());
        let started = BootChange::UpdateActivity {
            unit: HANDOFF_UNIT.to_owned(),
            activity: Activity {
                state: Some(ActiveState::Active),
                ..Activity::default()
            },
        };
        let _ = screen.apply_boot_changes(vec![started], 6000 * MS);
        assert!(!screen.state.is_complete());
        assert_eq!(screen.desk.handovers, [HANDOFF_UNIT]);
    }
}
