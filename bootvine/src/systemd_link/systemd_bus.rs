use std::{
    future::{Future, pending, ready},
    io, iter, mem,
    os::fd::{AsFd, AsRawFd, BorrowedFd, RawFd},
    pin::pin,
    time::Duration,
};

use dbus::{
    Message, MessageType, Path,
    arg::{AppendAll, PropMap, RefArg, Variant},
    channel::{BusType, Channel},
};
use futures::{Stream, StreamExt, TryStreamExt, future::select, stream};
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use tokio::{
    io::unix::AsyncFd,
    signal::unix::{Signal as OsSignal, SignalKind, signal},
    time::timeout,
};

use crate::{
    boot_model::{Activity, JobKind, JobResult, UnitFacts, UnitKind, UnitTimeouts},
    console_session::stream_sources::{
        CarriedItem, readable_or_timeout, readable_source, unfold_carrying,
    },
    systemd_link::link_error::Error,
};

const DESTINATION: &str = "org.freedesktop.systemd1";
const MANAGER_PATH: &str = "/org/freedesktop/systemd1";
const MANAGER_INTERFACE: &str = "org.freedesktop.systemd1.Manager";
const UNIT_INTERFACE: &str = "org.freedesktop.systemd1.Unit";
const SERVICE_INTERFACE: &str = "org.freedesktop.systemd1.Service";
const MOUNT_INTERFACE: &str = "org.freedesktop.systemd1.Mount";
const SWAP_INTERFACE: &str = "org.freedesktop.systemd1.Swap";
const SOCKET_INTERFACE: &str = "org.freedesktop.systemd1.Socket";
const JOB_INTERFACE: &str = "org.freedesktop.systemd1.Job";
const PROPERTIES_INTERFACE: &str = "org.freedesktop.DBus.Properties";
const PEER_INTERFACE: &str = "org.freedesktop.DBus.Peer";
const BUS_DESTINATION: &str = "org.freedesktop.DBus";
const BUS_PATH: &str = "/org/freedesktop/DBus";
const PRIVATE_SOCKET: &str = "unix:path=/run/systemd/private";
const SIGNAL_RULE: &str = "type='signal',path_namespace='/org/freedesktop/systemd1'";
const CALL_TIMEOUT: Duration = Duration::from_secs(1);
const UNLIMITED: Duration = Duration::from_micros(u64::MAX);
const STATUS_SILENT_MODE: &str = "no";
const STATUS_RESET_MODE: &str = "";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    System,
    User,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusRelease {
    AfterBoot,
    Immediately,
}

pub enum SignalBatch {
    Open(Vec<Signal>),
    Closed(Vec<Signal>),
}

pub enum Signal {
    JobNew {
        id: u32,
        job: Path<'static>,
        unit: String,
    },
    JobRemoved {
        id: u32,
        unit: String,
        result: JobResult,
    },
    UnitChanged {
        path: Path<'static>,
        activity: Activity,
    },
    BootFinished,
}

pub struct FetchedUnit {
    pub path: Path<'static>,
    pub facts: UnitFacts,
    pub activity: Activity,
}

pub struct JobListing {
    pub id: u32,
    pub unit: String,
    pub kind: JobKind,
}

type ListedJob = (u32, String, String, String, Path<'static>, Path<'static>);

impl From<ListedJob> for JobListing {
    fn from((id, unit, kind, ..): ListedJob) -> Self {
        Self {
            id,
            unit,
            kind: serde_plain::from_str(&kind).unwrap_or(JobKind::Unknown),
        }
    }
}

pub struct Session {
    channel: Channel,
    peer_to_peer: bool,
}

fn method_call(
    destination: &str,
    path: &str,
    interface: &str,
    member: &str,
    arguments: impl AppendAll,
) -> Result<Message, Error> {
    let Ok(mut call) = Message::new_method_call(destination, path, interface, member) else {
        return Err(Error::MalformedCall);
    };
    call.append_all(arguments);
    Ok(call)
}

