//! `liminal-tui` -- the primary Liminal interface, per the 2026-08-26 architecture pivot
//! (ROADMAP.md, docs/ARCHITECTURE.md). Master plan §72 (mode structure), §80-83 (TUI contract).
//!
//! Roadmap items 1 and 4: a mode skeleton (SPECTRAL/BELIEF/MEMORY/FIELD NOTES/REFERENCE, §72)
//! with a `ratatui-image` panel, now wired to `liminal-ledger`'s real SQLite store. REFERENCE
//! mode shows a real skeleton rendered from the most recent `liminal-capture` pose observation
//! when one exists (see `ledger_view.rs` for why this is a skeleton, not a camera image), and
//! falls back to the roadmap-item-1 synthetic demo pattern when no real data has arrived yet.

mod belief;
mod belief_frame;
mod calibration_view;
#[cfg(test)]
mod demo_frame;
mod field_notes;
mod ledger_view;
mod memory_frame;
mod mode;
mod skeleton_frame;
mod telemetry_frame;

use std::io;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use belief::derive_belief;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use field_notes::{build_field_notes, format_field_notes};
use image::{imageops::FilterType, DynamicImage};
use ledger_view::{
    extract_recent_observation_rates, LatestRecord, LedgerSnapshot, TelemetrySnapshot,
};
use mode::Mode;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Terminal;
use ratatui_image::picker::{Picker, ProtocolType};
use ratatui_image::{Resize, StatefulImage};

type ImageJob = (
    ratatui_image::protocol::StatefulProtocol,
    Resize,
    ratatui::layout::Rect,
);
type ImageResult = (
    ratatui_image::protocol::StatefulProtocol,
    ratatui::layout::Rect,
);

struct App {
    mode: Mode,
    tick: u32,
    picker: Picker,
    image_state: Option<ratatui_image::protocol::StatefulProtocol>,
    image_worker_tx: Sender<ImageJob>,
    image_result_rx: Receiver<ImageResult>,
    pending_image: Option<DynamicImage>,
    image_job_in_flight: bool,
    image_area: Option<ratatui::layout::Rect>,
    image_title: &'static str,
    snapshot: Option<LedgerSnapshot>,
    ledger_error: Option<String>,
    memory_window: usize,
    history_cursor: usize,
    paused: bool,
    vision_enabled: bool,
    demo_only: bool,
    demo_frame_limit: Option<u32>,
    labels_path: Option<PathBuf>,
    snapshot_tx: Sender<Result<Option<LedgerSnapshot>, String>>,
    snapshot_rx: Receiver<Result<Option<LedgerSnapshot>, String>>,
    snapshot_in_flight: bool,
    last_snapshot_request: Option<Instant>,
}

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}

fn belief_for_snapshot(snapshot: &LedgerSnapshot) -> belief::BeliefSnapshot {
    snapshot
        .persisted_belief
        .as_ref()
        .map(|belief| belief::BeliefSnapshot {
            occupancy_probability: belief.occupancy_probability,
            confidence: belief.confidence,
            disagreement: belief.disagreement,
            observed_modalities: belief.observed_modalities,
            sensor_health: belief.sensor_health,
            state: belief.state,
        })
        .unwrap_or_else(|| derive_belief(&snapshot.telemetry))
}

// The display pane is roughly 1.1:1 in terminal pixels once cell aspect ratio is included. A
// matching source aspect lets Resize::Scale occupy the pane instead of leaving a large void.
const HALFBLOCK_IMAGE_WIDTH: u32 = 120;
const HALFBLOCK_IMAGE_HEIGHT: u32 = 108;
const KITTY_IMAGE_WIDTH: u32 = 480;
const KITTY_IMAGE_HEIGHT: u32 = 432;
const DEMO_IMAGE_TITLE: &str = "DEMO RENDER (synthetic spectral composition)";
const TELEMETRY_IMAGE_TITLE: &str = "LIVE TELEMETRY FIELD (derived features only)";
const LIVE_IMAGE_TITLE: &str = "LIVE POSE (derived from real Vision data, not a camera image)";
const BG: Color = Color::Rgb(8, 15, 24);
const PANEL: Color = Color::Rgb(15, 28, 40);
const MUTED: Color = Color::Rgb(125, 151, 164);
const TEAL: Color = Color::Rgb(69, 224, 190);
const AMBER: Color = Color::Rgb(255, 184, 92);

fn demo_telemetry() -> TelemetrySnapshot {
    TelemetrySnapshot {
        camera_presence: Some(0.72),
        audio_rms: Some(0.18),
        audio_centroid_hz: Some(2800.0),
        audio_vad: Some(0.64),
        wifi_rssi_mean: Some(-48.0),
        wifi_noise_mean: Some(-86.0),
        wifi_network_count: Some(7.0),
        bluetooth_cluster_count: Some(4.0),
        bluetooth_mean_rssi: Some(-58.0),
    }
}

