use std::{
    collections::HashMap,
    process,
    sync::mpsc::Sender,
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use boot_tree::model::{Activity, HANDOFF_UNIT, HOST_MILESTONES, JobResult, Micros, UnitFacts};
use dbus::Path;

use crate::{
    clock::monotonic_now,
    error::Error,
    sysroot::{Sysroot, running_in_initrd},
    systemd::{Scope, Session, Signal, brings_unit_up},
};

const RECEIVE_TIMEOUT: Duration = Duration::from_millis(100);
const RECONNECT_INTERVAL: Duration = Duration::from_millis(50);
const RECONNECT_PATIENCE: Duration = Duration::from_secs(60);
const SWITCH_ROOT_UNIT: &str = "initrd-switch-root.service";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobKind {
    BringsUp,
    Unknown,
}

pub enum Update {
    Root(String),
    Handoff(String),
    Origin(Micros),
    Unit {
        facts: UnitFacts,
        activity: Activity,
    },
    JobQueued {
        id: u32,
        unit: String,
        at: Micros,
        kind: JobKind,
    },
    JobFinished {
        id: u32,
        result: JobResult,
        at: Micros,
    },
    Activity {
        unit: String,
        activity: Activity,
    },
    RootSwitched {
        at: Micros,
    },
    Resubscribed,
    Ended(Error),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OriginChoice {
    FromManager,
    Keep,
}

pub struct FeedOptions {
    pub scope: Scope,
    pub target: Option<String>,
}

pub fn spawn(options: FeedOptions, sender: Sender<Update>) -> JoinHandle<()> {
    thread::spawn(move || {
        let ended = Feed::new(options, sender.clone()).run();
        let _ = sender.send(Update::Ended(ended));
    })
}

struct Feed {
    options: FeedOptions,
    sender: Sender<Update>,
    unit_paths: HashMap<Path<'static>, String>,
    in_initrd: bool,
    sysroot: Option<Sysroot>,
}

impl Feed {
    fn new(options: FeedOptions, sender: Sender<Update>) -> Self {
        Self {
            in_initrd: options.scope == Scope::System && running_in_initrd(),
            options,
            sender,
            unit_paths: HashMap::new(),
            sysroot: None,
        }
    }

    fn send(&self, update: Update) {
        let _ = self.sender.send(update);
    }

    fn run(mut self) -> Error {
        let mut session = match self.connect_patiently(OriginChoice::FromManager) {
            Ok(session) => session,
            Err(error) => return error,
        };
        loop {
            match session.receive(RECEIVE_TIMEOUT) {
                Ok(signals) => signals
                    .into_iter()
                    .for_each(|signal| self.apply(&session, signal)),
                Err(_) => match self.reconnect() {
                    Ok(reconnected) => session = reconnected,
                    Err(error) => return error,
                },
            }
        }
    }

    fn connect(&mut self, origin: OriginChoice) -> Result<Session, Error> {
        let session = Session::open(self.options.scope)?;
        session.subscribe()?;
        let requested_root = match &self.options.target {
            Some(target) => target.clone(),
            None => session.default_target()?,
        };
        if origin == OriginChoice::FromManager {
            let boot_in_progress = session.boot_in_progress()?;
            self.send(Update::Origin(if boot_in_progress {
                0
            } else {
                monotonic_now()
            }));
        }
        let root = self
            .track(&session, &requested_root)
            .unwrap_or(requested_root);
        self.send(Update::Root(root.clone()));
        if let Some(handoff) = self.track(&session, HANDOFF_UNIT) {
            self.send(Update::Handoff(handoff));
        }
        let jobs: Vec<_> = session
            .list_jobs()?
            .into_iter()
            .filter(|job| brings_unit_up(&job.kind))
            .collect();
        jobs.iter()
            .map(|job| job.unit.as_str())
            .chain(HOST_MILESTONES)
            .for_each(|unit| {
                self.track(&session, unit);
            });
        let now = monotonic_now();
        jobs.into_iter().for_each(|job| {
            self.send(Update::JobQueued {
                id: job.id,
                unit: job.unit,
                at: now,
                kind: JobKind::BringsUp,
            });
        });
        eprintln!(
            "boot-tree: watching {root} in {} as pid {} over {}",
            if self.in_initrd { "initrd" } else { "stage 2" },
            process::id(),
            if session.is_peer_to_peer() {
                "the private socket"
            } else {
                "the bus"
            },
        );
        Ok(session)
    }

    fn reconnect(&mut self) -> Result<Session, Error> {
        match self.sysroot.take() {
            Some(sysroot) => {
                sysroot.enter()?;
                self.in_initrd = false;
                self.unit_paths.clear();
                self.send(Update::RootSwitched {
                    at: monotonic_now(),
                });
            }
            None => self.send(Update::Resubscribed),
        }
        self.connect_patiently(OriginChoice::Keep)
    }

    fn connect_patiently(&mut self, origin: OriginChoice) -> Result<Session, Error> {
        let deadline = Instant::now() + RECONNECT_PATIENCE;
        loop {
            match self.connect(origin) {
                Ok(session) => return Ok(session),
                Err(_) if Instant::now() < deadline => thread::sleep(RECONNECT_INTERVAL),
                Err(_) => return Err(Error::ManagerUnreachable),
            }
        }
    }

    fn track(&mut self, session: &Session, unit: &str) -> Option<String> {
        if let Some(known) = self.unit_paths.values().find(|known| *known == unit) {
            return Some(known.clone());
        }
        let fetched = session.fetch_unit(unit).ok()?;
        let canonical = fetched.facts.name.clone();
        if self.unit_paths.values().any(|known| *known == canonical) {
            return Some(canonical);
        }
        self.unit_paths.insert(fetched.path, canonical.clone());
        self.send(Update::Unit {
            facts: fetched.facts,
            activity: fetched.activity,
        });
        Some(canonical)
    }

    fn apply(&mut self, session: &Session, signal: Signal) {
        match signal {
            Signal::JobNew { id, job, unit } => {
                let kind = session.job_kind(&job);
                if kind.as_deref().is_ok_and(|kind| !brings_unit_up(kind)) {
                    return;
                }
                let unit = self.track(session, &unit).unwrap_or(unit);
                if self.in_initrd && unit == SWITCH_ROOT_UNIT {
                    self.sysroot = Sysroot::hold().ok();
                }
                self.send(Update::JobQueued {
                    id,
                    unit,
                    at: monotonic_now(),
                    kind: if kind.is_ok() {
                        JobKind::BringsUp
                    } else {
                        JobKind::Unknown
                    },
                });
            }
            Signal::JobRemoved { id, result } => self.send(Update::JobFinished {
                id,
                result,
                at: monotonic_now(),
            }),
            Signal::UnitChanged { path, activity } => {
                if let Some(unit) = self.unit_paths.get(&path) {
                    self.send(Update::Activity {
                        unit: unit.clone(),
                        activity,
                    });
                }
            }
        }
    }
}
