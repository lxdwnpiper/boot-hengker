use std::{
    collections::HashMap,
    iter::once,
    process,
    time::{Duration, Instant},
};

use dbus::Path;
use futures::{Stream, StreamExt, stream};
use tokio::time::sleep;

use crate::{
    boot_model::{Activity, BootChange, BootState, HANDOFF_UNIT, JobKind, Phase, UnitFacts},
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum ConnectAttempt {
    First,
    Again,
}

enum Link {
    Connected(BusConnection),
    Connecting {
        deadline: Instant,
        attempt: ConnectAttempt,
    },
    Finished,
}

impl Link {
    fn connecting(attempt: ConnectAttempt) -> Self {
        Self::Connecting {
            deadline: Instant::now() + RECONNECT_PATIENCE,
            attempt,
        }
    }
}

enum RootSwitch {
    NotQueued,
    Queued(Sysroot),
}

enum TrackedUnit {
    Known(String),
    New {
        facts: UnitFacts,
        activity: Activity,
    },
}

impl TrackedUnit {
    fn name(&self) -> &str {
        match self {
            Self::Known(name) => name,
            Self::New { facts, .. } => &facts.name,
        }
    }

    fn into_change(self) -> Option<BootChange> {
        match self {
            Self::Known(_) => None,
            Self::New { facts, activity } => Some(BootChange::TrackUnit { facts, activity }),
        }
    }
}

struct BootTracker {
    scope: Scope,
    target: RootRequest,
    phase: Phase,
    unit_paths: HashMap<Path<'static>, String>,
    root_switch: RootSwitch,
}

struct BusFeed {
    link: Link,
    tracker: BootTracker,
}

struct FeedProgress<'feed> {
    tracker: &'feed mut BootTracker,
    connection: &'feed mut BusConnection,
    changes: Vec<BootChange>,
}

impl BusFeed {
    fn new(phase: Phase, scope: Scope, target: RootRequest) -> Self {
        Self {
            link: Link::connecting(ConnectAttempt::First),
            tracker: BootTracker {
                scope,
                target,
                phase,
                unit_paths: HashMap::new(),
                root_switch: RootSwitch::NotQueued,
            },
        }
    }

    async fn next_inputs(&mut self) -> Option<Vec<ScreenInput>> {
        match &mut self.link {
            Link::Finished => None,
            Link::Connecting { deadline, attempt } => {
                let deadline = *deadline;
                let attempt = *attempt;
                Some(self.connect(deadline, attempt).await)
            }
            Link::Connected(connection) => {
                let received = self.tracker.receive(connection).await;
                Some(match received {
                    Ok(changes) if changes.is_empty() => Vec::new(),
                    Ok(changes) => vec![ScreenInput::Boot(changes)],
                    Err(_) => self.lose_connection(),
                })
            }
        }
    }

    async fn open_watched(&mut self, attempt: ConnectAttempt) -> Result<Vec<BootChange>, Error> {
        let mut connection = BusConnection::register(Session::open(self.tracker.scope)?)?;
        let changes = self
            .tracker
            .start_watching(&mut connection, attempt)
            .await?;
        self.link = Link::Connected(connection);
        Ok(changes)
    }

    async fn connect(&mut self, deadline: Instant, attempt: ConnectAttempt) -> Vec<ScreenInput> {
        match self.open_watched(attempt).await {
            Ok(changes) => vec![ScreenInput::Boot(changes), ScreenInput::FullRepaint],
            Err(_) if Instant::now() < deadline => {
                sleep(BACKGROUND_CHECK_INTERVAL).await;
                Vec::new()
            }
            Err(_) => {
                self.link = Link::Finished;
                vec![ScreenInput::Failed(Error::ManagerUnreachable)]
            }
        }
    }

    fn lose_connection(&mut self) -> Vec<ScreenInput> {
        let at = monotonic_now();
        let tracker = &mut self.tracker;
        let RootSwitch::Queued(sysroot) = &tracker.root_switch else {
            if tracker.phase == Phase::Shutdown {
                self.link = Link::Finished;
                return vec![ScreenInput::Boot(vec![BootChange::ManagerExited { at }])];
            }
            self.link = Link::connecting(ConnectAttempt::Again);
            return vec![ScreenInput::Boot(vec![BootChange::ForgetJobs])];
        };
        if let Err(error) = sysroot.enter() {
            self.link = Link::Finished;
            return vec![ScreenInput::Failed(error.into())];
        }
        tracker.root_switch = RootSwitch::NotQueued;
        tracker.unit_paths.clear();
        tracker.phase = Phase::Host;
        self.link = Link::connecting(ConnectAttempt::Again);
        vec![ScreenInput::Boot(vec![BootChange::SwitchRoot { at }])]
    }
}