fn picker_from_terminal() -> Picker {
    let mut picker = Picker::from_query_stdio().unwrap_or_else(|_| Picker::from_fontsize((8, 16)));

    // The picker query selects a native protocol when the terminal answers. Ghostty's native
    // image path is Kitty, so prefer it there for full pixel fidelity. An explicit override is
    // still available for terminals, multiplexers, or diagnostic captures that need halfblocks.
    let protocol = std::env::var("LIMINAL_IMAGE_PROTOCOL")
        .ok()
        .and_then(|value| match value.to_ascii_lowercase().as_str() {
            "halfblocks" | "halfblock" => Some(ProtocolType::Halfblocks),
            "kitty" => Some(ProtocolType::Kitty),
            "sixel" => Some(ProtocolType::Sixel),
            "iterm2" | "iterm" => Some(ProtocolType::Iterm2),
            _ => None,
        });
    let ghostty =
        std::env::var("TERM_PROGRAM").is_ok_and(|term| term.eq_ignore_ascii_case("ghostty"));
    if let Some(protocol) = protocol {
        picker.set_protocol_type(protocol);
    } else if ghostty && picker.protocol_type() != ProtocolType::Halfblocks {
        picker.set_protocol_type(ProtocolType::Kitty);
    }
    picker
}

#[cfg(test)]
fn image_dimensions(picker: Picker) -> (u32, u32) {
    match picker.protocol_type() {
        ProtocolType::Kitty => (KITTY_IMAGE_WIDTH, KITTY_IMAGE_HEIGHT),
        _ => (HALFBLOCK_IMAGE_WIDTH, HALFBLOCK_IMAGE_HEIGHT),
    }
}

fn spawn_image_worker() -> (Sender<ImageJob>, Receiver<ImageResult>) {
    let (tx_worker, rx_worker) = mpsc::channel::<ImageJob>();
    let (tx_result, rx_result) = mpsc::channel();
    thread::spawn(move || {
        while let Ok((mut protocol, resize, area)) = rx_worker.recv() {
            let background_color = protocol.background_color();
            protocol.resize_encode(&resize, background_color, area);
            if tx_result.send((protocol, area)).is_err() {
                break;
            }
        }
    });
    (tx_worker, rx_result)
}

fn enable_tui_colors() {
    // Liminal is a full-screen operator surface, not a line-oriented command. A parent shell's
    // NO_COLOR setting otherwise strips the telemetry palette and makes the visual field appear
    // blank in terminals such as Ghostty. Keep an explicit opt-out for automation/debugging.
    if !matches!(
        std::env::var("LIMINAL_COLOR").ok().as_deref(),
        Some("0") | Some("off") | Some("false")
    ) {
        std::env::remove_var("NO_COLOR");
    }
}

impl App {
    fn new(
        demo_only: bool,
        demo_frame_limit: Option<u32>,
        labels_path: Option<PathBuf>,
        image_worker_tx: Sender<ImageJob>,
        image_result_rx: Receiver<ImageResult>,
    ) -> io::Result<Self> {
        let picker = picker_from_terminal();
        let (snapshot_tx, snapshot_rx) = mpsc::channel();
        let mut app = Self {
            mode: Mode::Spectral,
            tick: 0,
            picker,
            image_state: None,
            image_worker_tx,
            image_result_rx,
            pending_image: None,
            image_job_in_flight: false,
            image_area: None,
            image_title: DEMO_IMAGE_TITLE,
            snapshot: None,
            ledger_error: None,
            memory_window: 24,
            history_cursor: 0,
            paused: false,
            vision_enabled: true,
            demo_only,
            demo_frame_limit,
            labels_path,
            snapshot_tx,
            snapshot_rx,
            snapshot_in_flight: false,
            last_snapshot_request: None,
        };
        // Prime the first frame from the ledger so a live session never flashes a stale demo
        // title/image while waiting for the first 200 ms poll tick.
        app.request_snapshot();
        app.advance_visual();
        Ok(app)
    }

    fn poll_image(&mut self) {
        let mut completed = None;
        while let Ok(result) = self.image_result_rx.try_recv() {
            completed = Some(result);
        }
        let Some((protocol, area)) = completed else {
            return;
        };
        self.image_state = Some(protocol);
        self.image_area = Some(area);
        self.image_job_in_flight = false;
    }

    fn queue_image(&mut self, area: ratatui::layout::Rect) {
        if self.image_job_in_flight {
            return;
        }
        let Some(frame) = self.pending_image.take() else {
            return;
        };
        let protocol = self.picker.new_resize_protocol(frame);
        if self
            .image_worker_tx
            .send((protocol, Resize::Scale(Some(FilterType::Lanczos3)), area))
            .is_ok()
        {
            self.image_job_in_flight = true;
        }
    }

    fn request_snapshot(&mut self) {
        if self.demo_only || self.snapshot_in_flight {
            return;
        }
        if self
            .last_snapshot_request
            .is_some_and(|requested| requested.elapsed() < Duration::from_secs(1))
        {
            return;
        }
        self.snapshot_in_flight = true;
        self.last_snapshot_request = Some(Instant::now());
        let sender = self.snapshot_tx.clone();
        thread::spawn(move || {
            let result =
                ledger_view::read_ledger_snapshot_checked(&liminal_ledger::default_db_path());
            let _ = sender.send(result);
        });
    }

    fn poll_snapshot(&mut self) {
        match self.snapshot_rx.try_recv() {
            Ok(Ok(snapshot)) => {
                self.snapshot = snapshot;
                self.ledger_error = None;
                self.snapshot_in_flight = false;
                self.advance_visual();
            }
            Ok(Err(error)) => {
                self.snapshot = None;
                self.ledger_error = Some(error);
                self.snapshot_in_flight = false;
                self.advance_visual();
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.snapshot_in_flight = false;
                self.ledger_error = Some("snapshot worker disconnected".to_string());
            }
        }
    }

