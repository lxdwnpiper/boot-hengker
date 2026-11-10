use std::{
    hint::black_box,
    iter::once,
    time::{Duration, Instant},
};

use ratatui::{Terminal, backend::TestBackend};

use super::{BootProjection, BootView, project_boot, screen_render_fixtures::ryzen_origin};
use crate::{
    boot_model::{BootState, JobResult, Phase, flatten_forest},
    boot_test_fixtures::{MS, QueuedUnit, queued_boot, running, waiting},
    screen_settings::{color_scheme::Colors, config_file::Timing, font_glyphs::Glyphs},
};

const SERVICE_COUNT: u32 = 200;
const CHAIN_LENGTH: u32 = 5;
const FINISHED_SERVICES: u32 = 40;
const DRAW_COUNT: u32 = 200;
const DRAW_SPACING: Duration = Duration::from_millis(16);

fn service_name(index: u32) -> String {
    format!("unit{index:03}.service")
}

fn crowded_service(index: u32, names: &[String]) -> QueuedUnit {
    let earlier = (!index.is_multiple_of(CHAIN_LENGTH)).then(|| names[index as usize - 1].as_str());
    let after: Vec<&str> = earlier.into_iter().collect();
    let name = &names[index as usize];
    if index.is_multiple_of(3) {
        running(name, &after, MS * index)
    } else {
        waiting(name, &after)
    }
}

fn crowded_boot() -> BootState {
    let names: Vec<String> = (0..SERVICE_COUNT).map(service_name).collect();
    let services: Vec<&str> = names.iter().map(String::as_str).collect();
    let mut state = queued_boot(
        Phase::Host,
        Duration::ZERO,
        once(waiting("crowded.target", &services))
            .chain((0..SERVICE_COUNT).map(|index| crowded_service(index, &names))),
    );
    (2..FINISHED_SERVICES + 2).for_each(|id| {
        state.finish_job(id, JobResult::Done, MS * 1500 * id);
    });
    state
}

fn report_timed_draws(label: &str, mut work: impl FnMut(Duration)) {
    let mut timings: Vec<Duration> = (0..DRAW_COUNT)
        .map(|draw| {
            let started = Instant::now();
            work(Duration::from_secs(90) + DRAW_SPACING * draw);
            started.elapsed()
        })
        .collect();
    timings.sort_unstable();
    let median = timings[timings.len() / 2];
    let slowest_tenth = timings[timings.len() * 9 / 10];
    println!("{label} over {DRAW_COUNT} draws: median {median:?}, p90 {slowest_tenth:?}");
}

fn draw_boot_frame(
    terminal: &mut Terminal<TestBackend>,
    state: &BootState,
    projection: &BootProjection,
    now: Duration,
) {
    terminal
        .draw(|frame| {
            frame.render_widget(
                BootView {
                    state,
                    projection,
                    colors: &Colors::default(),
                    timing: &Timing::default(),
                    glyphs: &Glyphs::default(),
                    now,
                    origin: &ryzen_origin(),
                    password_prompt: None,
                    hold_hint: None,
                    operator: None,
                },
                frame.area(),
            );
        })
        .expect("draw");
}

#[test]
#[ignore = "benchmark: cargo test --release frame_cost -- --ignored --nocapture"]
fn frame_cost_of_a_crowded_boot() {
    let state = crowded_boot();
    let mut terminal = Terminal::new(TestBackend::new(110, 40)).expect("test backend");
    report_timed_draws("whole frame", |now| {
        let projection = project_boot(&state, &Timing::default(), now);
        draw_boot_frame(&mut terminal, &state, &projection, now);
    });
}

#[test]
#[ignore = "benchmark: cargo test --release frame_cost -- --ignored --nocapture"]
fn frame_cost_with_a_cached_projection() {
    let state = crowded_boot();
    let projection = project_boot(&state, &Timing::default(), Duration::from_secs(90));
    let mut terminal = Terminal::new(TestBackend::new(110, 40)).expect("test backend");
    report_timed_draws("cached frame", |now| {
        draw_boot_frame(&mut terminal, &state, &projection, now);
    });
}

#[test]
#[ignore = "benchmark: cargo test --release frame_cost -- --ignored --nocapture"]
fn frame_cost_of_the_model_projection() {
    let state = crowded_boot();
    report_timed_draws("model projection", |now| {
        black_box(flatten_forest(state.waiting_forest(now)));
        black_box(state.milestones());
    });
}
