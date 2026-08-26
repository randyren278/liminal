//! `liminal-tui` -- the primary Liminal interface, per the 2026-08-26 architecture pivot
//! (ROADMAP.md, docs/ARCHITECTURE.md). Master plan §72 (mode structure), §80-83 (TUI contract).
//!
//! This binary is the roadmap's item 1: a mode skeleton (SPECTRAL/BELIEF/MEMORY/FIELD NOTES/
//! REFERENCE, §72) with a `ratatui-image` panel proven to render real animated bitmap output
//! over whatever terminal graphics protocol the user's terminal supports (Kitty, Sixel, or a
//! halfblock fallback), not ASCII-art approximation. It renders a synthetic demo pattern, not a
//! real sensor feed -- item 2 (Vision organ) wires up the first real one.

mod demo_frame;
mod mode;

use std::io;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use image::DynamicImage;
use mode::Mode;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Terminal;
use ratatui_image::picker::Picker;
use ratatui_image::{Resize, StatefulImage};

struct App {
    mode: Mode,
    tick: u32,
    picker: Picker,
    image_state: ratatui_image::protocol::StatefulProtocol,
}

impl App {
    fn new() -> io::Result<Self> {
        let picker = Picker::from_query_stdio().unwrap_or_else(|_| Picker::from_fontsize((8, 16)));
        let frame = demo_frame::plasma_frame(120, 60, 0);
        let image_state = picker.new_resize_protocol(DynamicImage::ImageRgb8(frame));
        Ok(Self {
            mode: Mode::Spectral,
            tick: 0,
            picker,
            image_state,
        })
    }

    fn advance(&mut self) {
        self.tick = self.tick.wrapping_add(1);
        let frame = demo_frame::plasma_frame(120, 60, self.tick);
        self.image_state = self
            .picker
            .new_resize_protocol(DynamicImage::ImageRgb8(frame));
    }
}

fn mode_body(mode: Mode) -> &'static str {
    match mode {
        Mode::Spectral => "Acoustic / RF / BLE fields render here once the sensor organs are wired (ROADMAP items 2, 5, 6).",
        Mode::Belief => "Fused occupancy/position hypothesis with epistemic confidence -- needs fusion (post-organs, §52).",
        Mode::Memory => "Timeline scrubber over Events/Episodes/Patterns -- needs liminald ingest (ROADMAP item 3).",
        Mode::FieldNotes => "Archivist/Ethnographer/Skeptic/Poet epistemic cards -- needs the agent layer (§63, later).",
        Mode::Reference => {
            "Calibration/debug view. The panel below is a SYNTHETIC DEMO PATTERN proving real \
             bitmap rendering over your terminal's graphics protocol -- it is NOT a camera feed. \
             §77 requires an explicit REFERENCE/CAMERA ACTIVE label whenever a real feed is \
             shown; this is not one, so it isn't labeled as one."
        }
    }
}

fn main() -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new()?;
    let tick_rate = Duration::from_millis(120);
    let mut last_tick = Instant::now();

    loop {
        terminal.draw(|frame| {
            let area = frame.area();
            let chunks = Layout::vertical([
                Constraint::Length(3),
                Constraint::Min(0),
                Constraint::Length(1),
            ])
            .split(area);

            let tabs: Vec<Span> = Mode::ALL
                .iter()
                .map(|m| {
                    if *m == app.mode {
                        Span::styled(
                            format!(" {} ", m.title()),
                            Style::default().fg(Color::Black).bg(Color::Cyan),
                        )
                    } else {
                        Span::raw(format!(" {} ", m.title()))
                    }
                })
                .collect();
            let header = Paragraph::new(Line::from(tabs))
                .block(Block::default().borders(Borders::ALL).title("LIMINAL"));
            frame.render_widget(header, chunks[0]);

            let body_chunks =
                Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)])
                    .split(chunks[1]);

            let body = Paragraph::new(mode_body(app.mode))
                .wrap(ratatui::widgets::Wrap { trim: true })
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(app.mode.title()),
                );
            frame.render_widget(body, body_chunks[0]);

            let image_block = Block::default()
                .borders(Borders::ALL)
                .title("DEMO RENDER (not a sensor feed)");
            let inner = image_block.inner(body_chunks[1]);
            frame.render_widget(image_block, body_chunks[1]);
            let image_widget = StatefulImage::default().resize(Resize::Fit(None));
            frame.render_stateful_widget(image_widget, inner, &mut app.image_state);

            let footer =
                Paragraph::new("[tab]/[shift+tab] switch mode  [1-5] jump to mode  [q] quit");
            frame.render_widget(footer, chunks[2]);
        })?;

        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => break,
                        KeyCode::Tab => app.mode = app.mode.next(),
                        KeyCode::BackTab => app.mode = app.mode.previous(),
                        KeyCode::Char(c) if c.is_ascii_digit() => {
                            if let Some(n) = c.to_digit(10) {
                                if n >= 1 && (n as usize) <= Mode::ALL.len() {
                                    app.mode = Mode::from_index(n as usize - 1);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        if last_tick.elapsed() >= tick_rate {
            app.advance();
            last_tick = Instant::now();
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}
