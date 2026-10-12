//! TUI composition root: joins simulation controls and read-only presentation.
mod app;
pub mod diagnostics;
pub mod verify;
pub mod view;
pub use app::App;
pub use view::{phase_label, primary_label};

pub fn render(frame: &mut ratatui::Frame, app: &App) {
    let context = view::ViewContext {
        real_device: app.real_device,
        scenario_label: if app.real_device {
            match app.runtime.backend() {
                app::AppBackend::Emulator(b) if b.serial != sofalink_emulator::SERIAL => "Fire TV",
                _ => "Android TV",
            }
        } else {
            app.scenario.label()
        },
        scenario_description: if app.real_device {
            if app.test_source {
                "测试图与测试音 · 实际解码、呈现与音频播放反馈"
            } else if app.reference_video {
                "动态参考图与 Windows 系统声音 · 实际客户端播放反馈"
            } else {
                "Windows 桌面与系统声音 · 实际客户端播放反馈"
            }
        } else {
            app.scenario.description()
        },
        details: app.details,
        quitting: app.quitting,
        notice: app.notice.as_deref(),
        history: &app.history,
    };
    view::render(frame, app.runtime.snapshot(), &context);
}

/// Render the actual widgets without requiring a terminal, for review and regression checks.
pub fn render_text(app: &App, width: u16, height: u16) -> Result<String, std::convert::Infallible> {
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height))?;
    terminal.draw(|frame| render(frame, app))?;
    let buffer = terminal.backend().buffer();
    let mut text = String::new();
    for y in 0..height {
        let mut x = 0;
        while x < width {
            let symbol = buffer[(x, y)].symbol();
            text.push_str(symbol);
            // A wide CJK glyph occupies two cells; its continuation is not an extra space.
            x += ratatui::text::Span::raw(symbol).width().max(1) as u16;
        }
        text.push('\n');
    }
    Ok(text)
}