impl BootTracker {
    async fn receive(&mut self, connection: &mut BusConnection) -> Result<Vec<BootChange>, Error> {
        let signals = connection
            .receive_signals(BACKGROUND_CHECK_INTERVAL)
            .await?;
        Ok(self
            .collect_changes(connection, signals, async |tracker, connection, signal| {
                tracker.changes_for(connection, signal).await
            })
            .await)
    }

    async fn collect_changes<Item>(
        &mut self,
        connection: &mut BusConnection,
        items: Vec<Item>,
        step: impl AsyncFn(&mut Self, &mut BusConnection, Item) -> Vec<BootChange>,
    ) -> Vec<BootChange> {
        let step = &step;
        stream::iter(items)
            .fold(
                FeedProgress {
                    tracker: self,
                    connection,
                    changes: Vec::new(),
                },
                |mut progress, item| async move {
                    let changes = step(progress.tracker, progress.connection, item).await;
                    progress.changes.extend(changes);
                    progress
                },
            )
            .await
            .changes
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

    async fn boot_progress_changes(
        &self,
        connection: &mut BusConnection,
        attempt: ConnectAttempt,
    ) -> Result<Vec<BootChange>, Error> {
        let boot_in_progress = connection.boot_in_progress().await?;
        let origin = (attempt == ConnectAttempt::First).then(|| {
            BootChange::SetOrigin(if boot_in_progress {
                Duration::ZERO
            } else {
                monotonic_now()
            })
        });
        let finished = if boot_in_progress {
            Vec::new()
        } else {
            self.boot_finished()
        };
        Ok(origin.into_iter().chain(finished).collect())
    }

    async fn start_watching(
        &mut self,
        connection: &mut BusConnection,
        attempt: ConnectAttempt,
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
        let boot_progress = self.boot_progress_changes(connection, attempt).await?;
        let tracked_root = self.track(connection, &requested_root).await;
        let root = tracked_root
            .as_ref()
            .map_or(requested_root, |tracked| tracked.name().to_owned());
        let tracked_handoff = self.track(connection, HANDOFF_UNIT).await;
        let handoff = tracked_handoff
            .as_ref()
            .map(|tracked| BootChange::SetHandoff(tracked.name().to_owned()));
        let watched_units = jobs
            .iter()
            .map(|job| job.unit.clone())
            .chain(BootState::new(self.phase).milestone_targets)
            .collect();
        let tracked_changes = self
            .collect_changes(
                connection,
                watched_units,
                async |tracker, connection, unit| {
                    tracker
                        .track(connection, &unit)
                        .await
                        .and_then(TrackedUnit::into_change)
                        .into_iter()
                        .collect()
                },
            )
            .await;
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
        Ok(boot_progress
            .into_iter()
            .chain(tracked_root.and_then(TrackedUnit::into_change))
            .chain(tracked_handoff.and_then(TrackedUnit::into_change))
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
        if self.unit_paths.values().any(|known| known == unit) {
            return Some(TrackedUnit::Known(unit.to_owned()));
        }
        let FetchedUnit {
            path,
            facts,
            activity,
        } = connection.fetch_unit(unit).await.ok()?;
        if self.unit_paths.values().any(|known| *known == facts.name) {
            return Some(TrackedUnit::Known(facts.name));
        }
        self.unit_paths.insert(path, facts.name.clone());
        Some(TrackedUnit::New { facts, activity })
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
            .map_or(unit, |tracked| tracked.name().to_owned());
        if self.phase == Phase::Initrd
            && matches!(self.root_switch, RootSwitch::NotQueued)
            && unit == SWITCH_ROOT_UNIT
            && let Ok(sysroot) = Sysroot::hold()
        {
            self.root_switch = RootSwitch::Queued(sysroot);
        }
        tracked
            .and_then(TrackedUnit::into_change)
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