    fn advance_visual(&mut self) {
        self.tick = self.tick.wrapping_add(1);
        let image_width = self.image_width();
        let image_height = self.image_height();

        if let Some(snapshot) = &self.snapshot {
            self.history_cursor = self
                .history_cursor
                .min(snapshot.recent_records.len().saturating_sub(1));
        } else {
            self.history_cursor = 0;
        }

        let (frame, title) = match (&self.snapshot, self.mode) {
            (Some(snapshot), Mode::Belief) if snapshot.has_telemetry() => (
                belief_frame::belief_frame(
                    image_width,
                    image_height,
                    self.tick,
                    belief_for_snapshot(snapshot),
                ),
                "LIVE BELIEF VOLUME (heuristic, uncertainty shown)",
            ),
            (Some(snapshot), Mode::Memory) if !snapshot.recent_observations.is_empty() => (
                memory_frame::memory_frame(
                    image_width,
                    image_height,
                    &snapshot.recent_observations
                        [..snapshot.recent_observations.len().min(self.memory_window)],
                ),
                "MEMORY TIMELINE (ledger observations, gaps not interpolated)",
            ),
            // SPECTRAL is a telemetry view even when Vision also has pose data. Pose belongs to
            // the explicitly selected calibration/debug screen below.
            (Some(snapshot), Mode::Spectral) if snapshot.has_telemetry() => (
                telemetry_frame::spectral_frame(
                    image_width,
                    image_height,
                    self.tick,
                    &snapshot.telemetry,
                ),
                TELEMETRY_IMAGE_TITLE,
            ),
            (Some(snapshot), Mode::Reference)
                if self.vision_enabled && !snapshot.latest_camera_joints.is_empty() =>
            {
                (
                    skeleton_frame::skeleton_frame(
                        image_width,
                        image_height,
                        &snapshot.latest_camera_joints,
                        0.25,
                    ),
                    LIVE_IMAGE_TITLE,
                )
            }
            (Some(snapshot), Mode::Reference)
                if !self.vision_enabled && snapshot.has_telemetry() =>
            {
                (
                    telemetry_frame::spectral_frame(
                        image_width,
                        image_height,
                        self.tick,
                        &snapshot.telemetry,
                    ),
                    "VISION OFF / NONVISUAL TELEMETRY FIELD",
                )
            }
            (Some(snapshot), Mode::Calibration) if self.labels_path.is_some() => (
                telemetry_frame::spectral_frame(
                    image_width,
                    image_height,
                    self.tick,
                    &snapshot.telemetry,
                ),
                "CALIBRATION INPUT (telemetry remains read-only)",
            ),
            (Some(snapshot), _) if snapshot.has_telemetry() => (
                telemetry_frame::spectral_frame(
                    image_width,
                    image_height,
                    self.tick,
                    &snapshot.telemetry,
                ),
                TELEMETRY_IMAGE_TITLE,
            ),
            _ if self.ledger_error.is_some() => (
                image::RgbImage::from_pixel(image_width, image_height, image::Rgb([8, 15, 24])),
                "LEDGER UNAVAILABLE (render disabled)",
            ),
            _ if self.demo_only => (
                telemetry_frame::spectral_frame(
                    image_width,
                    image_height,
                    self.tick,
                    &demo_telemetry(),
                ),
                DEMO_IMAGE_TITLE,
            ),
            _ => (
                // Normal mode must not briefly present synthetic plasma while the first ledger
                // snapshot is loading. Keep the visual state honest and let the live field take
                // over only after derived telemetry exists.
                telemetry_frame::spectral_frame(
                    image_width,
                    image_height,
                    self.tick,
                    &TelemetrySnapshot::default(),
                ),
                "WAITING FOR TELEMETRY (no live derived values yet)",
            ),
        };
        self.image_title = title;
        self.pending_image = Some(DynamicImage::ImageRgb8(frame));
    }

    fn image_width(&self) -> u32 {
        match self.picker.protocol_type() {
            ProtocolType::Kitty => KITTY_IMAGE_WIDTH,
            _ => HALFBLOCK_IMAGE_WIDTH,
        }
    }

    fn image_height(&self) -> u32 {
        match self.picker.protocol_type() {
            ProtocolType::Kitty => KITTY_IMAGE_HEIGHT,
            _ => HALFBLOCK_IMAGE_HEIGHT,
        }
    }
}

trait SnapshotTelemetry {
    fn has_telemetry(&self) -> bool;
}

impl SnapshotTelemetry for LedgerSnapshot {
    fn has_telemetry(&self) -> bool {
        let t = &self.telemetry;
        t.camera_presence.is_some()
            || t.audio_rms.is_some()
            || t.audio_centroid_hz.is_some()
            || t.audio_vad.is_some()
            || t.wifi_rssi_mean.is_some()
            || t.wifi_noise_mean.is_some()
            || t.wifi_network_count.is_some()
            || t.bluetooth_cluster_count.is_some()
            || t.bluetooth_mean_rssi.is_some()
    }
}

const MEMORY_WINDOWS: [usize; 5] = [24, 96, 256, 1024, 4096];

fn next_memory_window(current: usize) -> usize {
    MEMORY_WINDOWS
        .iter()
        .copied()
        .find(|window| *window > current)
        .unwrap_or(*MEMORY_WINDOWS.last().unwrap())
}

fn previous_memory_window(current: usize) -> usize {
    MEMORY_WINDOWS
        .iter()
        .rev()
        .copied()
        .find(|window| *window < current)
        .unwrap_or(MEMORY_WINDOWS[0])
}

