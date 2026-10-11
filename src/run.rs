use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, channel},
    },
    thread::sleep,
    time::Duration,
};

use boot_tree::{
    model::{BootState, Phase},
    view::BootView,
};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    DefaultTerminal,
    backend::{Backend, ClearType},
};
use signal_hook::{consts::SIGTERM, flag};

use crate::{
    clock::{host_name, monotonic_now},
    console::ForegroundVt,
    error::Error,
    feed::{self, FeedOptions, JobKind, Update},
    sysroot::running_in_initrd,
    systemd::Scope,
};

const FRAME_INTERVAL: Duration = Duration::from_millis(100);
const FRAMES_PER_FULL_REPAINT: u64 = 20;

pub struct Options {
    pub scope: Scope,
    pub target: Option<String>,
    pub hold: Duration,
}

pub fn run(options: Options) -> Result<(), Error> {
    let terminate = Arc::new(AtomicBool::new(false));
    flag::register(SIGTERM, Arc::clone(&terminate))?;
    let (sender, updates) = channel();
    let mut state = BootState::new(initial_phase(options.scope));
    feed::spawn(
        FeedOptions {
            scope: options.scope,
            target: options.target,
        },
        sender,
    );
    let foreground = ForegroundVt::take();
    let mut terminal = ratatui::try_init()?;
    let watched = watch(
        &mut state,
        &updates,
        &mut terminal,
        &terminate,
        options.hold,
    );
    ratatui::restore();
    if let Some(foreground) = foreground {
        foreground.give_back();
    }
    watched
}

fn initial_phase(scope: Scope) -> Phase {
    match scope {
        Scope::User => Phase::User,
        Scope::System if running_in_initrd() => Phase::Initrd,
        Scope::System => Phase::Host,
    }
}

fn absorb(state: &mut BootState, update: Update) -> Option<Error> {
    match update {
        Update::Root(root) => state.set_root(root),
        Update::Handoff(unit) => state.set_handoff(unit),
        Update::Origin(origin) => state.set_origin(origin),
        Update::Unit { facts, activity } => state.track_unit(facts, activity),
        Update::JobQueued {
            id,
            unit,
            at,
            kind: JobKind::BringsUp,
        } => state.queue_job(id, &unit, at),
        Update::JobQueued {
            id,
            unit,
            at,
            kind: JobKind::Unknown,
        } => state.queue_job_of_unknown_kind(id, &unit, at),
        Update::JobFinished { id, result, at } => state.finish_job(id, result, at),
        Update::Activity { unit, activity } => state.update_activity(&unit, activity),
        Update::RootSwitched { at } => state.switch_root(at),
        Update::Resubscribed => state.forget_jobs(),
        Update::Ended(error) => return Some(error),
    }
    None
}

fn draw(
    terminal: &mut DefaultTerminal,
    state: &BootState,
    host: &str,
    frame: u64,
) -> Result<(), Error> {
    terminal.draw(|canvas| {
        canvas.render_widget(
            BootView {
                state,
                now: monotonic_now(),
                host,
                frame,
            },
            canvas.area(),
        );
    })?;
    Ok(())
}

fn repaint_fully(terminal: &mut DefaultTerminal) -> Result<(), Error> {
    terminal.backend_mut().clear_region(ClearType::All)?;
    terminal.swap_buffers();
    Ok(())
}

fn asks_to_quit(event: &Event) -> bool {
    let Event::Key(KeyEvent {
        code, modifiers, ..
    }) = event
    else {
        return false;
    };
    *code == KeyCode::Char('q')
        || (*code == KeyCode::Char('c') && modifiers.contains(KeyModifiers::CONTROL))
}

fn watch(
    state: &mut BootState,
    updates: &Receiver<Update>,
    terminal: &mut DefaultTerminal,
    terminate: &AtomicBool,
    hold: Duration,
) -> Result<(), Error> {
    let host = host_name();
    let mut frame: u64 = 0;
    loop {
        if let Some(error) = updates.try_iter().find_map(|update| absorb(state, update)) {
            return Err(error);
        }
        if frame.is_multiple_of(FRAMES_PER_FULL_REPAINT) {
            repaint_fully(terminal)?;
        }
        draw(terminal, state, &host, frame)?;
        if state.is_complete() {
            eprintln!(
                "boot-tree: {} reached, handing the console over",
                state.root().unwrap_or("boot")
            );
            sleep(hold);
            return Ok(());
        }
        if terminate.load(Ordering::Relaxed) {
            return Ok(());
        }
        if event::poll(FRAME_INTERVAL)? && asks_to_quit(&event::read()?) {
            return Ok(());
        }
        frame += 1;
    }
}
