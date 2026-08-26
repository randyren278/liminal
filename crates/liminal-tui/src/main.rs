//! `liminal-tui` -- the primary Liminal interface, per the 2026-08-26 architecture pivot
//! (ROADMAP.md, docs/ARCHITECTURE.md). Master plan §72 (mode structure), §80-83 (TUI contract).
//!
//! Roadmap items 1 and 4: a mode skeleton (SPECTRAL/BELIEF/MEMORY/FIELD NOTES/REFERENCE, §72)
//! with a `ratatui-image` panel, now wired to `liminal-ledger`'s real SQLite store. REFERENCE
//! mode shows a real skeleton rendered from the most recent `liminal-capture` pose observation
//! when one exists (see `ledger_view.rs` for why this is a skeleton, not a camera image), and
//! falls back to the roadmap-item-1 synthetic demo pattern when no real data has arrived yet.

mod demo_frame;
mod ledger_view;
mod mode;
mod skeleton_frame;

use std::io;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use image::DynamicImage;
use ledger_view::{read_ledger_snapshot, LedgerSnapshot};
use mode::Mode;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Terminal;
use ratatui_image::picker::Picker;
use ratatui_image::{Resize, StatefulImage};

struct App {
    mode: Mode,
    tick: u32,
    picker: Picker,
    image_state: ratatui_image::protocol::StatefulProtocol,
    image_title: &'static str,
    snapshot: Option<LedgerSnapshot>,
}

const IMAGE_WIDTH: u32 = 120;
const IMAGE_HEIGHT: u32 = 60;
const DEMO_IMAGE_TITLE: &str = "DEMO RENDER (not a sensor feed)";
const LIVE_IMAGE_TITLE: &str = "LIVE POSE (derived from real Vision data, not a camera image)";
const BG: Color = Color::Rgb(8, 15, 24);
const PANEL: Color = Color::Rgb(15, 28, 40);
const MUTED: Color = Color::Rgb(125, 151, 164);
const TEAL: Color = Color::Rgb(69, 224, 190);
const AMBER: Color = Color::Rgb(255, 184, 92);

impl App {
    fn new() -> io::Result<Self> {
        let picker = Picker::from_query_stdio().unwrap_or_else(|_| Picker::from_fontsize((8, 16)));
        let frame = demo_frame::plasma_frame(IMAGE_WIDTH, IMAGE_HEIGHT, 0);
        let image_state = picker.new_resize_protocol(DynamicImage::ImageRgb8(frame));
        Ok(Self {
            mode: Mode::Spectral,
            tick: 0,
            picker,
            image_state,
            image_title: DEMO_IMAGE_TITLE,
            snapshot: None,
        })
    }

    fn advance(&mut self) {
        self.tick = self.tick.wrapping_add(1);
        self.snapshot = read_ledger_snapshot(&liminal_ledger::default_db_path());

        let (frame, title) = match &self.snapshot {
            Some(snapshot) if !snapshot.latest_camera_joints.is_empty() => (
                skeleton_frame::skeleton_frame(
                    IMAGE_WIDTH,
                    IMAGE_HEIGHT,
                    &snapshot.latest_camera_joints,
                    0.25,
                ),
                LIVE_IMAGE_TITLE,
            ),
            _ => (
                demo_frame::plasma_frame(IMAGE_WIDTH, IMAGE_HEIGHT, self.tick),
                DEMO_IMAGE_TITLE,
            ),
        };
        self.image_title = title;
        self.image_state = self
            .picker
            .new_resize_protocol(DynamicImage::ImageRgb8(frame));
    }
}

fn mode_body(mode: Mode, snapshot: &Option<LedgerSnapshot>) -> String {
    match mode {
        Mode::Spectral => match snapshot {
            Some(s) if !s.stream_event_counts.is_empty() => format!(
                "LIVE SENSOR FIELD\n\n{}\n\nDerived features only. Raw audio, video, SSIDs, and device names never enter the ledger.",
                s.stream_event_counts.iter().map(|(stream, count)| format!("{stream:<12} {count:>6} observations")).collect::<Vec<_>>().join("\n")
            ),
            _ => "WAITING FOR SENSOR FIELD\n\nStart the capture organ to stream derived camera, microphone, Wi-Fi, and Bluetooth observations into the ledger.".to_string(),
        },
        Mode::Belief => "NO FUSED BELIEF YET\n\nThe ingest path is live, but fusion is intentionally not inferred from raw observations. This panel will become confidence-aware once the fusion layer is built.".to_string(),
        Mode::Memory => match snapshot {
            Some(s) => format!(
                "LEDGER ONLINE\n\n{} total event(s) ingested.\n\nTimeline scrubbing over Events/Episodes/Patterns is next; this view is already reading the canonical SQLite ledger.",
                s.total_event_count
            ),
            None => "LEDGER NOT CREATED\n\nStart the centralized launcher to bring up liminald and the capture organs.".to_string(),
        },
        Mode::FieldNotes => "FIELD NOTES QUEUED\n\nArchivist / Ethnographer / Skeptic / Poet cards will appear here when the agent layer is connected. No interpretation is fabricated from missing data.".to_string(),
        Mode::Reference => match snapshot {
            Some(s) if !s.latest_camera_joints.is_empty() => format!(
                "REFERENCE / POSE DATA ACTIVE. {} joint(s) from the most recent liminal-capture \
                 observation, out of {} total ingested events. This is a skeleton derived from \
                 real Vision output -- never a camera image (§120: zero raw frames persisted or \
                 transmitted).",
                s.latest_camera_joints.len(),
                s.total_event_count
            ),
            _ => "Calibration/debug view. No real pose data ingested yet -- the panel below is a \
                  SYNTHETIC DEMO PATTERN proving real bitmap rendering over your terminal's \
                  graphics protocol. Run liminald and liminal-capture to see real data here."
                .to_string(),
        },
    }
}