#[cfg(test)]
fn format_latest_record(record: Option<&LatestRecord>) -> String {
    let Some(record) = record else {
        return "LATEST RECORD\nnone".to_string();
    };
    let stream = record.stream.as_deref().unwrap_or("none");
    let timestamp = record
        .timestamp_us
        .map_or_else(|| "none".to_string(), |value| value.to_string());
    let sources = if record.provenance_sources.is_empty() {
        "none recorded".to_string()
    } else {
        record.provenance_sources.join(", ")
    };
    format!(
        "LATEST RECORD\n{} / {} / stream {} / ts_us {}\nPROVENANCE SOURCES\n{}",
        record.id, record.kind, stream, timestamp, sources
    )
}

fn format_record_inline(record: Option<&LatestRecord>) -> String {
    let Some(record) = record else {
        return "none".to_string();
    };
    format!(
        "{} / {} / {} / ts_us {}",
        record.id,
        record.kind,
        record.stream.as_deref().unwrap_or("stream none"),
        record
            .timestamp_us
            .map_or_else(|| "none".to_string(), |value| value.to_string())
    )
}

fn format_sources_inline(record: Option<&LatestRecord>) -> String {
    record
        .map(|record| {
            if record.provenance_sources.is_empty() {
                "none recorded".to_string()
            } else {
                record.provenance_sources.join(", ")
            }
        })
        .unwrap_or_else(|| "none recorded".to_string())
}

fn apply_key(app: &mut App, code: KeyCode) -> bool {
    match code {
        KeyCode::Char('q') | KeyCode::Esc => true,
        KeyCode::Char('p') => {
            app.paused = !app.paused;
            false
        }
        KeyCode::Char('v') => {
            app.vision_enabled = !app.vision_enabled;
            false
        }
        KeyCode::Char('[') => {
            app.memory_window = previous_memory_window(app.memory_window);
            false
        }
        KeyCode::Char(']') => {
            app.memory_window = next_memory_window(app.memory_window);
            false
        }
        KeyCode::Char('j') | KeyCode::Down => {
            if let Some(snapshot) = &app.snapshot {
                app.history_cursor =
                    (app.history_cursor + 1).min(snapshot.recent_records.len().saturating_sub(1));
            }
            false
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.history_cursor = app.history_cursor.saturating_sub(1);
            false
        }
        KeyCode::Tab => {
            app.mode = app.mode.next();
            false
        }
        KeyCode::BackTab => {
            app.mode = app.mode.previous();
            false
        }
        KeyCode::Char(c) if c.is_ascii_digit() => {
            if let Some(n) = c.to_digit(10) {
                if n >= 1 && (n as usize) <= Mode::ALL.len() {
                    app.mode = Mode::from_index(n as usize - 1);
                }
            }
            false
        }
        _ => false,
    }
}

fn validate_demo_frame_options(demo_only: bool, demo_frame_limit: Option<u32>) -> io::Result<()> {
    if demo_frame_limit.is_some() && !demo_only {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "--demo-frames requires --demo",
        ));
    }
    Ok(())
}

