use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActiveState {
    Inactive,
    Activating,
    Active,
    Deactivating,
    Reloading,
    Failed,
}

impl ActiveState {
    #[must_use]
    pub fn parse(raw: &str) -> Self {
        match raw {
            "activating" => Self::Activating,
            "active" => Self::Active,
            "deactivating" => Self::Deactivating,
            "reloading" => Self::Reloading,
            "failed" => Self::Failed,
            _ => Self::Inactive,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobResult {
    Done,
    Skipped,
    Canceled,
    Failed,
    Timeout,
    Dependency,
}

impl JobResult {
    #[must_use]
    pub fn parse(raw: &str) -> Self {
        match raw {
            "done" => Self::Done,
            "skipped" => Self::Skipped,
            "canceled" => Self::Canceled,
            "timeout" => Self::Timeout,
            "dependency" => Self::Dependency,
            _ => Self::Failed,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitFacts {
    pub name: String,
    pub description: String,
    pub after: Vec<String>,
    pub before: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Activity {
    pub state: Option<ActiveState>,
    pub inactive_exit: Option<Duration>,
    pub active_enter: Option<Duration>,
    pub active_exit: Option<Duration>,
}

impl Activity {
    pub(super) fn merge(self, newer: Self) -> Self {
        Self {
            state: newer.state.or(self.state),
            inactive_exit: newer.inactive_exit.or(self.inactive_exit),
            active_enter: newer.active_enter.or(self.active_enter),
            active_exit: newer.active_exit.or(self.active_exit),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobKind {
    BringsUp,
    TakesDown,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitKind {
    Service,
    Target,
    Socket,
    Mount,
    Automount,
    Device,
    Swap,
    Path,
    Timer,
    Slice,
    Scope,
    Other,
}

impl UnitKind {
    #[must_use]
    pub fn of(name: &str) -> Self {
        match name.rsplit_once('.').map(|(_, suffix)| suffix) {
            Some("service") => Self::Service,
            Some("target") => Self::Target,
            Some("socket") => Self::Socket,
            Some("mount") => Self::Mount,
            Some("automount") => Self::Automount,
            Some("device") => Self::Device,
            Some("swap") => Self::Swap,
            Some("path") => Self::Path,
            Some("timer") => Self::Timer,
            Some("slice") => Self::Slice,
            Some("scope") => Self::Scope,
            _ => Self::Other,
        }
    }

    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Service | Self::Target | Self::Other => "",
            Self::Socket => "Socket",
            Self::Mount => "Mount",
            Self::Automount => "Automount",
            Self::Device => "Device",
            Self::Swap => "Swap",
            Self::Path => "Path",
            Self::Timer => "Timer",
            Self::Slice => "Slice",
            Self::Scope => "Scope",
        }
    }

    fn labelled_by_description(self) -> bool {
        matches!(
            self,
            Self::Device | Self::Mount | Self::Swap | Self::Automount
        )
    }
}

fn labelled_by_description(facts: &UnitFacts) -> bool {
    !facts.description.is_empty() && UnitKind::of(&facts.name).labelled_by_description()
}

pub(super) fn display_label(facts: &UnitFacts) -> String {
    if labelled_by_description(facts) {
        return facts.description.clone();
    }
    match UnitKind::of(&facts.name) {
        UnitKind::Target | UnitKind::Other => facts.name.clone(),
        _ => facts
            .name
            .rsplit_once('.')
            .map_or_else(|| facts.name.clone(), |(stem, _)| stem.to_owned()),
    }
}

pub(super) fn summary(facts: &UnitFacts) -> String {
    let repeats_the_name = facts.description == facts.name
        || facts
            .description
            .eq_ignore_ascii_case(&display_label(facts));
    if labelled_by_description(facts) || repeats_the_name {
        return String::new();
    }
    facts.description.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::boot_test_fixtures::{facts, waiting};

    #[test]
    fn kinds_with_a_tag_drop_their_suffix() {
        let socket = facts("docker.socket", &[]);
        assert_eq!(UnitKind::of(&socket.name), UnitKind::Socket);
        assert_eq!(UnitKind::Socket.tag(), "Socket");
        assert_eq!(display_label(&socket), "docker");
        assert_eq!(display_label(&facts("sshd.service", &[])), "sshd");
        assert_eq!(display_label(&facts("basic.target", &[])), "basic.target");
        assert_eq!(UnitKind::Target.tag(), "");
    }

    #[test]
    fn descriptions_that_repeat_the_label_are_dropped() {
        let described = |name: &str, description: &str| {
            summary(&waiting(name, &[]).described(description).facts)
        };
        assert_eq!(
            described("multi-user.target", "Multi-User System"),
            "Multi-User System"
        );
        assert_eq!(described("greetd.service", "greetd.service"), "");
        assert_eq!(described("greetd.service", "Greetd"), "");
        assert_eq!(described("dev-sda.device", "Samsung SSD"), "");
        assert_eq!(described("demo.service", ""), "");
    }
}
