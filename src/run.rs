use std::{
    io,
    os::fd::{AsFd, BorrowedFd},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::sleep,
    time::{Duration, Instant},
};

use bootvine::{
    config::Timing,
    model::{BootState, Phase},
    origin::BootOrigin,
    palette::Palette,
    view::BootView,
};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    DefaultTerminal,
    backend::Backend,
    buffer::{Buffer, CellDiffOption},
    style::Modifier,
};
use rustix::{
    event::{PollFd, PollFlags, poll},
    io::Errno,
    stdio::stdin,
    time::Timespec,
};
use signal_hook::{consts::SIGTERM, flag};

use crate::{
    clock::monotonic_now,
    console::Console,
    error::Error,
    feed::{Feed, RootRequest},
    password::{PasswordEntry, RequestWatch},
    sysroot::INITRD_RELEASE,
    systemd::{Scope, Session},
};

const FRAME_INTERVAL: Duration = Duration::from_millis(100);
const FULL_REPAINT_INTERVAL: Duration = Duration::from_secs(1);

pub struct Settings {
    pub scope: Scope,
    pub shutdown: bool,
    pub target: RootRequest,
    pub timing: Timing,
    pub palette: Palette,
}

pub fn run(settings: &Settings) -> Result<(), Error> {
    let terminate = Arc::new(AtomicBool::new(false));
    flag::register(SIGTERM, Arc::clone(&terminate))?;
    if settings.scope == Scope::System
        && !settings.shutdown
        && Session::open(Scope::System)
            .and_then(|session| session.boot_in_progress())
            .is_ok_and(|booting| !booting)
    {
        eprintln!("bootvine: boot already finished, leaving the console alone");
        return Ok(());
    }
    let phase = match settings.scope {
        _ if settings.shutdown => Phase::Shutdown,
        Scope::User => Phase::User,
        Scope::System if Path::new(INITRD_RELEASE).exists() => Phase::Initrd,
        Scope::System => Phase::Host,
    };
    let mut state = BootState::new(phase);
    let mut feed = Feed::new(settings.scope, settings.target.clone());
    let mut console = Console::take(settings.scope == Scope::System);
    let mut terminal = ratatui::try_init()?;
    let watched = watch(
        &mut state,
        &mut feed,
        &mut terminal,
        &terminate,
        settings,
        &mut console,
    );
    ratatui::restore();
    watched
}

fn sleep_until_readable<'fd>(
    sources: impl IntoIterator<Item = BorrowedFd<'fd>>,
    timeout: Duration,
) -> io::Result<()> {
    let mut watched: Vec<PollFd<'fd>> = sources
        .into_iter()
        .map(|source| PollFd::from_borrowed_fd(source, PollFlags::IN))
        .collect();
    match poll(
        &mut watched,
        Some(&Timespec::try_from(timeout).unwrap_or_default()),
    ) {
        Ok(_) | Err(Errno::INTR) => Ok(()),
        Err(error) => Err(error.into()),
    }
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

fn watch(
    state: &mut BootState,
    feed: &mut Feed,
    terminal: &mut DefaultTerminal,
    terminate: &AtomicBool,
    settings: &Settings,
    console: &mut Console,
) -> Result<(), Error> {
    let mut origin = BootOrigin::read();
    let mut origin_phase = state.phase;
    let mut frame: u64 = 0;
    let mut painted_connections = feed.connections;
    let mut last_full_repaint = Instant::now();
    let mut next_frame = Instant::now();
    let requests = RequestWatch::open();
    let mut password_entry: Option<PasswordEntry> = None;
    loop {
        if state.phase != origin_phase {
            origin = BootOrigin::read();
            origin_phase = state.phase;
        }
        let requests_changed = requests.as_ref().is_none_or(RequestWatch::changed);
        if requests_changed && console.come_forward_once_prompts_are_gone() {
            terminal.clear()?;
        }
        password_entry = if console.answers_password_prompts() {
            PasswordEntry::follow(password_entry.take(), requests_changed)
        } else {
            None
        };
        let repaint_due = feed.connections != painted_connections
            || last_full_repaint.elapsed() >= FULL_REPAINT_INTERVAL;
        if repaint_due {
            console.keep_up();
        }
        let completed = terminal.draw(|canvas| {
            canvas.render_widget(
                BootView {
                    state,
                    palette: &settings.palette,
                    timing: &settings.timing,
                    glyphs: &console.glyphs,
                    now: monotonic_now(),
                    origin: &origin,
                    frame,
                    password_prompt: password_entry.as_ref().map(PasswordEntry::prompt),
                },
                canvas.area(),
            );
            if console.bold_is_brightness {
                drop_bold(canvas.buffer_mut());
            }
        })?;
        if repaint_due {
            let painted = completed.buffer.clone();
            overwrite_every_cell(terminal.backend_mut(), &painted)?;
            painted_connections = feed.connections;
            last_full_repaint = Instant::now();
        }
        if state.is_complete() {
            eprintln!(
                "bootvine: {} reached, handing the console over",
                state.root.unit().unwrap_or("boot")
            );
            sleep(settings.timing.keep_final_frame_for);
            return Ok(());
        }
        if terminate.load(Ordering::Relaxed) {
            return Ok(());
        }
        sleep_until_readable(
            [
                Some(stdin()),
                feed.session_fd(),
                requests.as_ref().map(AsFd::as_fd),
            ]
            .into_iter()
            .flatten(),
            next_frame.saturating_duration_since(Instant::now()),
        )?;
        if Instant::now() >= next_frame {
            frame += 1;
            next_frame = Instant::now() + FRAME_INTERVAL;
        }
        while event::poll(Duration::ZERO)? {
            let event = event::read()?;
            match &mut password_entry {
                Some(entry) => {
                    if let Err(error) = entry.type_key(&event) {
                        eprintln!("bootvine: cannot answer the password prompt: {error}");
                    }
                }
                None if asks_to_quit(&event) => return Ok(()),
                None => {}
            }
        }
        feed.pump(state)?;
    }
}