// This is a pure renderer formatter; keeping its inputs explicit makes the read-only
// telemetry/ledger boundary auditable at the call site.
#[allow(clippy::too_many_arguments)]
fn mode_body(
    mode: Mode,
    snapshot: &Option<LedgerSnapshot>,
    memory_window: usize,
    history_cursor: usize,
    vision_enabled: bool,
    demo_only: bool,
    ledger_error: Option<&str>,
    labels_path: Option<&std::path::Path>,
) -> String {
    if demo_only {
        return format!(
            "DEMO MODE / SYNTHETIC\n\nThis animated bitmap is a renderer demonstration, not sensor output.\n\nSelected mode: {}\n\nRun without `--demo` to read the local ledger and render live derived telemetry.",
            mode.title()
        );
    }
    if let Some(error) = ledger_error {
        return format!(
            "LEDGER ERROR\n\nThe local ledger exists but could not be read.\n\n{error}\n\nSynthetic demo rendering is disabled in normal mode so a storage failure cannot look like healthy telemetry."
        );
    }
    match mode {
        Mode::Spectral => match snapshot {
            Some(s) if !s.stream_event_counts.is_empty() => {
                let t = &s.telemetry;
                let rates = extract_recent_observation_rates(&s.recent_observations)
                    .iter()
                    .map(|(stream, rate)| format!("{stream:<12} {rate:>6.2} / second"))
                    .collect::<Vec<_>>()
                    .join("\n");
                format!(
                    "LIVE SENSOR FIELD\n\n{}\n\nRECENT OBSERVATION RATES\n{}\n\nLATEST DERIVED VALUES\n{}\n{}\n{}\n{}\n{}\n\nLATEST OBSERVATION AGE\n{}\n\nThe field at right is driven by these values. Raw audio, video, SSIDs, and device names never enter the ledger.",
                    s.stream_event_counts.iter().map(|(stream, count)| format!("{stream:<12} {count:>6} observations")).collect::<Vec<_>>().join("\n"),
                    if rates.is_empty() { "none".to_string() } else { rates },
                    t.camera_presence.map_or("camera presence --".to_string(), |v| format!("camera presence {v:.2}")),
                    t.audio_rms.map_or("audio rms       --".to_string(), |v| format!("audio rms       {v:.3}  / centroid {:>5.0} Hz", t.audio_centroid_hz.unwrap_or(0.0))),
                    t.wifi_rssi_mean.map_or("wifi signal     --".to_string(), |v| format!("wifi signal     {v:>5.1} dBm / density {:>3.0}", t.wifi_network_count.unwrap_or(0.0))),
                    format_bluetooth_value(t.bluetooth_cluster_count, t.bluetooth_mean_rssi),
                    t.audio_vad.map_or("voice activity  --".to_string(), |v| format!("voice activity  {v:.2}  (heuristic)")),
                    format_observation_ages(&s.latest_observation_timestamps),
                )
            }
            _ => "WAITING FOR SENSOR FIELD\n\nStart the capture organ to stream derived camera, microphone, Wi-Fi, and Bluetooth observations into the ledger.".to_string(),
        },
        Mode::Belief => match snapshot {
            Some(s) if s.has_telemetry() => {
                let b = belief_for_snapshot(s);
                let source = if s.persisted_belief.is_some() { "daemon-persisted" } else { "render fallback" };
                let evidence = s
                    .persisted_belief
                    .as_ref()
                    .map(|belief| belief.evidence_ids.join(", "))
                    .unwrap_or_else(|| "live telemetry snapshot".to_string());
                format!("LIVE FUSED BELIEF\n\noccupancy       {:.2}\nconfidence      {:.2}\ndisagreement    {:.2}\nsensor health   {:.2}\nstate           {:?}\nmodalities      {}\nevidence        {}\nsource          {}\n\nThis is a transparent first-pass heuristic. Wi-Fi contributes environmental context only; it cannot create an occupancy belief.", b.occupancy_probability, b.confidence, b.disagreement, b.sensor_health, b.state, b.observed_modalities, evidence, source)
            }
            _ => "UNKNOWN PRESENCE\n\nNo supporting sensor evidence has arrived yet. The belief volume remains explicitly uncertain.".to_string(),
        },
        Mode::Memory => match snapshot {
            Some(s) if !s.recent_observations.is_empty() => format!(
                "LEDGER TIMELINE\nEVENTS {}  OBSERVATIONS {}/{}  WINDOW {}\nMEMORY {} episodes / {} patterns  COVERAGE {} day buckets\nOCCUPANCY {}  GAPS {}  AGENTS {}\nHISTORY {}/{}\n{}\nSOURCES {}\nj/k or arrows: browse records (read-only)",
                s.total_event_count,
                s.recent_observations.len().min(memory_window),
                s.recent_observations.len(),
                memory_window,
                s.episode_count,
                s.pattern_count,
                s.historical_buckets.len(),
                s.occupancy_events.len(),
                if s.pending_gap_streams.is_empty() {
                    "none".to_string()
                } else {
                    s.pending_gap_streams.join(", ")
                },
                s.agent_run_count,
                history_cursor.saturating_add(1),
                s.recent_records.len(),
                format_record_inline(s.recent_records.get(history_cursor)),
                format_sources_inline(s.recent_records.get(history_cursor))
            ),
            Some(s) => format!("LEDGER ONLINE\n\n{} event(s) ingested, but no timestamped observations are available yet.", s.total_event_count),
            None => "LEDGER NOT CREATED\n\nStart the centralized launcher to bring up liminald and the capture organs.".to_string(),
        },
        Mode::FieldNotes => match snapshot {
            Some(s) if !s.stream_event_counts.is_empty() => {
                format_field_notes(&build_field_notes(s))
            }
            _ => "NOTES / WAITING\n\nNo ledger observations are available. Notes stay empty rather than manufacturing a story from missing data.".to_string(),
        },
        Mode::Reference => match snapshot {
            Some(s) if vision_enabled && !s.latest_camera_joints.is_empty() => format!(
                "POSE DATA ACTIVE. {} joint(s) from the most recent liminal-capture \
                 observation, out of {} total ingested events. This is a skeleton derived from \
                 real Vision output -- never a camera image (§120: zero raw frames persisted or \
                 transmitted).",
                s.latest_camera_joints.len(),
                s.total_event_count
            ),
            _ if !vision_enabled => "Vision display is OFF. The panel below uses only nonvisual \
                  derived telemetry; press `v` to restore the pose reference view.".to_string(),
            _ => "POSE / WAITING. No real pose data ingested yet -- the panel below is a \
                  SYNTHETIC DEMO PATTERN proving real bitmap rendering over your terminal's \
                  graphics protocol. Run liminald and liminal-capture to see real data here."
                .to_string(),
        },
        Mode::Calibration => match labels_path {
            None => "CALIBRATE / NO LABEL FILE\n\nNo human or approved reference labels were supplied. This view will not treat sensor output as ground truth.\n\nRun with `--labels /path/to/trial-labels.jsonl` to show offline accuracy, Brier score, precision, and recall.".to_string(),
            Some(path) => match snapshot {
                Some(_) => match ledger_view::read_calibration_report_checked(
                    &liminal_ledger::default_db_path(),
                    path,
                ) {
                    Ok(Some(report)) => calibration_view::format_report(path, &report),
                    Ok(None) => format!(
                        "CALIBRATE / NO MATCHES\n\nThe supplied labels at {} did not match any persisted daemon fusion belief within the timestamp window. No live model state was changed.",
                        path.display()
                    ),
                    Err(error) => format!(
                        "CALIBRATE / LABEL ERROR\n\n{}\n\nThe live heuristic remains unchanged.",
                        error
                    ),
                },
                None => "CALIBRATE / WAITING FOR LEDGER\n\nNo persisted fusion beliefs are available to score against the supplied labels.".to_string(),
            },
        },
    }
}

