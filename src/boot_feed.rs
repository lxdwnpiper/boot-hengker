use std::{
    collections::HashMap,
    mem,
    os::fd::{AsFd, BorrowedFd},
    process,
    time::{Duration, Instant},
};

use bootvine::boot_model::{BootState, HANDOFF_UNIT, JobKind, Phase};
use dbus::Path;

use crate::{
    monotonic_clock::monotonic_now,
    switch_root::Sysroot,
    systemd_bus::{Scope, Session, Signal},
    watch_error::Error,
};

const RECONNECT_PATIENCE: Duration = Duration::from_secs(60);
const SWITCH_ROOT_UNIT: &str = "initrd-switch-root.service";
const SHUTDOWN_ROOTS: [&str; 5] = [
    "poweroff.target",
    "reboot.target",
    "halt.target",
    "kexec.target",
    "soft-reboot.target",
];
const SHUTDOWN_FALLBACK_ROOT: &str = "shutdown.target";
const SILENT_STATUS: &str = "no";
const CONFIGURED_STATUS: &str = "";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RootRequest {
    Named(String),
    ManagerDefault,
}

enum Link {
    Connected(Session),
    Connecting { deadline: Instant },
}

impl Link {
    fn connecting() -> Self {
        Self::Connecting {
            deadline: Instant::now() + RECONNECT_PATIENCE,
        }
    }
}

enum RootFs {
    Initrd,
    SwitchingTo(Sysroot),
    Host,
}

pub struct Feed {
    pub connections: u32,
    scope: Scope,
    target: RootRequest,
    link: Link,
    unit_paths: HashMap<Path<'static>, String>,
    root_fs: RootFs,
    gave_origin: bool,
}

impl Feed {
    pub fn new(phase: Phase, scope: Scope, target: RootRequest) -> Self {
        Self {
            connections: 0,
            root_fs: if phase == Phase::Initrd {
                RootFs::Initrd
            } else {
                RootFs::Host
            },
            scope,
            target,
            link: Link::connecting(),
            unit_paths: HashMap::new(),
            gave_origin: false,
        }
    }