fn main() -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new()?;
    let tick_rate = Duration::from_millis(200);
    let mut last_tick = Instant::now();

    loop {
        terminal.draw(|frame| {
            let area = frame.area();
            frame.render_widget(Block::default().style(Style::default().bg(BG)), area);
            let chunks = Layout::vertical([
                Constraint::Length(4),
                Constraint::Min(0),
                Constraint::Length(2),
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
            let mut header_lines = vec![Line::from(vec![
                Span::styled(
                    " ◈ LIMINAL ",
                    Style::default()
                        .fg(BG)
                        .bg(TEAL)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "  SENSORIUM / OPERATOR CONSOLE",
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
            ])];
            header_lines.push(Line::from(tabs));
            let header = Paragraph::new(Text::from(header_lines))
                .style(Style::default().bg(PANEL))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(TEAL)),
                );
            frame.render_widget(header, chunks[0]);

            let body_chunks =
                Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)])
                    .split(chunks[1]);
            let left_chunks =
                Layout::vertical([Constraint::Length(9), Constraint::Min(0)]).split(body_chunks[0]);

            let mut status_lines = vec![Line::from(Span::styled(
                format!(
                    "{}  /  {}",
                    app.mode.title(),
                    if app.snapshot.is_some() {
                        "LEDGER ONLINE"
                    } else {
                        "WAITING FOR LEDGER"
                    }
                ),
                Style::default()
                    .fg(if app.snapshot.is_some() { TEAL } else { AMBER })
                    .add_modifier(Modifier::BOLD),
            ))];
            if let Some(snapshot) = &app.snapshot {
                status_lines.push(Line::from(format!(
                    "EVENTS  {:>8}",
                    snapshot.total_event_count
                )));
                for (stream, count) in &snapshot.stream_event_counts {
                    status_lines.push(Line::from(format!("{stream:<8} {count:>8}")));
                }
                if let Some((kind, stream)) = &snapshot.latest_event {
                    status_lines.push(Line::from(Span::styled(
                        format!("\nLAST  {kind} / {stream}"),
                        Style::default().fg(MUTED),
                    )));
                }
            } else {
                status_lines.push(Line::from(Span::styled(
                    "EVENTS         --",
                    Style::default().fg(MUTED),
                )));
                status_lines.push(Line::from(Span::styled(
                    "Start with scripts/run-liminal.sh",
                    Style::default().fg(MUTED),
                )));
            }
            let status = Paragraph::new(Text::from(status_lines))
                .style(Style::default().fg(Color::White).bg(PANEL))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(Color::Rgb(44, 77, 91)))
                        .title(" TELEMETRY "),
                );
            frame.render_widget(status, left_chunks[0]);

            let body = Paragraph::new(mode_body(app.mode, &app.snapshot))
                .style(Style::default().fg(Color::Rgb(218, 232, 235)).bg(PANEL))
                .wrap(ratatui::widgets::Wrap { trim: true })
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(TEAL))
                        .title(app.mode.title()),
                );
            frame.render_widget(body, left_chunks[1]);

            let image_block = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Rgb(44, 77, 91)))
                .title(app.image_title);
            let inner = image_block.inner(body_chunks[1]);
            frame.render_widget(image_block, body_chunks[1]);
            let image_widget = StatefulImage::default().resize(Resize::Fit(None));
            frame.render_stateful_widget(image_widget, inner, &mut app.image_state);

            let footer = Paragraph::new(Line::from(vec![
                Span::styled(
                    " TAB ",
                    Style::default()
                        .fg(BG)
                        .bg(TEAL)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" switch mode   ", Style::default().fg(MUTED)),
                Span::styled(
                    "1-5",
                    Style::default().fg(AMBER).add_modifier(Modifier::BOLD),
                ),
                Span::styled(" jump   ", Style::default().fg(MUTED)),
                Span::styled(
                    "q / esc",
                    Style::default().fg(AMBER).add_modifier(Modifier::BOLD),
                ),
                Span::styled(" quit", Style::default().fg(MUTED)),
            ]))
            .style(Style::default().bg(BG));
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
