use std::{
    collections::HashMap,
    path::Path as FilePath,
    process,
    thread::sleep,
    time::{Duration, Instant},
};

use boot_tree::model::{BootState, HANDOFF_UNIT, HOST_MILESTONES};
use dbus::Path;

use crate::{
    clock::monotonic_now,
    error::Error,
    sysroot::{INITRD_RELEASE, Sysroot},
    systemd::{Scope, Session, Signal},
};

const RECONNECT_PATIENCE: Duration = Duration::from_secs(60);
const SWITCH_ROOT_UNIT: &str = "initrd-switch-root.service";
const SILENT_STATUS: &str = "no";
const CONFIGURED_STATUS: &str = "";

pub struct Feed {
    scope: Scope,
    target: Option<String>,
    session: Option<Session>,
    unit_paths: HashMap<Path<'static>, String>,
    in_initrd: bool,
    sysroot: Option<Sysroot>,
    gave_origin: bool,
    reconnect_deadline: Instant,
}

impl Feed {
    pub fn new(scope: Scope, target: Option<String>) -> Self {
        Self {
            in_initrd: scope == Scope::System && FilePath::new(INITRD_RELEASE).exists(),
            scope,
            target,
            session: None,
            unit_paths: HashMap::new(),
            sysroot: None,
            gave_origin: false,
            reconnect_deadline: Instant::now() + RECONNECT_PATIENCE,
        }
    }

    pub fn pump(&mut self, state: &mut BootState, wait: Duration) -> Result<(), Error> {
        let Some(session) = self.session.take() else {
            return self.connect_or_wait(state, wait);
        };
        match session.receive(wait) {
            Ok(signals) => {
                signals
                    .into_iter()
                    .for_each(|signal| self.apply(&session, state, signal));
                self.session = Some(session);
            }
            Err(_) => self.lose_connection(state)?,
        }
        Ok(())
    }

    fn connect_or_wait(&mut self, state: &mut BootState, wait: Duration) -> Result<(), Error> {
        match self.connect(state) {
            Ok(session) => {
                self.session = Some(session);
                Ok(())
            }
            Err(_) if Instant::now() < self.reconnect_deadline => {
                sleep(wait);
                Ok(())
            }
            Err(_) => Err(Error::ManagerUnreachable),
        }
    }

    fn lose_connection(&mut self, state: &mut BootState) -> Result<(), Error> {
        match self.sysroot.take() {
            Some(sysroot) => {
                sysroot.enter()?;
                self.in_initrd = false;
                self.unit_paths.clear();
                state.switch_root(monotonic_now());
            }
            None => state.forget_jobs(),
        }
        self.reconnect_deadline = Instant::now() + RECONNECT_PATIENCE;
        Ok(())
    }

    fn connect(&mut self, state: &mut BootState) -> Result<Session, Error> {
        let session = Session::open(self.scope)?;
        session.subscribe()?;
        if self.scope == Scope::System {
            session.set_show_status(SILENT_STATUS).ok();
        }
        let requested_root = match &self.target {
            Some(target) => target.clone(),
            None => session.default_target()?,
        };
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
        let jobs: Vec<_> = session
            .list_jobs()?
            .into_iter()
            .filter(|job| job.brings_unit_up)
            .collect();
        jobs.iter()
            .map(|job| job.unit.as_str())
            .chain(HOST_MILESTONES)
            .for_each(|unit| {
                self.track(&session, state, unit);
            });
        state.set_root(root.clone());
        let now = monotonic_now();
        jobs.iter()
            .for_each(|job| state.queue_job(job.id, &job.unit, now));
        eprintln!(
            "boot-tree: watching {root} in {} as pid {} over {}",
            if self.in_initrd { "initrd" } else { "stage 2" },
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
                let brings_unit_up = session.job_brings_unit_up(&job);
                if brings_unit_up.as_ref().is_ok_and(|up| !up) {
                    return;
                }
                let unit = self.track(session, state, &unit).unwrap_or(unit);
                if self.in_initrd && unit == SWITCH_ROOT_UNIT {
                    self.sysroot = Sysroot::hold().ok();
                }
                match brings_unit_up {
                    Ok(_) => state.queue_job(id, &unit, monotonic_now()),
                    Err(_) => state.queue_job_of_unknown_kind(id, &unit, monotonic_now()),
                }
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
        if let Some(session) = &self.session
            && self.scope == Scope::System
        {
            session.set_show_status(CONFIGURED_STATUS).ok();
        }
    }
}
