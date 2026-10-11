use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::sleep,
    time::{Duration, Instant},
};

use bootvine::{
    model::{BootState, Phase},
    palette::Palette,
    view::BootView,
};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    DefaultTerminal,
    backend::Backend,
    buffer::{Buffer, CellDiffOption},
};
use rustix::system::uname;
use signal_hook::{consts::SIGTERM, flag};

use crate::{
    clock::monotonic_now,
    console::{Console, ForegroundVt},
    error::Error,
    feed::Feed,
    sysroot::INITRD_RELEASE,
    systemd::{Scope, Session},
};

const FRAME_INTERVAL: Duration = Duration::from_millis(100);
const FULL_REPAINT_INTERVAL: Duration = Duration::from_secs(1);

pub struct Options {
    pub scope: Scope,
    pub shutdown: bool,
    pub target: Option<String>,
    pub hold: Duration,
    pub palette: Palette,
}

pub fn run(options: &Options) -> Result<(), Error> {
    let terminate = Arc::new(AtomicBool::new(false));
    flag::register(SIGTERM, Arc::clone(&terminate))?;
    if options.scope == Scope::System
        && !options.shutdown
        && Session::open(Scope::System)
            .and_then(|session| session.boot_in_progress())
            .is_ok_and(|booting| !booting)
    {
        eprintln!("bootvine: boot already finished, leaving the console alone");
        return Ok(());
    }
    let phase = match options.scope {
        _ if options.shutdown => Phase::Shutdown,
        Scope::User => Phase::User,
        Scope::System if Path::new(INITRD_RELEASE).exists() => Phase::Initrd,
        Scope::System => Phase::Host,
    };
    let mut state = BootState::new(phase);
    let mut feed = Feed::new(options.scope, options.target.clone());
    let mut console = Console::take(options.scope == Scope::System);
    let _foreground = ForegroundVt::take();
    let mut terminal = ratatui::try_init()?;
    let watched = watch(
        &mut state,
        &mut feed,
        &mut terminal,
        &terminate,
        &options.palette,
        &mut console,
        options.hold,
    );
    ratatui::restore();
    watched
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

fn watch(
    state: &mut BootState,
    feed: &mut Feed,
    terminal: &mut DefaultTerminal,
    terminate: &AtomicBool,
    palette: &Palette,
    console: &mut Console,
    hold: Duration,
) -> Result<(), Error> {
    let host = uname().nodename().to_string_lossy().into_owned();
    let mut frame: u64 = 0;
    let mut painted_connections = feed.connections;
    let mut last_full_repaint = Instant::now();
    loop {
        feed.pump(state, FRAME_INTERVAL)?;
        let repaint_due = feed.connections != painted_connections
            || last_full_repaint.elapsed() >= FULL_REPAINT_INTERVAL;
        if repaint_due {
            console.keep_up();
        }
        let completed = terminal.draw(|canvas| {
            canvas.render_widget(
                BootView {
                    state,
                    palette,
                    glyphs: &console.glyphs,
                    now: monotonic_now(),
                    host: &host,
                    frame,
                },
                canvas.area(),
            );
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
                state.root.as_deref().unwrap_or("boot")
            );
            sleep(hold);
            return Ok(());
        }
        if terminate.load(Ordering::Relaxed) {
            return Ok(());
        }
        if event::poll(Duration::ZERO)? && asks_to_quit(&event::read()?) {
            return Ok(());
        }
        frame += 1;
    }
}