fn manager_call(member: &str, arguments: impl AppendAll) -> Result<Message, Error> {
    method_call(
        DESTINATION,
        MANAGER_PATH,
        MANAGER_INTERFACE,
        member,
        arguments,
    )
}

fn property_call(path: &str, interface: &str, property: &str) -> Result<Message, Error> {
    method_call(
        DESTINATION,
        path,
        PROPERTIES_INTERFACE,
        "Get",
        (interface, property),
    )
}

fn boot_in_progress_call() -> Result<Message, Error> {
    property_call(MANAGER_PATH, MANAGER_INTERFACE, "FinishTimestampMonotonic")
}

fn read_boot_in_progress(reply: &Message) -> Result<bool, Error> {
    let finished: Variant<u64> = reply.read1()?;
    Ok(finished.0 == 0)
}

async fn boot_finish_or_lost_connection() -> Result<(), Error> {
    let mut connection = BusConnection::register(Session::open(Scope::System)?)?;
    connection.subscribe().await?;
    if connection.boot_in_progress().await? {
        connection
            .into_signals()
            .any(|signal| ready(matches!(signal, Signal::BootFinished)))
            .await;
    }
    Ok(())
}

async fn termination_request(listener: io::Result<OsSignal>) {
    match listener {
        Ok(mut listener) => {
            listener.recv().await;
        }
        Err(_) => pending().await,
    }
}

pub async fn restore_status_after_watch(
    watch: impl Future<Output = Result<StatusRelease, Error>>,
) -> Result<(), Error> {
    let terminated = termination_request(signal(SignalKind::terminate()));
    let watched = watch.await;
    if !matches!(watched, Ok(StatusRelease::Immediately)) {
        select(pin!(terminated), pin!(boot_finish_or_lost_connection())).await;
    }
    if let Ok(session) = Session::open(Scope::System) {
        session.restore_configured_status().ok();
    }
    watched.map(drop)
}

impl Session {
    pub fn open(scope: Scope) -> Result<Self, Error> {
        if scope == Scope::System
            && let Ok(channel) = Channel::open_private(PRIVATE_SOCKET)
        {
            let session = Self::watching(channel, true);
            session.send(authentication_carrier()?)?;
            return Ok(session);
        }
        let bus = match scope {
            Scope::System => BusType::System,
            Scope::User => BusType::Session,
        };
        let session = Self::watching(Channel::get_private(bus)?, false);
        session.blocking_call(method_call(
            BUS_DESTINATION,
            BUS_PATH,
            BUS_DESTINATION,
            "AddMatch",
            (SIGNAL_RULE,),
        )?)?;
        Ok(session)
    }

    fn watching(mut channel: Channel, peer_to_peer: bool) -> Self {
        channel.set_watch_enabled(true);
        Self {
            channel,
            peer_to_peer,
        }
    }

    fn blocking_call(&self, call: Message) -> Result<Message, Error> {
        Ok(self.channel.send_with_reply_and_block(call, CALL_TIMEOUT)?)
    }

    fn restore_configured_status(&self) -> Result<(), Error> {
        self.blocking_call(manager_call("SetShowStatus", (STATUS_RESET_MODE,))?)?;
        Ok(())
    }

    pub fn boot_in_progress(&self) -> Result<bool, Error> {
        read_boot_in_progress(&self.blocking_call(boot_in_progress_call()?)?)
    }

    fn send(&self, call: Message) -> Result<u32, Error> {
        let serial = self.channel.send(call).map_err(|()| Error::Disconnected)?;
        self.channel.flush();
        Ok(serial)
    }

    fn socket_has_unread_bytes(&self) -> bool {
        let mut watched = [PollFd::new(self, PollFlags::IN)];
        poll(&mut watched, Some(&Timespec::default())).is_ok_and(|ready| ready > 0)
    }

    fn read_one_chunk(&self) -> bool {
        self.channel.read_write(Some(Duration::ZERO)).is_ok() && self.channel.is_connected()
    }

    fn arrived_messages(&self) -> Vec<Message> {
        iter::repeat_with(|| self.read_one_chunk())
            .find(|still_connected| !still_connected || !self.socket_has_unread_bytes());
        iter::from_fn(|| self.channel.pop_message()).collect()
    }
}

