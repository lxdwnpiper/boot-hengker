use std::{path::Path, time::Duration};

use futures::{StreamExt, stream};
use tokio::{runtime::Builder, time::sleep};

use crate::{
    boot_model::{BootState, Phase, boot_origin::BootOrigin},
    console_session::{
        ask_password::RequestWatch,
        console_takeover::Console,
        live_desk::{
            LiveDesk, console_upkeep_inputs, full_repaint_inputs, password_request_inputs,
            prompt_check_inputs,
        },
        stream_sources::{frame_inputs, key_inputs, termination_inputs},
        watch_screen::{WatchEnding, WatchScreen, watch_until_ending},
    },
    screen_settings::{color_scheme::Colors, config_file::Timing},
    systemd_link::{
        boot_feed::{RootRequest, bus_inputs},
        link_error::Error,
        switch_root::INITRD_RELEASE,
        systemd_bus::{Scope, Session, StatusOutput},
    },
};

pub struct Settings {
    pub scope: Scope,
    pub shutdown: bool,
    pub target: RootRequest,
    pub timing: Timing,
    pub colors: Colors,
}

struct ConfiguredStatusOnExit {
    scope: Scope,
}

impl Drop for ConfiguredStatusOnExit {
    fn drop(&mut self) {
        if self.scope == Scope::System
            && let Ok(session) = Session::open(Scope::System)
        {
            session.set_show_status(StatusOutput::Configured).ok();
        }
    }
}

fn boot_already_finished(settings: &Settings) -> bool {
    settings.scope == Scope::System
        && !settings.shutdown
        && Session::open(Scope::System)
            .and_then(|session| session.boot_in_progress())
            .is_ok_and(|booting| !booting)
}

fn starting_phase(settings: &Settings) -> Phase {
    match settings.scope {
        _ if settings.shutdown => Phase::Shutdown,
        Scope::User => Phase::User,
        Scope::System if Path::new(INITRD_RELEASE).exists() => Phase::Initrd,
        Scope::System => Phase::Host,
    }
}

pub fn run(settings: &Settings) -> Result<(), Error> {
    if boot_already_finished(settings) {
        eprintln!("bootvine: boot already finished, leaving the console alone");
        return Ok(());
    }
    Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()?
        .block_on(watch_live_boot(settings, starting_phase(settings)))
}

async fn watch_live_boot(settings: &Settings, phase: Phase) -> Result<(), Error> {
    let _restores_status = ConfiguredStatusOnExit {
        scope: settings.scope,
    };
    let requests = RequestWatch::open();
    let desk = LiveDesk::new(
        Console::take(settings.scope == Scope::System),
        requests.is_some(),
    );
    let password_inputs = requests.map(password_request_inputs).transpose()?;
    let event_inputs = stream::select(
        stream::select(
            bus_inputs(phase, settings.scope, settings.target.clone()),
            key_inputs()?,
        ),
        stream::select(
            termination_inputs()?,
            stream::iter(password_inputs).flatten(),
        ),
    );
    let timer_inputs = stream::select(
        stream::select(frame_inputs(), prompt_check_inputs()),
        stream::select(console_upkeep_inputs(), full_repaint_inputs()),
    );
    let inputs = stream::select(event_inputs, timer_inputs);
    let mut screen = WatchScreen::new(
        BootState::new(phase),
        desk,
        BootOrigin::read(),
        settings.colors,
        settings.timing,
        Duration::ZERO,
    );
    let mut terminal = ratatui::try_init()?;
    let ending = watch_until_ending(&mut screen, &mut terminal, inputs).await;
    if matches!(ending, WatchEnding::BootCompleted) {
        sleep(settings.timing.keep_final_frame_for).await;
    }
    ratatui::restore();
    match ending {
        WatchEnding::Failed(error) => Err(error),
        WatchEnding::BootCompleted | WatchEnding::ViewerLeft => Ok(()),
    }
}
