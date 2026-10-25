use std::{future::Future, io, iter, os::fd::AsRawFd, time::Duration};

use crossterm::event::{self, Event};
use futures::{FutureExt, Stream, StreamExt, stream};
use tokio::{
    io::{Interest, unix::AsyncFd},
    signal::unix::{SignalKind, signal},
    time::{Interval, MissedTickBehavior, interval, timeout},
};

use crate::console_session::watch_screen::ScreenInput;

pub const FRAME_INTERVAL: Duration = Duration::from_millis(16);
pub const BACKGROUND_CHECK_INTERVAL: Duration = Duration::from_millis(100);

pub struct CarriedItem<Item, State> {
    pub item: Item,
    pub state: State,
}

pub fn unfold_carrying<State, Item, Next, Pending>(
    initial: State,
    mut next: Next,
) -> impl Stream<Item = Item>
where
    Next: FnMut(State) -> Pending,
    Pending: Future<Output = Option<CarriedItem<Item, State>>>,
{
    stream::unfold(initial, move |state| {
        next(state).map(|carried| carried.map(|carried| (carried.item, carried.state)))
    })
}

pub fn readable_source<Source: AsRawFd>(source: Source) -> io::Result<AsyncFd<Source>> {
    Ok(unsafe { AsyncFd::register_with_interest(source, Interest::READABLE) }?)
}

pub fn items_when_readable<Source: AsRawFd, Item>(
    source: Source,
    read: impl Fn(&Source) -> Item,
) -> io::Result<impl Stream<Item = Item>> {
    let readiness = readable_source(source)?;
    Ok(stream::poll_fn(move |context| {
        readiness.poll_read_ready(context).map(|ready| {
            ready.ok().map(|mut guard| {
                guard.clear_ready();
                read(readiness.get_ref())
            })
        })
    }))
}

pub async fn readable_or_timeout<Source: AsRawFd>(readiness: &AsyncFd<Source>, patience: Duration) {
    if let Ok(Ok(mut guard)) = timeout(patience, readiness.readable()).await {
        guard.clear_ready();
    }
}

fn steady_interval(period: Duration) -> Interval {
    let mut ticks = interval(period);
    ticks.set_missed_tick_behavior(MissedTickBehavior::Delay);
    ticks
}

pub fn periodic_inputs(
    period: Duration,
    input: fn() -> ScreenInput,
) -> impl Stream<Item = ScreenInput> {
    let mut ticks = steady_interval(period);
    stream::poll_fn(move |context| ticks.poll_tick(context).map(|_| Some(input())))
}

pub fn frame_inputs() -> impl Stream<Item = ScreenInput> {
    periodic_inputs(FRAME_INTERVAL, || ScreenInput::Frame)
}

fn drain_terminal_events() -> io::Result<Vec<Event>> {
    iter::from_fn(|| match event::poll(Duration::ZERO) {
        Ok(true) => Some(event::read()),
        Ok(false) => None,
        Err(error) => Some(Err(error)),
    })
    .collect()
}

fn terminal_inputs(drained: io::Result<Vec<Event>>) -> Vec<ScreenInput> {
    match drained {
        Ok(events) => events.into_iter().map(ScreenInput::Key).collect(),
        Err(error) => vec![ScreenInput::Failed(error.into())],
    }
}

pub fn key_inputs() -> io::Result<impl Stream<Item = ScreenInput>> {
    Ok(
        items_when_readable(io::stdin(), |_| drain_terminal_events())?
            .map(terminal_inputs)
            .flat_map(stream::iter),
    )
}

pub fn termination_inputs() -> io::Result<impl Stream<Item = ScreenInput>> {
    let mut terminate = signal(SignalKind::terminate())?;
    Ok(stream::once(async move {
        terminate.recv().await;
        ScreenInput::Terminate
    }))
}