pub struct BusConnection {
    session: AsyncFd<Session>,
    early_signals: Vec<Signal>,
}

impl BusConnection {
    pub fn register(session: Session) -> io::Result<Self> {
        Ok(Self {
            session: readable_source(session)?,
            early_signals: Vec::new(),
        })
    }

    #[must_use]
    pub fn peer_to_peer(&self) -> bool {
        self.session.get_ref().peer_to_peer
    }

    fn drain_replies(&mut self) -> Result<Vec<Message>, Error> {
        let session = self.session.get_ref();
        let (signals, replies): (Vec<Message>, Vec<Message>) = session
            .arrived_messages()
            .into_iter()
            .partition(|message| message.msg_type() == MessageType::Signal);
        self.early_signals
            .extend(signals.iter().filter_map(parse_signal));
        if session.channel.is_connected() {
            Ok(replies)
        } else {
            Err(Error::Disconnected)
        }
    }

    async fn next_replies(&mut self) -> Result<Vec<Message>, Error> {
        let drained = self.drain_replies()?;
        if !drained.is_empty() {
            return Ok(drained);
        }
        self.session.readable().await?.clear_ready();
        self.drain_replies()
    }

    async fn reply_to(&mut self, serial: u32) -> Result<Message, Error> {
        let mut replies = pin!(
            unfold_carrying(self, |connection| async move {
                let replies = connection.next_replies().await;
                Some(CarriedItem {
                    item: replies,
                    state: connection,
                })
            })
            .map_ok(|replies| stream::iter(replies.into_iter().map(Ok::<_, Error>)))
            .try_flatten()
            .try_filter(|reply| ready(reply.get_reply_serial() == Some(serial)))
        );
        replies.try_next().await?.ok_or(Error::Disconnected)
    }

    async fn call(&mut self, call: Message) -> Result<Message, Error> {
        let serial = self.session.get_ref().send(call)?;
        let Ok(reply) = timeout(CALL_TIMEOUT, self.reply_to(serial)).await else {
            return Err(Error::CallTimedOut);
        };
        let mut reply = reply?;
        reply.as_result()?;
        Ok(reply)
    }

    pub async fn subscribe(&mut self) -> Result<(), Error> {
        self.call(manager_call("Subscribe", ())?).await?;
        Ok(())
    }

    pub async fn silence_status_output(&mut self) -> Result<(), Error> {
        self.call(manager_call("SetShowStatus", (STATUS_SILENT_MODE,))?)
            .await?;
        Ok(())
    }

    pub async fn default_target(&mut self) -> Result<String, Error> {
        let reply = self.call(manager_call("GetDefaultTarget", ())?).await?;
        Ok(reply.read1()?)
    }

    pub async fn boot_in_progress(&mut self) -> Result<bool, Error> {
        read_boot_in_progress(&self.call(boot_in_progress_call()?).await?)
    }

    pub async fn list_jobs(&mut self) -> Result<Vec<JobListing>, Error> {
        let reply = self.call(manager_call("ListJobs", ())?).await?;
        let listed: Vec<ListedJob> = reply.read1()?;
        Ok(listed.into_iter().map(JobListing::from).collect())
    }

    pub async fn job_kind(&mut self, job: &Path<'_>) -> Result<JobKind, Error> {
        let reply = self
            .call(property_call(job, JOB_INTERFACE, "JobType")?)
            .await?;
        let kind: Variant<String> = reply.read1()?;
        Ok(serde_plain::from_str(&kind.0).unwrap_or(JobKind::Unknown))
    }

    pub async fn fetch_unit(&mut self, name: &str) -> Result<FetchedUnit, Error> {
        let path: Path<'static> = self
            .call(manager_call("GetUnit", (name,))?)
            .await?
            .read1()?;
        let properties = self.all_properties(&path, UNIT_INTERFACE).await?;
        let kind_properties = match kind_interface(UnitKind::of(&unit_id(name, &properties))) {
            Some(interface) => self.all_properties(&path, interface).await?,
            None => PropMap::new(),
        };
        Ok(FetchedUnit {
            facts: unit_facts_from(name, &properties, &kind_properties),
            activity: activity_from(&properties),
            path,
        })
    }

