use std::{
    collections::HashMap,
    iter::once,
    mem, process,
    time::{Duration, Instant},
};

use dbus::Path;
use futures::{Stream, StreamExt, stream};
use tokio::{io::unix::AsyncFd, time::sleep};

use crate::{
    boot_model::{BootChange, BootState, HANDOFF_UNIT, JobKind, Phase},
    console_session::{
        stream_sources::{
            CarriedItem, FRAME_INTERVAL, readable_or_timeout, readable_source, unfold_carrying,
        },
        watch_screen::ScreenInput,
    },
    systemd_link::{
        link_error::Error,
        monotonic_clock::monotonic_now,
        switch_root::Sysroot,
        systemd_bus::{FetchedUnit, JobListing, Scope, Session, Signal},
    },
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
    Connected(AsyncFd<Session>),
    Connecting { deadline: Instant },
    Finished,
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

struct TrackedUnit {
    name: String,
    change: Option<BootChange>,
}

struct BusFeed {
    scope: Scope,
    target: RootRequest,
    phase: Phase,
    link: Link,
    unit_paths: HashMap<Path<'static>, String>,
    root_fs: RootFs,
    gave_origin: bool,
}

fn boot_inputs(changes: Vec<BootChange>) -> Vec<ScreenInput> {
    if changes.is_empty() {
        return Vec::new();
    }
    vec![ScreenInput::Boot(changes)]
}

impl BusFeed {
    fn new(phase: Phase, scope: Scope, target: RootRequest) -> Self {
        Self {
            scope,
            target,
            root_fs: if phase == Phase::Initrd {
                RootFs::Initrd
            } else {
                RootFs::Host
            },
            phase,
            link: Link::connecting(),
            unit_paths: HashMap::new(),
            gave_origin: false,
        }
    }

    async fn next_inputs(&mut self) -> Option<Vec<ScreenInput>> {
        if let Link::Connected(readiness) = &self.link {
            readable_or_timeout(readiness, FRAME_INTERVAL).await;
        }
        match mem::replace(&mut self.link, Link::Finished) {
            Link::Finished => None,
            Link::Connecting { deadline } => Some(self.connect(deadline).await),
            Link::Connected(readiness) => Some(self.receive(readiness)),
        }
    }

    async fn connect(&mut self, deadline: Instant) -> Vec<ScreenInput> {
        match self.open_session() {
            Ok(changes) => vec![ScreenInput::Boot(changes), ScreenInput::Reconnected],
            Err(_) if Instant::now() < deadline => {
                sleep(FRAME_INTERVAL).await;
                self.link = Link::Connecting { deadline };
                Vec::new()
            }
            Err(_) => vec![ScreenInput::Failed(Error::ManagerUnreachable)],
        }
    }

    fn receive(&mut self, readiness: AsyncFd<Session>) -> Vec<ScreenInput> {
        let Ok(signals) = readiness.get_ref().receive() else {
            return self.lose_connection();
        };
        let changes = signals
            .into_iter()
            .flat_map(|signal| self.changes_for(readiness.get_ref(), signal))
            .collect();
        self.link = Link::Connected(readiness);
        boot_inputs(changes)
    }

    fn lose_connection(&mut self) -> Vec<ScreenInput> {
        let at = monotonic_now();
        let RootFs::SwitchingTo(sysroot) = mem::replace(&mut self.root_fs, RootFs::Host) else {
            if self.phase == Phase::Shutdown {
                return boot_inputs(vec![BootChange::ManagerExited { at }]);
            }
            self.link = Link::connecting();
            return boot_inputs(vec![BootChange::ForgetJobs]);
        };
        if let Err(error) = sysroot.enter() {
            return vec![ScreenInput::Failed(error.into())];
        }
        self.unit_paths.clear();
        self.phase = Phase::Host;
        self.link = Link::connecting();
        boot_inputs(vec![BootChange::SwitchRoot { at }])
    }

    fn requested_root(&self, session: &Session, jobs: &[JobListing]) -> Result<String, Error> {
        let RootRequest::Named(target) = &self.target else {
            return if self.phase == Phase::Shutdown {
                Ok(SHUTDOWN_ROOTS
                    .into_iter()
                    .find(|root| {
                        jobs.iter()
                            .any(|job| job.unit == *root && job.kind == JobKind::BringsUp)
                    })
                    .unwrap_or(SHUTDOWN_FALLBACK_ROOT)
                    .to_owned())
            } else {
                session.default_target()
            };
        };
        Ok(target.clone())
    }

    fn origin_change(&mut self, session: &Session) -> Result<Option<BootChange>, Error> {
        if self.gave_origin {
            return Ok(None);
        }
        self.gave_origin = true;
        let origin = if session.boot_in_progress()? {
            Duration::ZERO
        } else {
            monotonic_now()
        };
        Ok(Some(BootChange::SetOrigin(origin)))
    }

    fn open_session(&mut self) -> Result<Vec<BootChange>, Error> {
        let session = Session::open(self.scope)?;
        session.subscribe()?;
        if self.scope == Scope::System {
            session.set_show_status(SILENT_STATUS).ok();
        }
        let shutting_down = self.phase == Phase::Shutdown;
        let jobs: Vec<JobListing> = session
            .list_jobs()?
            .into_iter()
            .filter(|job| shutting_down || job.kind == JobKind::BringsUp)
            .collect();
        let requested_root = self.requested_root(&session, &jobs)?;
        let origin_change = self.origin_change(&session)?;
        let tracked_root = self.track(&session, &requested_root);
        let root = tracked_root
            .as_ref()
            .map_or(requested_root, |tracked| tracked.name.clone());
        let tracked_handoff = self.track(&session, HANDOFF_UNIT);
        let handoff = tracked_handoff
            .as_ref()
            .map(|tracked| BootChange::SetHandoff(tracked.name.clone()));
        let milestones = BootState::new(self.phase).milestone_targets;
        let tracked_changes: Vec<BootChange> = jobs
            .iter()
            .map(|job| job.unit.as_str())
            .chain(milestones.iter().map(String::as_str))
            .filter_map(|unit| self.track(&session, unit))
            .filter_map(|tracked| tracked.change)
            .collect();
        eprintln!(
            "bootvine: watching {root} in {} as pid {} over {}",
            self.phase.label(),
            process::id(),
            if session.peer_to_peer {
                "the private socket"
            } else {
                "the bus"
            },
        );
        let queued_at = monotonic_now();
        let changes = origin_change
            .into_iter()
            .chain(tracked_root.and_then(|tracked| tracked.change))
            .chain(tracked_handoff.and_then(|tracked| tracked.change))
            .chain(handoff)
            .chain(tracked_changes)
            .chain(once(BootChange::SetRoot(root)))
            .chain(jobs.into_iter().map(|job| BootChange::QueueJob {
                id: job.id,
                unit: job.unit,
                kind: job.kind,
                at: queued_at,
            }))
            .collect();
        self.link = Link::Connected(readable_source(session)?);
        Ok(changes)
    }

    fn track(&mut self, session: &Session, unit: &str) -> Option<TrackedUnit> {
        if let Some(known) = self.unit_paths.values().find(|known| *known == unit) {
            return Some(TrackedUnit {
                name: known.clone(),
                change: None,
            });
        }
        let FetchedUnit {
            path,
            facts,
            activity,
        } = session.fetch_unit(unit).ok()?;
        let name = facts.name.clone();
        if self.unit_paths.values().any(|known| *known == name) {
            return Some(TrackedUnit { name, change: None });
        }
        self.unit_paths.insert(path, name.clone());
        Some(TrackedUnit {
            name,
            change: Some(BootChange::TrackUnit { facts, activity }),
        })
    }

    fn changes_for(&mut self, session: &Session, signal: Signal) -> Vec<BootChange> {
        let at = monotonic_now();
        match signal {
            Signal::JobNew { id, job, unit } => self.queue_new_job(session, id, &job, unit, at),
            Signal::JobRemoved { id, result } => vec![BootChange::FinishJob { id, result, at }],
            Signal::UnitChanged { path, activity } => self
                .unit_paths
                .get(&path)
                .map(|unit| BootChange::UpdateActivity {
                    unit: unit.clone(),
                    activity,
                })
                .into_iter()
                .collect(),
        }
    }

    fn queue_new_job(
        &mut self,
        session: &Session,
        id: u32,
        job: &Path<'static>,
        unit: String,
        at: Duration,
    ) -> Vec<BootChange> {
        let kind = session.job_kind(job).unwrap_or(JobKind::Unknown);
        if kind == JobKind::TakesDown && self.phase != Phase::Shutdown {
            return Vec::new();
        }
        let tracked = self.track(session, &unit);
        let unit = tracked
            .as_ref()
            .map_or(unit, |tracked| tracked.name.clone());
        if matches!(self.root_fs, RootFs::Initrd)
            && unit == SWITCH_ROOT_UNIT
            && let Ok(sysroot) = Sysroot::hold()
        {
            self.root_fs = RootFs::SwitchingTo(sysroot);
        }
        tracked
            .and_then(|tracked| tracked.change)
            .into_iter()
            .chain(once(BootChange::QueueJob { id, unit, kind, at }))
            .collect()
    }
}

impl Drop for BusFeed {
    fn drop(&mut self) {
        if let Link::Connected(readiness) = &self.link
            && self.scope == Scope::System
        {
            readiness.get_ref().set_show_status(CONFIGURED_STATUS).ok();
        }
    }
}

pub fn bus_inputs(
    phase: Phase,
    scope: Scope,
    target: RootRequest,
) -> impl Stream<Item = ScreenInput> {
    unfold_carrying(BusFeed::new(phase, scope, target), |mut feed| async move {
        let inputs = feed.next_inputs().await?;
        Some(CarriedItem {
            item: inputs,
            state: feed,
        })
    })
    .flat_map(stream::iter)
}