    pub fn session_fd(&self) -> Option<BorrowedFd<'_>> {
        match &self.link {
            Link::Connected(session) => Some(session.as_fd()),
            Link::Connecting { .. } => None,
        }
    }

    pub fn pump(&mut self, state: &mut BootState) -> Result<(), Error> {
        if state.manager_exited_at.is_some() {
            return Ok(());
        }
        match mem::replace(&mut self.link, Link::connecting()) {
            Link::Connecting { deadline } => self.connect(state, deadline),
            Link::Connected(session) => match session.receive() {
                Ok(signals) => {
                    signals
                        .into_iter()
                        .for_each(|signal| self.apply(&session, state, signal));
                    self.link = Link::Connected(session);
                    Ok(())
                }
                Err(_) => self.lose_connection(state),
            },
        }
    }

    fn connect(&mut self, state: &mut BootState, deadline: Instant) -> Result<(), Error> {
        match self.open_session(state) {
            Ok(session) => {
                self.link = Link::Connected(session);
                self.connections += 1;
                Ok(())
            }
            Err(_) if Instant::now() < deadline => {
                self.link = Link::Connecting { deadline };
                Ok(())
            }
            Err(_) => Err(Error::ManagerUnreachable),
        }
    }

    fn lose_connection(&mut self, state: &mut BootState) -> Result<(), Error> {
        match &self.root_fs {
            RootFs::SwitchingTo(sysroot) => {
                sysroot.enter()?;
                self.root_fs = RootFs::Host;
                self.unit_paths.clear();
                state.switch_root(monotonic_now());
            }
            RootFs::Initrd | RootFs::Host if state.phase == Phase::Shutdown => {
                state.manager_exited_at = Some(monotonic_now());
            }
            RootFs::Initrd | RootFs::Host => state.forget_jobs(),
        }
        Ok(())
    }

    fn open_session(&mut self, state: &mut BootState) -> Result<Session, Error> {
        let session = Session::open(self.scope)?;
        session.subscribe()?;
        if self.scope == Scope::System {
            session.set_show_status(SILENT_STATUS).ok();
        }
        let shutting_down = state.phase == Phase::Shutdown;
        let jobs: Vec<_> = session
            .list_jobs()?
            .into_iter()
            .filter(|job| shutting_down || job.kind == JobKind::BringsUp)
            .collect();
        let requested_root = match &self.target {
            RootRequest::Named(target) => target.clone(),
            RootRequest::ManagerDefault if shutting_down => SHUTDOWN_ROOTS
                .into_iter()
                .find(|root| {
                    jobs.iter()
                        .any(|job| job.unit == *root && job.kind == JobKind::BringsUp)
                })
                .unwrap_or(SHUTDOWN_FALLBACK_ROOT)
                .to_owned(),
            RootRequest::ManagerDefault => session.default_target()?,
        };
        let milestones = state.milestone_targets.clone();
        if !self.gave_origin {
            state.origin = if session.boot_in_progress()? {
                Duration::ZERO
            } else {
                monotonic_now()
            };
            self.gave_origin = true;
        }
        let root = self
            .track(&session, state, &requested_root)
            .unwrap_or(requested_root);
        if let Some(handoff) = self.track(&session, state, HANDOFF_UNIT) {
            state.set_handoff(handoff);
        }
        jobs.iter()
            .map(|job| job.unit.as_str())
            .chain(milestones.iter().map(String::as_str))
            .for_each(|unit| {
                self.track(&session, state, unit);
            });
        state.set_root(root.clone());
        let now = monotonic_now();
        jobs.iter()
            .for_each(|job| state.queue_job(job.id, &job.unit, job.kind, now));
        eprintln!(
            "bootvine: watching {root} in {} as pid {} over {}",
            state.phase.label(),
            process::id(),
            if session.peer_to_peer {
                "the private socket"
            } else {
                "the bus"
            },
        );
        Ok(session)
    }

    fn track(&mut self, session: &Session, state: &mut BootState, unit: &str) -> Option<String> {
        if let Some(known) = self.unit_paths.values().find(|known| *known == unit) {
            return Some(known.clone());
        }
        let fetched = session.fetch_unit(unit).ok()?;
        let canonical = fetched.facts.name.clone();
        if self.unit_paths.values().any(|known| *known == canonical) {
            return Some(canonical);
        }
        self.unit_paths.insert(fetched.path, canonical.clone());
        state.track_unit(fetched.facts, fetched.activity);
        Some(canonical)
    }

    fn apply(&mut self, session: &Session, state: &mut BootState, signal: Signal) {
        match signal {
            Signal::JobNew { id, job, unit } => {
                let kind = session.job_kind(&job).unwrap_or(JobKind::Unknown);
                if kind == JobKind::TakesDown && state.phase != Phase::Shutdown {
                    return;
                }
                let unit = self.track(session, state, &unit).unwrap_or(unit);
                if matches!(self.root_fs, RootFs::Initrd)
                    && unit == SWITCH_ROOT_UNIT
                    && let Ok(sysroot) = Sysroot::hold()
                {
                    self.root_fs = RootFs::SwitchingTo(sysroot);
                }
                state.queue_job(id, &unit, kind, monotonic_now());
            }
            Signal::JobRemoved { id, result } => state.finish_job(id, result, monotonic_now()),
            Signal::UnitChanged { path, activity } => {
                if let Some(unit) = self.unit_paths.get(&path) {
                    state.update_activity(unit, activity);
                }
            }
        }
    }
}

impl Drop for Feed {
    fn drop(&mut self) {
        if let Link::Connected(session) = &self.link
            && self.scope == Scope::System
        {
            session.set_show_status(CONFIGURED_STATUS).ok();
        }
    }
}