    pub async fn failure_reason(&mut self, name: &str) -> Result<Option<String>, Error> {
        let Some(interface) = kind_interface(UnitKind::of(name)) else {
            return Ok(None);
        };
        let path: Path<'static> = self
            .call(manager_call("GetUnit", (name,))?)
            .await?
            .read1()?;
        let properties = self.all_properties(&path, interface).await?;
        Ok(failure_reason_from(&properties))
    }

    async fn all_properties(&mut self, path: &Path<'_>, interface: &str) -> Result<PropMap, Error> {
        Ok(self
            .call(method_call(
                DESTINATION,
                path,
                PROPERTIES_INTERFACE,
                "GetAll",
                (interface,),
            )?)
            .await?
            .read1()?)
    }

    pub async fn receive_signals(&mut self, patience: Duration) -> SignalBatch {
        if self.early_signals.is_empty() {
            readable_or_timeout(&self.session, patience).await;
        }
        self.take_signals()
    }

    fn take_signals(&mut self) -> SignalBatch {
        let still_connected = self.drain_replies().is_ok();
        let signals = mem::take(&mut self.early_signals);
        if still_connected {
            SignalBatch::Open(signals)
        } else {
            SignalBatch::Closed(signals)
        }
    }

    async fn next_signals(&mut self) -> SignalBatch {
        if self.early_signals.is_empty() {
            let Ok(mut readiness) = self.session.readable().await else {
                return SignalBatch::Closed(Vec::new());
            };
            readiness.clear_ready();
        }
        self.take_signals()
    }

    fn into_signals(self) -> impl Stream<Item = Signal> {
        unfold_carrying(self, |mut connection| async move {
            match connection.next_signals().await {
                SignalBatch::Open(signals) => Some(CarriedItem {
                    item: signals,
                    state: connection,
                }),
                SignalBatch::Closed(_) => None,
            }
        })
        .flat_map(stream::iter)
    }
}

fn text_property(properties: &PropMap, property: &str) -> Option<String> {
    properties
        .get(property)
        .and_then(|value| value.0.as_str())
        .map(ToOwned::to_owned)
}

fn signal_name(number: i64) -> String {
    let name = match number {
        1 => "SIGHUP",
        2 => "SIGINT",
        3 => "SIGQUIT",
        4 => "SIGILL",
        5 => "SIGTRAP",
        6 => "SIGABRT",
        7 => "SIGBUS",
        8 => "SIGFPE",
        9 => "SIGKILL",
        11 => "SIGSEGV",
        13 => "SIGPIPE",
        14 => "SIGALRM",
        15 => "SIGTERM",
        other => return format!("signal {other}"),
    };
    name.to_owned()
}

fn failure_reason_from(properties: &PropMap) -> Option<String> {
    let result = text_property(properties, "Result").filter(|result| result != "success")?;
    let status = properties
        .get("ExecMainStatus")
        .and_then(|value| value.0.as_i64());
    let reason = match (result.as_str(), status) {
        ("exit-code", Some(status)) if status != 0 => format!("exited with status {status}"),
        ("exit-code", _) => "exited with an error".to_owned(),
        ("signal", Some(signal)) => format!("killed by {}", signal_name(signal)),
        ("core-dump", Some(signal)) => format!("dumped core on {}", signal_name(signal)),
        ("timeout", _) => "timed out".to_owned(),
        ("watchdog", _) => "stopped answering the watchdog".to_owned(),
        ("start-limit-hit", _) => "restarted too often".to_owned(),
        ("oom-kill", _) => "killed for running out of memory".to_owned(),
        ("resources", _) => "could not set up its resources".to_owned(),
        ("protocol", _) => "broke the start-up protocol".to_owned(),
        (other, _) => other.to_owned(),
    };
    Some(
        match text_property(properties, "StatusText").filter(|text| !text.is_empty()) {
            Some(status_text) => format!("{reason}: {status_text}"),
            None => reason,
        },
    )
}

