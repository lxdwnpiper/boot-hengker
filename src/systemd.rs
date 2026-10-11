use std::{iter, time::Duration};

use bootvine::model::{ActiveState, Activity, JobKind, JobResult, UnitFacts};
use dbus::{
    Message, MessageType, Path,
    arg::{AppendAll, PropMap, RefArg, Variant},
    channel::{BusType, Channel},
};

use crate::error::Error;

const DESTINATION: &str = "org.freedesktop.systemd1";
const MANAGER_PATH: &str = "/org/freedesktop/systemd1";
const MANAGER_INTERFACE: &str = "org.freedesktop.systemd1.Manager";
const UNIT_INTERFACE: &str = "org.freedesktop.systemd1.Unit";
const JOB_INTERFACE: &str = "org.freedesktop.systemd1.Job";
const PROPERTIES_INTERFACE: &str = "org.freedesktop.DBus.Properties";
const PEER_INTERFACE: &str = "org.freedesktop.DBus.Peer";
const BUS_DESTINATION: &str = "org.freedesktop.DBus";
const BUS_PATH: &str = "/org/freedesktop/DBus";
const PRIVATE_SOCKET: &str = "unix:path=/run/systemd/private";
const SIGNAL_RULE: &str = "type='signal',path_namespace='/org/freedesktop/systemd1'";
const STOP_JOB: &str = "stop";
const CALL_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    System,
    User,
}

pub enum Signal {
    JobNew {
        id: u32,
        job: Path<'static>,
        unit: String,
    },
    JobRemoved {
        id: u32,
        result: JobResult,
    },
    UnitChanged {
        path: Path<'static>,
        activity: Activity,
    },
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
            kind: job_kind(&kind),
        }
    }
}

fn job_kind(raw: &str) -> JobKind {
    if raw == STOP_JOB {
        JobKind::TakesDown
    } else {
        JobKind::BringsUp
    }
}

pub struct Session {
    channel: Channel,
    pub peer_to_peer: bool,
}

impl Session {
    pub fn open(scope: Scope) -> Result<Self, Error> {
        if scope == Scope::System
            && let Ok(channel) = Channel::open_private(PRIVATE_SOCKET)
        {
            send_authentication_carrier(&channel)?;
            return Ok(Self {
                channel,
                peer_to_peer: true,
            });
        }
        let bus = match scope {
            Scope::System => BusType::System,
            Scope::User => BusType::Session,
        };
        let session = Self {
            channel: Channel::get_private(bus)?,
            peer_to_peer: false,
        };
        session.call(
            BUS_DESTINATION,
            BUS_PATH,
            BUS_DESTINATION,
            "AddMatch",
            (SIGNAL_RULE,),
        )?;
        Ok(session)
    }

    fn call(
        &self,
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
        Ok(self.channel.send_with_reply_and_block(call, CALL_TIMEOUT)?)
    }

    pub fn subscribe(&self) -> Result<(), Error> {
        self.call(
            DESTINATION,
            MANAGER_PATH,
            MANAGER_INTERFACE,
            "Subscribe",
            (),
        )?;
        Ok(())
    }

    pub fn set_show_status(&self, mode: &str) -> Result<(), Error> {
        self.call(
            DESTINATION,
            MANAGER_PATH,
            MANAGER_INTERFACE,
            "SetShowStatus",
            (mode,),
        )?;
        Ok(())
    }

    pub fn default_target(&self) -> Result<String, Error> {
        let reply = self.call(
            DESTINATION,
            MANAGER_PATH,
            MANAGER_INTERFACE,
            "GetDefaultTarget",
            (),
        )?;
        Ok(reply.read1()?)
    }

    pub fn boot_in_progress(&self) -> Result<bool, Error> {
        let reply = self.call(
            DESTINATION,
            MANAGER_PATH,
            PROPERTIES_INTERFACE,
            "Get",
            (MANAGER_INTERFACE, "FinishTimestampMonotonic"),
        )?;
        let finished: Variant<u64> = reply.read1()?;
        Ok(finished.0 == 0)
    }

    pub fn list_jobs(&self) -> Result<Vec<JobListing>, Error> {
        let reply = self.call(DESTINATION, MANAGER_PATH, MANAGER_INTERFACE, "ListJobs", ())?;
        let listed: Vec<ListedJob> = reply.read1()?;
        Ok(listed.into_iter().map(JobListing::from).collect())
    }

    pub fn job_kind(&self, job: &Path) -> Result<JobKind, Error> {
        let reply = self.call(
            DESTINATION,
            job,
            PROPERTIES_INTERFACE,
            "Get",
            (JOB_INTERFACE, "JobType"),
        )?;
        let kind: Variant<String> = reply.read1()?;
        Ok(job_kind(&kind.0))
    }

    pub fn fetch_unit(&self, name: &str) -> Result<FetchedUnit, Error> {
        let path: Path<'static> = self
            .call(
                DESTINATION,
                MANAGER_PATH,
                MANAGER_INTERFACE,
                "GetUnit",
                (name,),
            )?
            .read1()?;
        let properties: PropMap = self
            .call(
                DESTINATION,
                &path,
                PROPERTIES_INTERFACE,
                "GetAll",
                (UNIT_INTERFACE,),
            )?
            .read1()?;
        let text = |property: &str| {
            properties
                .get(property)
                .and_then(|value| value.0.as_str())
                .map(ToOwned::to_owned)
        };
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
        Ok(FetchedUnit {
            facts: UnitFacts {
                name: text("Id").unwrap_or_else(|| name.to_owned()),
                description: text("Description").unwrap_or_default(),
                after: names("After"),
                before: names("Before"),
            },
            activity: activity_from(&properties),
            path,
        })
    }

    pub fn receive(&self, timeout: Duration) -> Result<Vec<Signal>, Error> {
        let drain = || -> Vec<Signal> {
            iter::from_fn(|| self.channel.pop_message())
                .filter_map(|message| parse_signal(&message))
                .collect()
        };
        let queued = drain();
        if !queued.is_empty() {
            return Ok(queued);
        }
        if self.channel.read_write(Some(timeout)).is_err() || !self.channel.is_connected() {
            return Err(Error::Disconnected);
        }
        Ok(drain())
    }
}

fn send_authentication_carrier(channel: &Channel) -> Result<(), Error> {
    let Ok(mut ping) = Message::new_method_call(DESTINATION, MANAGER_PATH, PEER_INTERFACE, "Ping")
    else {
        return Err(Error::MalformedCall);
    };
    ping.set_no_reply(true);
    if channel.send(ping).is_err() {
        return Err(Error::Disconnected);
    }
    channel.flush();
    Ok(())
}

fn parse_signal(message: &Message) -> Option<Signal> {
    if message.msg_type() != MessageType::Signal {
        return None;
    }
    let interface = message.interface()?;
    let member = message.member()?;
    match &*interface {
        MANAGER_INTERFACE => parse_job_signal(message, &member),
        PROPERTIES_INTERFACE if &*member == "PropertiesChanged" => parse_unit_change(message),
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
            let (id, _job, _unit, result): (u32, Path<'static>, String, String) =
                message.read4().ok()?;
            Some(Signal::JobRemoved {
                id,
                result: JobResult::parse(&result),
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
            .map(ActiveState::parse),
        inactive_exit: monotonic("InactiveExitTimestampMonotonic"),
        active_enter: monotonic("ActiveEnterTimestampMonotonic"),
        active_exit: monotonic("ActiveExitTimestampMonotonic"),
    }
}