fn format_bluetooth_value(cluster_count: Option<f64>, mean_rssi: Option<f64>) -> String {
    match cluster_count {
        Some(0.0) => "bluetooth       0 clusters / no advertisers observed".to_string(),
        Some(count) => format!(
            "bluetooth       {count:>3.0} clusters / mean {:>5.1} dBm",
            mean_rssi.unwrap_or(0.0)
        ),
        None => "bluetooth       --".to_string(),
    }
}

fn format_observation_ages(timestamps: &std::collections::BTreeMap<String, i64>) -> String {
    let Some(now_us) = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_micros()).ok())
    else {
        return "clock unavailable".to_string();
    };
    if timestamps.is_empty() {
        return "none".to_string();
    }
    timestamps
        .iter()
        .map(|(stream, timestamp_us)| {
            let age_seconds = (now_us - timestamp_us).max(0) / 1_000_000;
            format!("{stream} {age_seconds}s")
        })
        .collect::<Vec<_>>()
        .join("  ")
}

fn main() -> io::Result<()> {
    enable_tui_colors();
    let mut demo_only = false;
    let mut demo_frame_limit = None;
    let mut labels_path = None;
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--demo" => demo_only = true,
            "--demo-frames" => {
                let frames = arguments.next().ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "--demo-frames requires a number",
                    )
                })?;
                let frames = frames.parse::<u32>().map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "--demo-frames requires a positive number",
                    )
                })?;
                if frames == 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "--demo-frames requires a positive number",
                    ));
                }
                demo_frame_limit = Some(frames);
            }
            "--labels" => {
                labels_path = Some(
                    arguments
                        .next()
                        .ok_or_else(|| {
                            io::Error::new(io::ErrorKind::InvalidInput, "--labels requires a path")
                        })?
                        .into(),
                );
            }
            _ => {
                eprintln!(
                    "unknown argument: {argument}\nusage: liminal-tui [--demo] [--demo-frames N] [--labels PATH]"
                );
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "unknown argument",
                ));
            }
        }
    }
    validate_demo_frame_options(demo_only, demo_frame_limit)?;

    enable_raw_mode()?;
    let _terminal_guard = TerminalGuard;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let (image_worker_tx, image_result_rx) = spawn_image_worker();
    let mut app = App::new(
        demo_only,
        demo_frame_limit,
        labels_path,
        image_worker_tx,
        image_result_rx,
    )?;
    let tick_rate = Duration::from_millis(200);
    let mut last_tick = Instant::now();

    loop {
        app.poll_image();
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
                    if app.demo_only {
                        "DEMO MODE"
                    } else if app.ledger_error.is_some() {
                        "LEDGER ERROR"
                    } else if app.snapshot.is_some() {
                        "LEDGER ONLINE"
                    } else {
                        "WAITING FOR LEDGER"
                    }
                ),
                Style::default()
                    .fg(if app.demo_only || app.snapshot.is_some() {
                        TEAL
                    } else {
                        AMBER
                    })
                    .add_modifier(Modifier::BOLD),
            ))];
            if app.demo_only {
                status_lines.push(Line::from("SYNTHETIC BITMAP"));
                status_lines.push(Line::from(Span::styled(
                    "LEDGER READ  OFF",
                    Style::default().fg(MUTED),
                )));
                status_lines.push(Line::from(Span::styled(
                    "Telemetry is intentionally disabled",
                    Style::default().fg(MUTED),
                )));
            } else if let Some(error) = &app.ledger_error {
                status_lines.push(Line::from("LEDGER READ  FAILED"));
                status_lines.push(Line::from(Span::styled(error, Style::default().fg(AMBER))));
            } else if let Some(snapshot) = &app.snapshot {
                let rates = extract_recent_observation_rates(&snapshot.recent_observations);
                status_lines.push(Line::from(format!(
                    "EVENTS  {:>8}",
                    snapshot.total_event_count
                )));
                for (stream, count) in &snapshot.stream_event_counts {
                    let rate = rates.get(stream).copied().unwrap_or(0.0);
                    status_lines.push(Line::from(format!("{stream:<8} {count:>6} {rate:>5.1}/s")));
                }
                // Keep the health row visible in the smallest supported terminal. The newest
                // record and its provenance are available in MEMORY; spending this row on a
                // duplicate label would hide pending sensor gaps, which are more important to
                // the operator's safety/readiness judgment.
                status_lines.push(Line::from(format!(
                    "GAPS {:>4}  PENDING {:<12}  BELIEFS {:>4}  AGENTS {:>3}",
                    snapshot.sensor_gap_count,
                    if snapshot.pending_gap_streams.is_empty() {
                        "none".to_string()
                    } else {
                        snapshot.pending_gap_streams.join(",")
                    },
                    snapshot.belief_count,
                    snapshot.agent_run_count
                )));
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

            let body = Paragraph::new(mode_body(
                app.mode,
                &app.snapshot,
                app.memory_window,
                app.history_cursor,
                app.vision_enabled,
                app.demo_only,
                app.ledger_error.as_deref(),
                app.labels_path.as_deref(),
            ))
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
            // Queue resize/encoding without blocking the render loop. Keep the last completed
            // protocol visible while a newer telemetry frame is prepared in the worker.
            app.queue_image(inner);
            if let Some(image_state) = app.image_state.as_mut() {
                if app.image_area == Some(inner) {
                    let image_widget =
                        StatefulImage::default().resize(Resize::Scale(Some(FilterType::Lanczos3)));
                    frame.render_stateful_widget(image_widget, inner, image_state);
                } else {
                    // A terminal resize is handled by the worker on the next frame. Render the
                    // already-encoded image directly here so the UI thread never resizes it.
                    image_state.render(inner, frame.buffer_mut());
                }
            } else {
                frame.render_widget(
                    Paragraph::new("Preparing spectral field…")
                        .style(Style::default().fg(MUTED).bg(BG)),
                    inner,
                );
            }

            let footer = Paragraph::new(Line::from(vec![
                Span::styled(
                    " TAB ",
                    Style::default()
                        .fg(BG)
                        .bg(TEAL)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" switch mode   ", Style::default().fg(MUTED)),
                Span::styled("p", Style::default().fg(AMBER).add_modifier(Modifier::BOLD)),
                Span::styled(
                    format!(" {}   ", if app.paused { "resume" } else { "pause" }),
                    Style::default().fg(MUTED),
                ),
                Span::styled("v", Style::default().fg(AMBER).add_modifier(Modifier::BOLD)),
                Span::styled(
                    format!(
                        " vision {}   ",
                        if app.vision_enabled { "on" } else { "off" }
                    ),
                    Style::default().fg(MUTED),
                ),
                Span::styled(
                    "[ ]",
                    Style::default().fg(AMBER).add_modifier(Modifier::BOLD),
                ),
                Span::styled(" memory window   ", Style::default().fg(MUTED)),
                Span::styled(
                    "j/k",
                    Style::default().fg(AMBER).add_modifier(Modifier::BOLD),
                ),
                Span::styled(" history   ", Style::default().fg(MUTED)),
                Span::styled(
                    "1-6",
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

        if app.demo_frame_limit.is_some_and(|limit| app.tick >= limit) {
            break;
        }

        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press && apply_key(&mut app, key.code) {
                    break;
                }
            }
        }

        app.poll_snapshot();
        app.request_snapshot();

        if !app.paused && last_tick.elapsed() >= tick_rate {
            app.advance_visual();
            last_tick = Instant::now();
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_windows_cycle_through_short_and_long_history() {
        assert_eq!(next_memory_window(24), 96);
        assert_eq!(next_memory_window(4096), 4096);
        assert_eq!(previous_memory_window(1024), 256);
        assert_eq!(previous_memory_window(24), 24);
    }

    #[test]
    fn kitty_uses_a_higher_fidelity_source_than_halfblocks() {
        let halfblocks = Picker::from_fontsize((8, 16));
        assert_eq!(
            image_dimensions(halfblocks),
            (HALFBLOCK_IMAGE_WIDTH, HALFBLOCK_IMAGE_HEIGHT)
        );

        let mut kitty = halfblocks;
        kitty.set_protocol_type(ProtocolType::Kitty);
        assert_eq!(
            image_dimensions(kitty),
            (KITTY_IMAGE_WIDTH, KITTY_IMAGE_HEIGHT)
        );
        const {
            assert!(KITTY_IMAGE_WIDTH > HALFBLOCK_IMAGE_WIDTH);
            assert!(KITTY_IMAGE_HEIGHT > HALFBLOCK_IMAGE_HEIGHT);
        }
    }

    #[test]
    fn image_worker_returns_a_completed_protocol() {
        let (image_worker_tx, image_result_rx) = spawn_image_worker();
        let picker = Picker::from_fontsize((8, 16));
        let image = DynamicImage::ImageRgb8(telemetry_frame::spectral_frame(
            HALFBLOCK_IMAGE_WIDTH,
            HALFBLOCK_IMAGE_HEIGHT,
            0,
            &demo_telemetry(),
        ));
        image_worker_tx
            .send((
                picker.new_resize_protocol(image),
                Resize::Scale(Some(FilterType::Lanczos3)),
                ratatui::layout::Rect::new(0, 0, 114, 58),
            ))
            .expect("image worker should accept a job");

        assert!(image_result_rx
            .recv_timeout(Duration::from_secs(10))
            .is_ok());
    }

    #[test]
    fn queued_image_becomes_visible_after_worker_completion() {
        let (image_worker_tx, image_result_rx) = spawn_image_worker();
        let (snapshot_tx, snapshot_rx) = mpsc::channel();
        let mut app = App {
            mode: Mode::Spectral,
            tick: 0,
            picker: Picker::from_fontsize((8, 16)),
            image_state: None,
            image_worker_tx,
            image_result_rx,
            pending_image: Some(DynamicImage::ImageRgb8(telemetry_frame::spectral_frame(
                32,
                32,
                0,
                &demo_telemetry(),
            ))),
            image_job_in_flight: false,
            image_area: None,
            image_title: DEMO_IMAGE_TITLE,
            snapshot: None,
            ledger_error: None,
            memory_window: 24,
            history_cursor: 0,
            paused: false,
            vision_enabled: true,
            demo_only: true,
            demo_frame_limit: None,
            labels_path: None,
            snapshot_tx,
            snapshot_rx,
            snapshot_in_flight: false,
            last_snapshot_request: None,
        };
        let area = ratatui::layout::Rect::new(0, 0, 20, 10);

        app.queue_image(area);
        assert!(app.image_job_in_flight);
        for _ in 0..200 {
            app.poll_image();
            if app.image_state.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(app.image_state.is_some());
        assert_eq!(app.image_area, Some(area));
        assert!(!app.image_job_in_flight);
    }

    #[test]
    fn demo_frame_options_fail_before_terminal_setup_when_not_in_demo_mode() {
        assert!(validate_demo_frame_options(false, Some(1)).is_err());
        assert!(validate_demo_frame_options(true, Some(1)).is_ok());
    }

    #[test]
    fn latest_record_formatter_keeps_provenance_drilldown_visible() {
        let record = LatestRecord {
            id: "belief-7".to_string(),
            kind: "belief".to_string(),
            stream: Some("fusion".to_string()),
            timestamp_us: Some(42),
            provenance_sources: vec!["camera-3".to_string(), "audio-4".to_string()],
        };
        let rendered = format_latest_record(Some(&record));
        assert!(rendered.contains("belief-7 / belief / stream fusion / ts_us 42"));
        assert!(rendered.contains("camera-3, audio-4"));
        assert_eq!(format_sources_inline(Some(&record)), "camera-3, audio-4");
    }

    #[test]
    fn operator_keys_select_modes_and_memory_windows_without_side_effects() {
        let (snapshot_tx, snapshot_rx) = mpsc::channel();
        let (image_worker_tx, _image_worker_rx) = mpsc::channel();
        let (_image_result_tx, image_result_rx) = mpsc::channel();
        let mut app = App {
            mode: Mode::Spectral,
            tick: 0,
            picker: Picker::from_fontsize((8, 16)),
            image_state: None,
            image_worker_tx,
            image_result_rx,
            pending_image: None,
            image_job_in_flight: false,
            image_area: None,
            image_title: DEMO_IMAGE_TITLE,
            snapshot: None,
            ledger_error: None,
            memory_window: 24,
            history_cursor: 0,
            paused: false,
            vision_enabled: true,
            demo_only: false,
            demo_frame_limit: None,
            labels_path: None,
            snapshot_tx,
            snapshot_rx,
            snapshot_in_flight: false,
            last_snapshot_request: None,
        };

        assert!(!apply_key(&mut app, KeyCode::Char('2')));
        assert_eq!(app.mode, Mode::Belief);
        assert!(!apply_key(&mut app, KeyCode::Char('6')));
        assert_eq!(app.mode, Mode::Calibration);
        assert!(!apply_key(&mut app, KeyCode::Char('2')));
        assert_eq!(app.mode, Mode::Belief);
        assert!(!apply_key(&mut app, KeyCode::Char(']')));
        assert_eq!(app.memory_window, 96);
        assert!(!apply_key(&mut app, KeyCode::Char('p')));
        assert!(app.paused);
        assert!(!apply_key(&mut app, KeyCode::Char('v')));
        assert!(!app.vision_enabled);
        assert!(!apply_key(&mut app, KeyCode::BackTab));
        assert_eq!(app.mode, Mode::Spectral);
        assert!(apply_key(&mut app, KeyCode::Char('q')));
    }

    #[test]
    fn demo_mode_never_reads_or_displays_ledger_data() {
        let (snapshot_tx, snapshot_rx) = mpsc::channel();
        let (image_worker_tx, _image_worker_rx) = mpsc::channel();
        let (_image_result_tx, image_result_rx) = mpsc::channel();
        let mut app = App {
            mode: Mode::Spectral,
            tick: 0,
            picker: Picker::from_fontsize((8, 16)),
            image_state: None,
            image_worker_tx,
            image_result_rx,
            pending_image: None,
            image_job_in_flight: false,
            image_area: None,
            image_title: DEMO_IMAGE_TITLE,
            snapshot: None,
            ledger_error: None,
            memory_window: 24,
            history_cursor: 0,
            paused: false,
            vision_enabled: true,
            demo_only: true,
            demo_frame_limit: None,
            labels_path: None,
            snapshot_tx,
            snapshot_rx,
            snapshot_in_flight: false,
            last_snapshot_request: None,
        };

        app.advance_visual();

        assert!(app.snapshot.is_none());
        assert_eq!(app.image_title, DEMO_IMAGE_TITLE);
        assert!(
            mode_body(Mode::Spectral, &app.snapshot, 24, 0, true, true, None, None)
                .contains("DEMO MODE")
        );
    }

    #[test]
    fn bluetooth_zero_is_explained_as_no_advertisers_not_sensor_failure() {
        assert_eq!(
            format_bluetooth_value(Some(0.0), None),
            "bluetooth       0 clusters / no advertisers observed"
        );
        assert_eq!(
            format_bluetooth_value(Some(2.0), Some(-50.0)),
            "bluetooth         2 clusters / mean -50.0 dBm"
        );
        assert_eq!(format_bluetooth_value(None, None), "bluetooth       --");
    }

    #[test]
    fn ledger_errors_are_visible_and_never_presented_as_demo_data() {
        let body = mode_body(
            Mode::Spectral,
            &None,
            24,
            0,
            true,
            false,
            Some("database integrity check failed"),
            None,
        );

        assert!(body.contains("LEDGER ERROR"));
        assert!(body.contains("database integrity check failed"));
        assert!(body.contains("Synthetic demo rendering is disabled"));
    }

    #[test]
    fn calibration_mode_refuses_to_imply_ground_truth_without_labels() {
        let body = mode_body(Mode::Calibration, &None, 24, 0, true, false, None, None);
        assert!(body.contains("NO LABEL FILE"));
        assert!(body.contains("No human or approved reference labels were supplied."));
        assert!(body.contains("will not treat sensor output as ground truth"));
    }
}