fn unit_id(name: &str, properties: &PropMap) -> String {
    text_property(properties, "Id").unwrap_or_else(|| name.to_owned())
}

fn kind_interface(kind: UnitKind) -> Option<&'static str> {
    match kind {
        UnitKind::Service => Some(SERVICE_INTERFACE),
        UnitKind::Mount => Some(MOUNT_INTERFACE),
        UnitKind::Swap => Some(SWAP_INTERFACE),
        UnitKind::Socket => Some(SOCKET_INTERFACE),
        _ => None,
    }
}

fn time_limit(properties: &PropMap, property: &str) -> Option<Duration> {
    properties
        .get(property)
        .and_then(|value| value.0.as_u64())
        .map(Duration::from_micros)
        .filter(|limit| !limit.is_zero() && *limit != UNLIMITED)
}

fn timeouts_from(kind: UnitKind, unit: &PropMap, kind_specific: &PropMap) -> UnitTimeouts {
    let (start, stop) = if kind == UnitKind::Service {
        ("TimeoutStartUSec", "TimeoutStopUSec")
    } else {
        ("TimeoutUSec", "TimeoutUSec")
    };
    UnitTimeouts {
        start: time_limit(kind_specific, start),
        stop: time_limit(kind_specific, stop),
        job: time_limit(unit, "JobTimeoutUSec"),
        job_running: time_limit(unit, "JobRunningTimeoutUSec"),
    }
}

