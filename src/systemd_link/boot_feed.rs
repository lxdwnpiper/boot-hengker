use std::{
    collections::HashMap,
    iter::once,
    mem, process,
    time::{Duration, Instant},
};

use dbus::Path;
use futures::{Stream, StreamExt, stream};
use tokio::time::sleep;

use crate::{
    boot_model::{BootChange, BootState, HANDOFF_UNIT, JobKind, Phase},
    console_session::{
        stream_sources::{BACKGROUND_CHECK_INTERVAL, CarriedItem, unfold_carrying},
        watch_screen::ScreenInput,
    },
    systemd_link::{
        link_error::Error,
        monotonic_clock::monotonic_now,
        switch_root::Sysroot,
        systemd_bus::{
            BusConnection, FetchedUnit, JobListing, Scope, Session, Signal, StatusOutput,
        },
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RootRequest {
    Named(String),
    ManagerDefault,
}

enum Link {
    Connected(BusConnection),
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

struct FeedProgress<'feed> {
    feed: &'feed mut BusFeed,
    connection: &'feed mut BusConnection,
    changes: Vec<BootChange>,
}

struct WatchedConnection {
    connection: BusConnection,
    changes: Vec<BootChange>,
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
        match mem::replace(&mut self.link, Link::Finished) {
            Link::Finished => None,
            Link::Connecting { deadline } => Some(self.connect(deadline).await),
            Link::Connected(connection) => Some(self.receive(connection).await),
        }
    }

    async fn open_watched(&mut self) -> Result<WatchedConnection, Error> {
        let mut connection = BusConnection::register(Session::open(self.scope)?)?;
        let changes = self.start_watching(&mut connection).await?;
        Ok(WatchedConnection {
            connection,
            changes,
        })
    }

    async fn connect(&mut self, deadline: Instant) -> Vec<ScreenInput> {
        match self.open_watched().await {
            Ok(watched) => {
                self.link = Link::Connected(watched.connection);
                vec![ScreenInput::Boot(watched.changes), ScreenInput::FullRepaint]
            }
            Err(_) if Instant::now() < deadline => {
                sleep(BACKGROUND_CHECK_INTERVAL).await;
                self.link = Link::Connecting { deadline };
                Vec::new()
            }
            Err(_) => vec![ScreenInput::Failed(Error::ManagerUnreachable)],
        }
    }

    async fn receive(&mut self, mut connection: BusConnection) -> Vec<ScreenInput> {
        let Ok(signals) = connection.receive_signals(BACKGROUND_CHECK_INTERVAL).await else {
            return self.lose_connection();
        };
        let progress = stream::iter(signals)
            .fold(
                FeedProgress {
                    feed: &mut *self,
                    connection: &mut connection,
                    changes: Vec::new(),
                },
                |mut progress, signal| async move {
                    let changes = progress.feed.changes_for(progress.connection, signal).await;
                    progress.changes.extend(changes);
                    progress
                },
            )
            .await;
        let changes = progress.changes;
        self.link = Link::Connected(connection);
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

    async fn requested_root(
        &self,
        connection: &mut BusConnection,
        jobs: &[JobListing],
    ) -> Result<String, Error> {
        let RootRequest::Named(target) = &self.target else {
            if self.phase != Phase::Shutdown {
                return connection.default_target().await;
            }
            return Ok(SHUTDOWN_ROOTS
                .into_iter()
                .find(|root| {
                    jobs.iter()
                        .any(|job| job.unit == *root && job.kind == JobKind::BringsUp)
                })
                .unwrap_or(SHUTDOWN_FALLBACK_ROOT)
                .to_owned());
        };
        Ok(target.clone())
    }

    async fn origin_change(
        &mut self,
        connection: &mut BusConnection,
    ) -> Result<Vec<BootChange>, Error> {
        if self.gave_origin {
            return Ok(Vec::new());
        }
        self.gave_origin = true;
        if connection.boot_in_progress().await? {
            return Ok(vec![BootChange::SetOrigin(Duration::ZERO)]);
        }
        Ok(once(BootChange::SetOrigin(monotonic_now()))
            .chain(self.boot_finished())
            .collect())
    }

    async fn track_all(
        &mut self,
        connection: &mut BusConnection,
        units: Vec<String>,
    ) -> Vec<BootChange> {
        stream::iter(units)
            .fold(
                FeedProgress {
                    feed: &mut *self,
                    connection: &mut *connection,
                    changes: Vec::new(),
                },
                |mut progress, unit| async move {
                    let tracked = progress.feed.track(progress.connection, &unit).await;
                    progress
                        .changes
                        .extend(tracked.and_then(|tracked| tracked.change));
                    progress
                },
            )
            .await
            .changes
    }

    async fn start_watching(
        &mut self,
        connection: &mut BusConnection,
    ) -> Result<Vec<BootChange>, Error> {
        connection.subscribe().await?;
        if self.scope == Scope::System {
            connection.set_show_status(StatusOutput::Silent).await.ok();
        }
        let shutting_down = self.phase == Phase::Shutdown;
        let jobs: Vec<JobListing> = connection
            .list_jobs()
            .await?
            .into_iter()
            .filter(|job| shutting_down || job.kind == JobKind::BringsUp)
            .collect();
        let requested_root = self.requested_root(connection, &jobs).await?;
        let origin_change = self.origin_change(connection).await?;
        let tracked_root = self.track(connection, &requested_root).await;
        let root = tracked_root
            .as_ref()
            .map_or(requested_root, |tracked| tracked.name.clone());
        let tracked_handoff = self.track(connection, HANDOFF_UNIT).await;
        let handoff = tracked_handoff
            .as_ref()
            .map(|tracked| BootChange::SetHandoff(tracked.name.clone()));
        let watched_units = jobs
            .iter()
            .map(|job| job.unit.clone())
            .chain(BootState::new(self.phase).milestone_targets)
            .collect();
        let tracked_changes = self.track_all(connection, watched_units).await;
        eprintln!(
            "bootvine: watching {root} in {} as pid {} over {}",
            self.phase.label(),
            process::id(),
            if connection.peer_to_peer() {
                "the private socket"
            } else {
                "the bus"
            },
        );
        let queued_at = monotonic_now();
        Ok(origin_change
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
            .collect())
    }

    async fn track(&mut self, connection: &mut BusConnection, unit: &str) -> Option<TrackedUnit> {
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
        } = connection.fetch_unit(unit).await.ok()?;
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

    async fn changes_for(
        &mut self,
        connection: &mut BusConnection,
        signal: Signal,
    ) -> Vec<BootChange> {
        let at = monotonic_now();
        match signal {
            Signal::JobNew { id, job, unit } => {
                self.queue_new_job(connection, id, &job, unit, at).await
            }
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
            Signal::BootFinished => self.boot_finished(),
        }
    }

    fn boot_finished(&self) -> Vec<BootChange> {
        (self.phase == Phase::Host)
            .then_some(BootChange::BootFinished)
            .into_iter()
            .collect()
    }

    async fn queue_new_job(
        &mut self,
        connection: &mut BusConnection,
        id: u32,
        job: &Path<'static>,
        unit: String,
        at: Duration,
    ) -> Vec<BootChange> {
        let kind = connection.job_kind(job).await.unwrap_or(JobKind::Unknown);
        if kind == JobKind::TakesDown && self.phase != Phase::Shutdown {
            return Vec::new();
        }
        let tracked = self.track(connection, &unit).await;
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
