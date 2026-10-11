use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
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
use rustix::system::uname;
use signal_hook::{consts::SIGTERM, flag};

use crate::{
    clock::monotonic_now, console::ForegroundVt, error::Error, feed::Feed, sysroot::INITRD_RELEASE,
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
    let phase = match options.scope {
        Scope::User => Phase::User,
        Scope::System if Path::new(INITRD_RELEASE).exists() => Phase::Initrd,
        Scope::System => Phase::Host,
    };
    let mut state = BootState::new(phase);
    let mut feed = Feed::new(options.scope, options.target);
    let _foreground = ForegroundVt::take();
    let mut terminal = ratatui::try_init()?;
    let watched = watch(
        &mut state,
        &mut feed,
        &mut terminal,
        &terminate,
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

fn watch(
    state: &mut BootState,
    feed: &mut Feed,
    terminal: &mut DefaultTerminal,
    terminate: &AtomicBool,
    hold: Duration,
) -> Result<(), Error> {
    let host = uname().nodename().to_string_lossy().into_owned();
    let mut frame: u64 = 0;
    loop {
        feed.pump(state, FRAME_INTERVAL)?;
        if frame.is_multiple_of(FRAMES_PER_FULL_REPAINT) {
            terminal.backend_mut().clear_region(ClearType::All)?;
            terminal.swap_buffers();
        }
        terminal.draw(|canvas| {
            canvas.render_widget(
                BootView {
                    state,
                    now: monotonic_now(),
                    host: &host,
                    frame,
                },
                canvas.area(),
            );
        })?;
        if state.is_complete() {
            eprintln!(
                "boot-tree: {} reached, handing the console over",
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
