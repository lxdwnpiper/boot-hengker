use std::{path::Path, time::Duration};

use bootvine::{
    boot_model::boot_origin::BootOrigin,
    boot_view::IntroView,
    screen_settings::{config_file::read_config_file, font_glyphs::Glyphs},
};
use ratatui::{Terminal, backend::TestBackend};

fn main() {
    let config = read_config_file(Path::new("/etc/bootvine.toml")).unwrap_or_default();
    let operator = if config.look.operator.is_empty() {
        "piperCs".to_owned()
    } else {
        config.look.operator.clone()
    };
    let origin = BootOrigin {
        host: "localarch".to_owned(),
        system: None,
        generation: None,
        kernel: "Linux".to_owned(),
    };
    let intro_for = config.look.intro_for.max(Duration::from_millis(1));
    let frame_count = 4;
    for step in 0..frame_count {
        let now = intro_for * step / (frame_count - 1);
        let mut terminal = Terminal::new(TestBackend::new(88, 20)).expect("test backend");
        terminal
            .draw(|frame| {
                frame.render_widget(
                    IntroView {
                        now,
                        intro_for,
                        operator: &operator,
                        origin: &origin,
                        colors: &config.colors,
                        glyphs: &Glyphs::default(),
                    },
                    frame.area(),
                );
            })
            .expect("draw");
        let buffer = terminal.backend().buffer().clone();
        let area = buffer.area;
        println!("--- t = {} ms ---", now.as_millis());
        for row in 0..area.height {
            let line: String = (0..area.width)
                .map(|column| buffer[(column, row)].symbol().to_owned())
                .collect();
            println!("{line}");
        }
    }
}