fn unit_facts_from(name: &str, properties: &PropMap, kind_properties: &PropMap) -> UnitFacts {
    let text = |property: &str| text_property(properties, property);
    let names = |property: &str| {
        properties
            .get(property)
            .and_then(|value| value.0.as_iter())
            .map(|items| {
                items
                    .filter_map(|item| item.as_str().map(ToOwned::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    };
    let id = unit_id(name, properties);
    UnitFacts {
        timeouts: timeouts_from(UnitKind::of(&id), properties, kind_properties),
        name: id,
        description: text("Description").filter(|description| !description.is_empty()),
        after: names("After"),
        before: names("Before"),
    }
}

impl AsRawFd for Session {
    fn as_raw_fd(&self) -> RawFd {
        self.channel.watch().fd
    }
}

impl AsFd for Session {
    fn as_fd(&self) -> BorrowedFd<'_> {
        unsafe { BorrowedFd::borrow_raw(self.as_raw_fd()) }
    }
}

fn authentication_carrier() -> Result<Message, Error> {
    let mut ping = method_call(DESTINATION, MANAGER_PATH, PEER_INTERFACE, "Ping", ())?;
    ping.set_no_reply(true);
    Ok(ping)
}

fn parse_signal(message: &Message) -> Option<Signal> {
    if message.msg_type() != MessageType::Signal {
        return None;
    }
    let interface = message.interface()?;
    let member = message.member()?;
    match (&*interface, &*member) {
        (MANAGER_INTERFACE, "JobNew" | "JobRemoved") => parse_job_signal(message, &member),
        (MANAGER_INTERFACE, "StartupFinished") => Some(Signal::BootFinished),
        (PROPERTIES_INTERFACE, "PropertiesChanged") => parse_unit_change(message),
        _ => None,
    }
}

fn parse_job_signal(message: &Message, member: &str) -> Option<Signal> {
    match member {
        "JobNew" => {
            let (id, job, unit): (u32, Path<'static>, String) = message.read3().ok()?;
            Some(Signal::JobNew { id, job, unit })
        }
        "JobRemoved" => {
            let (id, _job, unit, result): (u32, Path<'static>, String, String) =
                message.read4().ok()?;
            Some(Signal::JobRemoved {
                id,
                unit,
                result: serde_plain::from_str(&result).unwrap_or(JobResult::Failed),
            })
        }
        _ => None,
    }
}

fn parse_unit_change(message: &Message) -> Option<Signal> {
    let (changed_interface, changed, _invalidated): (String, PropMap, Vec<String>) =
        message.read3().ok()?;
    if changed_interface != UNIT_INTERFACE {
        return None;
    }
    Some(Signal::UnitChanged {
        path: message.path()?.into_static(),
        activity: activity_from(&changed),
    })
}

fn activity_from(properties: &PropMap) -> Activity {
    let monotonic = |property: &str| {
        properties
            .get(property)
            .and_then(|value| value.0.as_u64())
            .filter(|micros| *micros != 0)
            .map(Duration::from_micros)
    };
    Activity {
        state: properties
            .get("ActiveState")
            .and_then(|value| value.0.as_str())
            .and_then(|raw| serde_plain::from_str(raw).ok()),
        inactive_exit: monotonic("InactiveExitTimestampMonotonic"),
        active_enter: monotonic("ActiveEnterTimestampMonotonic"),
        active_exit: monotonic("ActiveExitTimestampMonotonic"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_finished_signal_marks_the_boot_finished() {
        let startup_finished = Message::signal(
            &Path::from(MANAGER_PATH),
            &MANAGER_INTERFACE.into(),
            &"StartupFinished".into(),
        )
        .append3(0_u64, 0_u64, 0_u64)
        .append3(0_u64, 0_u64, 0_u64);
        assert!(matches!(
            parse_signal(&startup_finished),
            Some(Signal::BootFinished)
        ));
    }

    fn limit_properties(entries: &[(&str, Duration)]) -> PropMap {
        entries
            .iter()
            .map(|(name, limit)| {
                let micros = u64::try_from(limit.as_micros()).unwrap_or(u64::MAX);
                (
                    (*name).to_owned(),
                    Variant(Box::new(micros) as Box<dyn RefArg>),
                )
            })
            .collect()
    }

    #[test]
    fn unlimited_timeouts_are_absent() {
        let unit = limit_properties(&[
            ("JobTimeoutUSec", UNLIMITED),
            ("JobRunningTimeoutUSec", Duration::ZERO),
        ]);
        let service = limit_properties(&[
            ("TimeoutStartUSec", UNLIMITED),
            ("TimeoutStopUSec", Duration::from_secs(90)),
        ]);
        assert_eq!(
            timeouts_from(UnitKind::Service, &unit, &service),
            UnitTimeouts {
                stop: Some(Duration::from_secs(90)),
                ..UnitTimeouts::default()
            }
        );
    }

    fn reason_properties(result: &str, status: i32, status_text: &str) -> PropMap {
        [
            ("Result", Box::new(result.to_owned()) as Box<dyn RefArg>),
            ("ExecMainStatus", Box::new(status)),
            ("StatusText", Box::new(status_text.to_owned())),
        ]
        .into_iter()
        .map(|(name, value)| (name.to_owned(), Variant(value)))
        .collect()
    }

    #[test]
    fn exit_codes_and_signals_become_a_reason() {
        assert_eq!(
            failure_reason_from(&reason_properties("exit-code", 1, "")).as_deref(),
            Some("exited with status 1")
        );
        assert_eq!(
            failure_reason_from(&reason_properties("signal", 11, "")).as_deref(),
            Some("killed by SIGSEGV")
        );
    }

    #[test]
    fn status_text_follows_the_reason() {
        assert_eq!(
            failure_reason_from(&reason_properties("timeout", 0, "waiting for the disk"))
                .as_deref(),
            Some("timed out: waiting for the disk")
        );
    }

    #[test]
    fn success_has_no_reason() {
        assert_eq!(
            failure_reason_from(&reason_properties("success", 0, "")),
            None
        );
    }

    #[test]
    fn mounts_use_one_timeout_for_both_directions() {
        let mount = limit_properties(&[("TimeoutUSec", Duration::from_secs(20))]);
        let timeouts = timeouts_from(UnitKind::Mount, &PropMap::new(), &mount);
        assert_eq!(timeouts.start, Some(Duration::from_secs(20)));
        assert_eq!(timeouts.stop, Some(Duration::from_secs(20)));
    }
}
