use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc as SyncArc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use echo_core::SessionState;
use x11rb::connection::Connection;
use x11rb::protocol::shape::{self, SK, SO};
use x11rb::protocol::xproto::{
    Arc as XArc, AtomEnum, ChangeWindowAttributesAux, ConnectionExt, CreateGCAux, CreateWindowAux,
    EventMask, PropMode, Rectangle, WindowClass,
};
use x11rb::COPY_DEPTH_FROM_PARENT;

use super::waveform::{sine_rms_fixture, RmsRing};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    BottomCenter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HudConfig {
    pub enabled: bool,
    pub anchor: Anchor,
}

impl Default for HudConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            anchor: Anchor::BottomCenter,
        }
    }
}

/// Projection of the session plus RMS samples. Does not own the session.
#[derive(Debug, Clone)]
pub struct HudState {
    pub session: SessionState,
    pub rms: RmsRing,
}

impl HudState {
    #[must_use]
    pub fn from_session(session: SessionState) -> Self {
        Self {
            session,
            rms: RmsRing::new(48),
        }
    }

    #[must_use]
    pub fn visible(&self) -> bool {
        !matches!(self.session, SessionState::Idle)
    }
}

#[derive(Debug)]
pub enum HudError {
    Display(String),
}

impl std::fmt::Display for HudError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Display(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for HudError {}

pub struct RecordingHud {
    running: Option<SyncArc<AtomicBool>>,
    worker: Option<JoinHandle<()>>,
}

impl RecordingHud {
    #[must_use]
    pub fn start() -> Self {
        if hud_disabled()
            || std::env::var_os("DISPLAY").is_none()
            || std::env::var_os("ECHO_AUDIO_FIXTURE").is_some()
        {
            return Self {
                running: None,
                worker: None,
            };
        }
        let running = SyncArc::new(AtomicBool::new(true));
        let worker_running = SyncArc::clone(&running);
        let worker = std::thread::spawn(move || {
            if let Err(err) = show_recording(&worker_running) {
                eprintln!("recording HUD: {err}");
            }
        });
        Self {
            running: Some(running),
            worker: Some(worker),
        }
    }
}

impl Drop for RecordingHud {
    fn drop(&mut self) {
        if let Some(running) = &self.running {
            running.store(false, Ordering::SeqCst);
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn hud_disabled() -> bool {
    matches!(
        std::env::var("ECHO_HUD").ok().as_deref(),
        Some("0") | Some("false") | Some("off")
    )
}

/// Whether `ECHO_HUD` leaves the capsule enabled. The HUD additionally needs
/// an X11 display at record time.
#[must_use]
pub fn enabled() -> bool {
    !hud_disabled()
}

pub fn run_hud_demo() -> Result<(), HudError> {
    let mut state = HudState::from_session(SessionState::Recording {
        started: Instant::now(),
    });
    for sample in sine_rms_fixture(24, 3.0) {
        state.rms.push(sample);
    }
    show(&state, &HudConfig::default(), Duration::from_millis(250))
}

pub fn show(state: &HudState, config: &HudConfig, hold: Duration) -> Result<(), HudError> {
    if !config.enabled || !state.visible() {
        return Ok(());
    }
    let (conn, screen_num) =
        x11rb::connect(None).map_err(|err| HudError::Display(err.to_string()))?;
    let screen = &conn.setup().roots[screen_num];
    let width = 280u16;
    let height = 56u16;
    let (x, y) = match config.anchor {
        Anchor::BottomCenter => {
            let x = (i32::from(screen.width_in_pixels) - i32::from(width)) / 2;
            let y = i32::from(screen.height_in_pixels) - i32::from(height) - 48;
            (x as i16, y as i16)
        }
    };
    let win = conn
        .generate_id()
        .map_err(|err| HudError::Display(err.to_string()))?;
    let aux = CreateWindowAux::new()
        .background_pixel(0x0018_1b20)
        .border_pixel(screen.black_pixel)
        .override_redirect(1)
        .event_mask(EventMask::EXPOSURE);
    conn.create_window(
        COPY_DEPTH_FROM_PARENT,
        win,
        screen.root,
        x,
        y,
        width,
        height,
        0,
        WindowClass::INPUT_OUTPUT,
        0,
        &aux,
    )
    .map_err(|err| HudError::Display(err.to_string()))?;
    let title = b"Echo Recording";
    conn.change_property(
        PropMode::REPLACE,
        win,
        AtomEnum::WM_NAME,
        AtomEnum::STRING,
        8,
        u32::try_from(title.len()).unwrap_or(0),
        title,
    )
    .map_err(|err| HudError::Display(err.to_string()))?;
    set_above(&conn, win, screen.root)?;
    let _ = shape::rectangles(
        &conn,
        SO::SET,
        SK::INPUT,
        x11rb::protocol::xproto::ClipOrdering::UNSORTED,
        win,
        0,
        0,
        &[],
    );
    conn.map_window(win)
        .map_err(|err| HudError::Display(err.to_string()))?;
    draw_bars(&conn, win, width, height, &state.rms.bars(24))?;
    conn.flush()
        .map_err(|err| HudError::Display(err.to_string()))?;
    std::thread::sleep(hold);
    conn.destroy_window(win)
        .map_err(|err| HudError::Display(err.to_string()))?;
    conn.flush()
        .map_err(|err| HudError::Display(err.to_string()))?;
    Ok(())
}

fn show_recording(running: &AtomicBool) -> Result<(), HudError> {
    const WIDTH: u16 = 220;
    const HEIGHT: u16 = 56;
    const FRAME: Duration = Duration::from_millis(33);

    let (conn, screen_num) =
        x11rb::connect(None).map_err(|err| HudError::Display(err.to_string()))?;
    let screen = &conn.setup().roots[screen_num];
    let x = ((i32::from(screen.width_in_pixels) - i32::from(WIDTH)) / 2) as i16;
    let y = (i32::from(screen.height_in_pixels) - i32::from(HEIGHT) - 48) as i16;
    let win = conn
        .generate_id()
        .map_err(|err| HudError::Display(err.to_string()))?;
    conn.create_window(
        COPY_DEPTH_FROM_PARENT,
        win,
        screen.root,
        x,
        y,
        WIDTH,
        HEIGHT,
        0,
        WindowClass::INPUT_OUTPUT,
        0,
        &CreateWindowAux::new()
            .background_pixel(0x0014_1821)
            .border_pixel(0x0014_1821)
            .override_redirect(1)
            .event_mask(EventMask::EXPOSURE),
    )
    .map_err(|err| HudError::Display(err.to_string()))?;
    set_above(&conn, win, screen.root)?;
    set_capsule_shape(&conn, win, WIDTH, HEIGHT)?;
    let _ = shape::rectangles(
        &conn,
        SO::SET,
        SK::INPUT,
        x11rb::protocol::xproto::ClipOrdering::UNSORTED,
        win,
        0,
        0,
        &[],
    );
    conn.map_window(win)
        .map_err(|err| HudError::Display(err.to_string()))?;
    conn.flush()
        .map_err(|err| HudError::Display(err.to_string()))?;

    let started = Instant::now();
    while running.load(Ordering::SeqCst) {
        draw_recording_frame(&conn, win, WIDTH, HEIGHT, started.elapsed().as_secs_f32())?;
        conn.flush()
            .map_err(|err| HudError::Display(err.to_string()))?;
        std::thread::sleep(FRAME);
    }
    conn.destroy_window(win)
        .map_err(|err| HudError::Display(err.to_string()))?;
    conn.flush()
        .map_err(|err| HudError::Display(err.to_string()))?;
    Ok(())
}

fn set_capsule_shape<C: Connection>(
    conn: &C,
    win: u32,
    width: u16,
    height: u16,
) -> Result<(), HudError> {
    let radius = f32::from(height) / 2.0;
    let rows: Vec<Rectangle> = (0..height)
        .map(|row| {
            let dy = (f32::from(row) + 0.5 - radius).abs();
            let inset = (radius - (radius * radius - dy * dy).max(0.0).sqrt()).round() as i16;
            Rectangle {
                x: inset,
                y: row as i16,
                width: width.saturating_sub((inset.max(0) as u16).saturating_mul(2)),
                height: 1,
            }
        })
        .collect();
    shape::rectangles(
        conn,
        SO::SET,
        SK::BOUNDING,
        x11rb::protocol::xproto::ClipOrdering::UNSORTED,
        win,
        0,
        0,
        &rows,
    )
    .map_err(|err| HudError::Display(err.to_string()))?;
    Ok(())
}

fn draw_recording_frame<C: Connection>(
    conn: &C,
    win: u32,
    width: u16,
    height: u16,
    elapsed: f32,
) -> Result<(), HudError> {
    const BG: u32 = 0x0014_1821;
    const RED: u32 = 0x00ff_5a67;
    const RED_DARK: u32 = 0x006f_2933;
    const CYAN: u32 = 0x006e_c1e4;
    const CYAN_DIM: u32 = 0x0034_7086;

    conn.change_window_attributes(win, &ChangeWindowAttributesAux::new().background_pixel(BG))
        .map_err(|err| HudError::Display(err.to_string()))?;
    conn.clear_area(false, win, 0, 0, width, height)
        .map_err(|err| HudError::Display(err.to_string()))?;

    let red_gc = create_color_gc(conn, win, RED)?;
    let red_dark_gc = create_color_gc(conn, win, RED_DARK)?;
    let cyan_gc = create_color_gc(conn, win, CYAN)?;
    let cyan_dim_gc = create_color_gc(conn, win, CYAN_DIM)?;

    let pulse = ((elapsed * 4.4).sin() * 0.5 + 0.5).clamp(0.0, 1.0);
    let outer = (14.0 + pulse * 4.0).round() as u16;
    let inner = 10u16;
    conn.poly_fill_arc(
        win,
        red_dark_gc,
        &[XArc {
            x: 28 - (outer / 2) as i16,
            y: 28 - (outer / 2) as i16,
            width: outer,
            height: outer,
            angle1: 0,
            angle2: 360 * 64,
        }],
    )
    .map_err(|err| HudError::Display(err.to_string()))?;
    conn.poly_fill_arc(
        win,
        red_gc,
        &[XArc {
            x: 28 - (inner / 2) as i16,
            y: 28 - (inner / 2) as i16,
            width: inner,
            height: inner,
            angle1: 0,
            angle2: 360 * 64,
        }],
    )
    .map_err(|err| HudError::Display(err.to_string()))?;

    let center_y = i32::from(height) / 2;
    for index in 0..13i32 {
        let phase = elapsed * 6.0 + index as f32 * 0.72;
        let carrier = (phase.sin() * 0.5 + 0.5).powf(1.4);
        let envelope = 1.0 - ((index as f32 - 6.0).abs() / 8.5).min(0.72);
        let bar_height = (6.0 + carrier * envelope * 28.0).round() as i32;
        let x = 52 + index * 9;
        let gc = if index % 3 == 0 { cyan_dim_gc } else { cyan_gc };
        conn.poly_fill_rectangle(
            win,
            gc,
            &[Rectangle {
                x: x as i16,
                y: (center_y - bar_height / 2) as i16,
                width: 5,
                height: bar_height as u16,
            }],
        )
        .map_err(|err| HudError::Display(err.to_string()))?;
    }

    for gc in [red_gc, red_dark_gc, cyan_gc, cyan_dim_gc] {
        conn.free_gc(gc)
            .map_err(|err| HudError::Display(err.to_string()))?;
    }
    Ok(())
}

fn create_color_gc<C: Connection>(conn: &C, win: u32, color: u32) -> Result<u32, HudError> {
    let gc = conn
        .generate_id()
        .map_err(|err| HudError::Display(err.to_string()))?;
    conn.create_gc(gc, win, &CreateGCAux::new().foreground(color))
        .map_err(|err| HudError::Display(err.to_string()))?;
    Ok(gc)
}

fn set_above<C: Connection>(conn: &C, win: u32, root: u32) -> Result<(), HudError> {
    let net_state: u32 = conn
        .intern_atom(false, b"_NET_WM_STATE")
        .map_err(|err| HudError::Display(err.to_string()))?
        .reply()
        .map_err(|err| HudError::Display(err.to_string()))?
        .atom;
    let above: u32 = conn
        .intern_atom(false, b"_NET_WM_STATE_ABOVE")
        .map_err(|err| HudError::Display(err.to_string()))?
        .reply()
        .map_err(|err| HudError::Display(err.to_string()))?
        .atom;
    conn.change_property(
        PropMode::REPLACE,
        win,
        net_state,
        AtomEnum::ATOM,
        32,
        1,
        &above.to_le_bytes(),
    )
    .map_err(|err| HudError::Display(err.to_string()))?;
    let _ = root;
    Ok(())
}

fn draw_bars<C: Connection>(
    conn: &C,
    win: u32,
    width: u16,
    height: u16,
    bars: &[f32],
) -> Result<(), HudError> {
    let gc = conn
        .generate_id()
        .map_err(|err| HudError::Display(err.to_string()))?;
    conn.create_gc(
        gc,
        win,
        &CreateGCAux::new()
            .foreground(0x006e_c1e4)
            .background(0x0018_1b20),
    )
    .map_err(|err| HudError::Display(err.to_string()))?;
    conn.change_window_attributes(
        win,
        &ChangeWindowAttributesAux::new().background_pixel(0x0018_1b20),
    )
    .map_err(|err| HudError::Display(err.to_string()))?;
    conn.clear_area(false, win, 0, 0, width, height)
        .map_err(|err| HudError::Display(err.to_string()))?;
    if bars.is_empty() {
        return Ok(());
    }
    let gap = 2i16;
    let bar_w = ((i32::from(width) - 16) / bars.len() as i32).max(2) as u16;
    for (i, level) in bars.iter().enumerate() {
        let h = ((f32::from(height - 8) * level).round() as i32).clamp(2, i32::from(height) - 4);
        let x = 8 + i as i16 * (bar_w as i16 + gap);
        let y = i16::try_from(i32::from(height) - 4 - h).unwrap_or(0);
        conn.poly_fill_rectangle(
            win,
            gc,
            &[x11rb::protocol::xproto::Rectangle {
                x,
                y,
                width: bar_w,
                height: h as u16,
            }],
        )
        .map_err(|err| HudError::Display(err.to_string()))?;
    }
    Ok(())
}
