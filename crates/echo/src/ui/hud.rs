use std::time::{Duration, Instant};

use echo_core::SessionState;
use x11rb::connection::Connection;
use x11rb::protocol::shape::{self, SK, SO};
use x11rb::protocol::xproto::{
    AtomEnum, ChangeWindowAttributesAux, ConnectionExt, CreateGCAux, CreateWindowAux, EventMask,
    PropMode, WindowClass,
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
