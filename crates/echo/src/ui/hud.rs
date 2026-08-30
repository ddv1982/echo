use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc as SyncArc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::audio::LevelMeter;
use x11rb::connection::Connection;
use x11rb::protocol::shape::{self, SK, SO};
use x11rb::protocol::xproto::{
    AtomEnum, ColormapAlloc, ConnectionExt, CreateGCAux, CreateWindowAux, EventMask, PropMode,
    Rectangle, VisualClass, WindowClass,
};

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

/// The session states the capsule renders. Set from the session machine in
/// rec.rs; the HUD thread reads it each frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum HudState {
    Recording,
    Transcribing,
    Done,
    Failed,
}

impl HudState {
    fn from_bits(bits: u8) -> Self {
        match bits {
            1 => Self::Transcribing,
            2 => Self::Done,
            3 => Self::Failed,
            _ => Self::Recording,
        }
    }

    fn bits(self) -> u8 {
        match self {
            Self::Recording => 0,
            Self::Transcribing => 1,
            Self::Done => 2,
            Self::Failed => 3,
        }
    }
}

/// The recording capsule: an always-on-top, click-through X11 window drawn
/// from real microphone levels. Lives from the start of capture until after
/// injection, so the longest wait in the session has an indicator.
pub struct RecordingHud {
    running: Option<SyncArc<AtomicBool>>,
    state: Option<SyncArc<AtomicU8>>,
    worker: Option<JoinHandle<()>>,
}

impl RecordingHud {
    #[must_use]
    pub fn start(level: LevelMeter) -> Self {
        if hud_disabled() || std::env::var_os("DISPLAY").is_none() {
            return Self {
                running: None,
                state: None,
                worker: None,
            };
        }
        let running = SyncArc::new(AtomicBool::new(true));
        let state = SyncArc::new(AtomicU8::new(HudState::Recording.bits()));
        let worker_running = SyncArc::clone(&running);
        let worker_state = SyncArc::clone(&state);
        let worker = std::thread::spawn(move || {
            if let Err(err) = run(&worker_running, &worker_state, &level) {
                eprintln!("recording HUD: {err}");
            }
        });
        Self {
            running: Some(running),
            state: Some(state),
            worker: Some(worker),
        }
    }

    /// A HUD that never opened (no display, disabled) ignores state changes.
    pub fn set_state(&self, state: HudState) {
        if let Some(shared) = &self.state {
            shared.store(state.bits(), Ordering::SeqCst);
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

fn hud_is_disabled(env: Option<&str>, file: &echo_core::Config) -> bool {
    match env {
        Some("0" | "false" | "off") => true,
        Some("1" | "true" | "on") => false,
        _ => file.hud == Some(false),
    }
}

fn hud_disabled() -> bool {
    hud_is_disabled(
        std::env::var("ECHO_HUD").ok().as_deref(),
        &crate::settings::file_config(),
    )
}

/// Whether `ECHO_HUD` and the config file leave the capsule enabled. The HUD
/// additionally needs an X11 display at record time.
#[must_use]
pub fn enabled() -> bool {
    !hud_disabled()
}

const WIDTH: u32 = 152;
const HEIGHT: u32 = 44;
const FRAME: Duration = Duration::from_millis(33);
const BAR_COUNT: usize = 11;
/// Done holds briefly, then the capsule fades. Failed holds about a second.
const DONE_HOLD: Duration = Duration::from_millis(300);
const DONE_FADE: Duration = Duration::from_millis(200);
const FAILED_HOLD: Duration = Duration::from_millis(1000);

/// Fast attack, slow release, the broadcast-meter behavior open-wispr and
/// sflow publish: rising levels close most of the gap in one frame, falling
/// levels retain 0.80 per frame. A dead mic reads as a flat line, which is
/// the diagnostic a fake animation hides.
const ATTACK: f32 = 0.65;
const RELEASE: f32 = 0.80;

fn smooth(displayed: f32, measured: f32) -> f32 {
    if measured > displayed {
        displayed + (measured - displayed) * ATTACK
    } else {
        displayed * RELEASE + measured * (1.0 - RELEASE)
    }
}

const BAR_MIN: f32 = 3.0;
const BAR_MAX: f32 = 26.0;
/// Speech RMS sits well under 1.0; gain brings conversational levels to
/// mid-scale, and the square root keeps quiet sounds visible.
const LEVEL_GAIN: f32 = 3.0;

fn bar_height(level: f32) -> f32 {
    let normalized = (level * LEVEL_GAIN).clamp(0.0, 1.0);
    BAR_MIN + (BAR_MAX - BAR_MIN) * normalized.sqrt()
}

/// One frame of the capsule, premultiplied RGBA. Reused across frames.
struct FrameBuffer {
    width: u32,
    height: u32,
    pixmap: tiny_skia::Pixmap,
}

impl FrameBuffer {
    fn new(width: u32, height: u32) -> Option<Self> {
        tiny_skia::Pixmap::new(width, height).map(|pixmap| Self {
            width,
            height,
            pixmap,
        })
    }
}

// Paired with the dark-theme tokens in frontend/src/styles/tokens.css:
// --surface-card 0 0% 7%, --recording 354 100% 67%, --text-secondary 0 0% 70%.
// X11 cannot read CSS, so update both places together.
const BG: (u8, u8, u8) = (0x12, 0x12, 0x12);
const RED: (u8, u8, u8) = (0xff, 0x57, 0x68);
const GRAY: (u8, u8, u8) = (0xb3, 0xb3, 0xb3);

fn color((r, g, b): (u8, u8, u8), alpha: f32) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba(
        f32::from(r) / 255.0,
        f32::from(g) / 255.0,
        f32::from(b) / 255.0,
        alpha.clamp(0.0, 1.0),
    )
    .unwrap_or(tiny_skia::Color::BLACK)
}

fn rounded_rect(x: f32, y: f32, width: f32, height: f32, radius: f32) -> tiny_skia::Path {
    let radius = radius.min(width / 2.0).min(height / 2.0);
    let (x0, y0, x1, y1) = (x, y, x + width, y + height);
    let mut builder = tiny_skia::PathBuilder::new();
    builder.move_to(x0 + radius, y0);
    builder.line_to(x1 - radius, y0);
    builder.quad_to(x1, y0, x1, y0 + radius);
    builder.line_to(x1, y1 - radius);
    builder.quad_to(x1, y1, x1 - radius, y1);
    builder.line_to(x0 + radius, y1);
    builder.quad_to(x0, y1, x0, y1 - radius);
    builder.line_to(x0, y0 + radius);
    builder.quad_to(x0, y0, x0 + radius, y0);
    builder.close();
    builder.finish().unwrap_or_else(|| {
        tiny_skia::PathBuilder::from_rect(tiny_skia::Rect::from_xywh(0.0, 0.0, 1.0, 1.0).unwrap())
    })
}

fn fill(frame: &mut FrameBuffer, path: &tiny_skia::Path, rgb: (u8, u8, u8), alpha: f32) {
    let mut paint = tiny_skia::Paint::default();
    paint.set_color(color(rgb, alpha));
    paint.anti_alias = true;
    frame.pixmap.fill_path(
        path,
        &paint,
        tiny_skia::FillRule::Winding,
        tiny_skia::Transform::identity(),
        None,
    );
}

/// Draw one frame. `translucent` selects the compositor path: per-pixel alpha
/// with a translucent capsule fill, a hairline border, and a soft glow behind
/// the dot. Without a compositor the same layout flattens onto an opaque
/// fill, because ARGB transparency would render black there.
fn render_frame(
    frame: &mut FrameBuffer,
    state: HudState,
    bars: &[f32],
    elapsed: f32,
    fade: f32,
    translucent: bool,
) {
    let width = frame.width as f32;
    let height = frame.height as f32;
    frame.pixmap.fill(tiny_skia::Color::TRANSPARENT);

    let capsule_alpha = if translucent { 0.82 * fade } else { fade };
    let capsule = rounded_rect(0.0, 0.0, width, height, height / 2.0);
    fill(frame, &capsule, BG, capsule_alpha);
    if translucent {
        let mut paint = tiny_skia::Paint::default();
        paint.set_color(color((0xff, 0xff, 0xff), 0.10 * fade));
        paint.anti_alias = true;
        frame.pixmap.stroke_path(
            &capsule,
            &paint,
            &tiny_skia::Stroke {
                width: 1.0,
                ..tiny_skia::Stroke::default()
            },
            tiny_skia::Transform::identity(),
            None,
        );
    }

    match state {
        HudState::Recording | HudState::Failed => {
            let pulse = if state == HudState::Recording {
                (elapsed * 4.4).sin() * 0.5 + 0.5
            } else {
                1.0
            };
            if translucent {
                let glow = 14.0 + pulse * 5.0;
                let glow_path =
                    rounded_rect(22.0 - glow / 2.0, 22.0 - glow / 2.0, glow, glow, glow / 2.0);
                fill(frame, &glow_path, RED, 0.18 * fade);
            }
            let dot = rounded_rect(18.0, 18.0, 8.0, 8.0, 4.0);
            fill(frame, &dot, RED, (0.55 + 0.45 * pulse) * fade);
            if state == HudState::Recording {
                draw_bars(frame, bars, fade);
            }
        }
        HudState::Transcribing => draw_shimmer_dots(frame, elapsed, fade),
        HudState::Done => draw_check(frame, fade),
    }
}

fn draw_bars(frame: &mut FrameBuffer, bars: &[f32], fade: f32) {
    let center_y = frame.height as f32 / 2.0;
    for (index, level) in bars.iter().enumerate() {
        let height = bar_height(*level);
        let x = 42.0 + index as f32 * 8.0;
        let bar = rounded_rect(x, center_y - height / 2.0, 4.0, height, 2.0);
        fill(frame, &bar, GRAY, fade);
    }
}

fn draw_shimmer_dots(frame: &mut FrameBuffer, elapsed: f32, fade: f32) {
    let center_y = frame.height as f32 / 2.0;
    for index in 0..3 {
        // A traveling shimmer: each dot brightens in turn, left to right.
        let phase = (elapsed * 2.2 - index as f32 * 0.33).rem_euclid(1.0);
        let brightness = 0.35 + 0.65 * (1.0 - (phase * 2.0 - 1.0).abs());
        let x = frame.width as f32 / 2.0 - 18.0 + index as f32 * 14.0;
        let dot = rounded_rect(x, center_y - 4.0, 8.0, 8.0, 4.0);
        fill(frame, &dot, GRAY, brightness * fade);
    }
}

fn draw_check(frame: &mut FrameBuffer, fade: f32) {
    let mut builder = tiny_skia::PathBuilder::new();
    let center_x = frame.width as f32 / 2.0;
    let center_y = frame.height as f32 / 2.0;
    builder.move_to(center_x - 9.0, center_y + 0.5);
    builder.line_to(center_x - 2.0, center_y + 7.0);
    builder.line_to(center_x + 10.0, center_y - 7.0);
    let Some(path) = builder.finish() else {
        return;
    };
    let mut paint = tiny_skia::Paint::default();
    paint.set_color(color(GRAY, fade));
    paint.anti_alias = true;
    frame.pixmap.stroke_path(
        &path,
        &paint,
        &tiny_skia::Stroke {
            width: 3.0,
            line_cap: tiny_skia::LineCap::Round,
            line_join: tiny_skia::LineJoin::Round,
            ..tiny_skia::Stroke::default()
        },
        tiny_skia::Transform::identity(),
        None,
    );
}

struct ArgbVisual {
    visual_id: u32,
    colormap: u32,
    red_mask: u32,
    green_mask: u32,
    blue_mask: u32,
}

/// The 32-bit TrueColor visual with an alpha channel, plus the colormap a
/// window using it needs. Meaningful only when a compositor owns the
/// `_NET_WM_CM_S0` selection; without one, transparent pixels render black.
fn argb_visual<C: Connection>(conn: &C, screen_num: usize) -> Result<Option<ArgbVisual>, HudError> {
    let compositor = conn
        .intern_atom(false, b"_NET_WM_CM_S0")
        .map_err(|err| HudError::Display(err.to_string()))?
        .reply()
        .map_err(|err| HudError::Display(err.to_string()))?
        .atom;
    let owner = conn
        .get_selection_owner(compositor)
        .map_err(|err| HudError::Display(err.to_string()))?
        .reply()
        .map_err(|err| HudError::Display(err.to_string()))?
        .owner;
    if owner == x11rb::NONE {
        return Ok(None);
    }
    let screen = &conn.setup().roots[screen_num];
    let found = screen
        .allowed_depths
        .iter()
        .find(|depth| depth.depth == 32)
        .and_then(|depth| {
            depth
                .visuals
                .iter()
                .find(|visual| visual.class == VisualClass::TRUE_COLOR)
        });
    let Some(visual) = found else {
        return Ok(None);
    };
    let colormap = conn
        .generate_id()
        .map_err(|err| HudError::Display(err.to_string()))?;
    conn.create_colormap(ColormapAlloc::NONE, colormap, screen.root, visual.visual_id)
        .map_err(|err| HudError::Display(err.to_string()))?;
    Ok(Some(ArgbVisual {
        visual_id: visual.visual_id,
        colormap,
        red_mask: visual.red_mask,
        green_mask: visual.green_mask,
        blue_mask: visual.blue_mask,
    }))
}

/// Pack one premultiplied RGBA pixel for the window's visual. The masks come
/// from the visual itself; the alpha byte is kept only on the 32-bit path.
fn pack_pixel(pixel: tiny_skia::PremultipliedColorU8, masks: (u32, u32, u32), alpha: bool) -> u32 {
    let (red, green, blue) = if alpha {
        (pixel.red(), pixel.green(), pixel.blue())
    } else {
        let color = pixel.demultiply();
        (color.red(), color.green(), color.blue())
    };
    let mut packed =
        shift_into(red, masks.0) | shift_into(green, masks.1) | shift_into(blue, masks.2);
    if alpha {
        packed |= shift_into(pixel.alpha(), !(masks.0 | masks.1 | masks.2));
    }
    packed
}

fn shift_into(channel: u8, mask: u32) -> u32 {
    if mask == 0 {
        return 0;
    }
    let shift = mask.trailing_zeros();
    let bits = (mask >> shift).count_ones();
    let max = (1u32 << bits) - 1;
    ((u32::from(channel) * max + 127) / 255) << shift
}

#[allow(clippy::too_many_lines)]
fn run(
    running: &SyncArc<AtomicBool>,
    shared_state: &SyncArc<AtomicU8>,
    meter: &LevelMeter,
) -> Result<(), HudError> {
    let (conn, screen_num) =
        x11rb::connect(None).map_err(|err| HudError::Display(err.to_string()))?;
    let argb = argb_visual(&conn, screen_num)?;
    let translucent = argb.is_some();
    let screen = &conn.setup().roots[screen_num];
    let root_masks = {
        let visual = screen
            .allowed_depths
            .iter()
            .flat_map(|depth| depth.visuals.iter())
            .find(|visual| visual.visual_id == screen.root_visual);
        visual
            .map(|visual| (visual.red_mask, visual.green_mask, visual.blue_mask))
            .unwrap_or((0xff0000, 0xff00, 0xff))
    };
    let x = ((i32::from(screen.width_in_pixels) - WIDTH as i32) / 2) as i16;
    let y = (i32::from(screen.height_in_pixels) - HEIGHT as i32 - 48) as i16;
    let win = conn
        .generate_id()
        .map_err(|err| HudError::Display(err.to_string()))?;
    let (depth, visual_id, colormap): (u8, u32, u32) = match &argb {
        Some(argb) => (32, argb.visual_id, argb.colormap),
        None => (0, 0, 0),
    };
    let mut aux = CreateWindowAux::new()
        .border_pixel(0)
        .override_redirect(1)
        .event_mask(EventMask::EXPOSURE);
    if translucent {
        aux = aux.colormap(colormap).background_pixel(0);
    } else {
        aux = aux.background_pixel(0x0012_1212);
    }
    conn.create_window(
        depth,
        win,
        screen.root,
        x,
        y,
        WIDTH as u16,
        HEIGHT as u16,
        0,
        WindowClass::INPUT_OUTPUT,
        visual_id,
        &aux,
    )
    .map_err(|err| HudError::Display(err.to_string()))?;
    set_above(&conn, win)?;
    if !translucent {
        set_capsule_shape(&conn, win, WIDTH as u16, HEIGHT as u16)?;
    }
    // Click-through: the input shape is an empty region.
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
    let gc = conn
        .generate_id()
        .map_err(|err| HudError::Display(err.to_string()))?;
    conn.create_gc(gc, win, &CreateGCAux::new())
        .map_err(|err| HudError::Display(err.to_string()))?;
    conn.map_window(win)
        .map_err(|err| HudError::Display(err.to_string()))?;
    conn.flush()
        .map_err(|err| HudError::Display(err.to_string()))?;

    let mut frame =
        FrameBuffer::new(WIDTH, HEIGHT).ok_or_else(|| HudError::Display("pixmap".into()))?;
    let mut history: VecDeque<f32> = (0..BAR_COUNT).map(|_| 0.0).collect();
    let mut displayed = 0.0f32;
    let started = Instant::now();
    let mut terminal_since: Option<(HudState, Instant)> = None;
    loop {
        if !running.load(Ordering::SeqCst) {
            break;
        }
        let state = HudState::from_bits(shared_state.load(Ordering::SeqCst));
        let elapsed = started.elapsed().as_secs_f32();
        match (terminal_since, state) {
            (None, HudState::Done) => terminal_since = Some((HudState::Done, Instant::now())),
            (None, HudState::Failed) => terminal_since = Some((HudState::Failed, Instant::now())),
            _ => {}
        }
        let fade = match terminal_since {
            Some((HudState::Done, at)) => {
                let held = at.elapsed();
                if held < DONE_HOLD {
                    1.0
                } else {
                    let fading = (held - DONE_HOLD).as_secs_f32() / DONE_FADE.as_secs_f32();
                    if fading >= 1.0 {
                        break;
                    }
                    1.0 - fading
                }
            }
            Some((HudState::Failed, at)) => {
                if at.elapsed() >= FAILED_HOLD {
                    break;
                }
                1.0
            }
            _ => 1.0,
        };

        displayed = smooth(displayed, meter.level());
        if state == HudState::Recording {
            history.push_back(displayed);
            history.pop_front();
        }
        render_frame(
            &mut frame,
            state,
            history.make_contiguous(),
            elapsed,
            fade,
            translucent,
        );
        put_frame(&conn, win, gc, &frame, &argb, root_masks)?;
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

fn put_frame<C: Connection>(
    conn: &C,
    win: u32,
    gc: u32,
    frame: &FrameBuffer,
    argb: &Option<ArgbVisual>,
    root_masks: (u32, u32, u32),
) -> Result<(), HudError> {
    let (masks, with_alpha, depth) = match argb {
        Some(argb) => ((argb.red_mask, argb.green_mask, argb.blue_mask), true, 32u8),
        None => (root_masks, false, 24u8),
    };
    let mut data = Vec::with_capacity((frame.width * frame.height * 4) as usize);
    for pixel in frame.pixmap.pixels() {
        data.extend_from_slice(&pack_pixel(*pixel, masks, with_alpha).to_le_bytes());
    }
    conn.put_image(
        x11rb::protocol::xproto::ImageFormat::Z_PIXMAP,
        win,
        gc,
        frame.width as u16,
        frame.height as u16,
        0,
        0,
        0,
        depth,
        &data,
    )
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

fn set_above<C: Connection>(conn: &C, win: u32) -> Result<(), HudError> {
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
    Ok(())
}

/// Cycle every state for screenshots: two seconds of recording bars driven
/// by the fixture or a synthetic source, then transcribing, done, failed.
pub fn run_hud_demo() -> Result<(), HudError> {
    let meter = LevelMeter::new();
    let cancel = crate::audio::CancellationToken::new();

    let fixture = std::env::var_os("ECHO_AUDIO_FIXTURE").map(std::path::PathBuf::from);
    let player = fixture.and_then(|path| {
        crate::audio::load_wav(&path).ok().map(|capture| {
            crate::audio::play_fixture_meter(&capture.pcm, meter.clone(), cancel.clone())
        })
    });
    if player.is_none() {
        // No fixture: a deterministic stand-in so the demo still moves.
        let demo_meter = meter.clone();
        let demo_cancel = cancel.clone();
        std::thread::spawn(move || {
            let mut tick = 0u32;
            while !demo_cancel.is_cancelled() {
                let level = if tick % 40 < 24 {
                    0.05 + 0.20 * (tick as f32 * 0.7).sin().abs()
                } else {
                    0.0
                };
                demo_meter.publish(level);
                tick += 1;
                std::thread::sleep(Duration::from_millis(30));
            }
        });
    }

    let running = SyncArc::new(AtomicBool::new(true));
    let state = SyncArc::new(AtomicU8::new(HudState::Recording.bits()));
    let worker = {
        let running = SyncArc::clone(&running);
        let state = SyncArc::clone(&state);
        std::thread::spawn(move || run(&running, &state, &meter))
    };
    std::thread::sleep(Duration::from_millis(2000));
    state.store(HudState::Transcribing.bits(), Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(1500));
    state.store(HudState::Done.bits(), Ordering::SeqCst);
    // Done holds, fades, and exits the loop on its own.
    let _ = worker.join();
    cancel.cancel();

    let running = SyncArc::new(AtomicBool::new(true));
    let failed = SyncArc::new(AtomicU8::new(HudState::Failed.bits()));
    let worker = {
        let running = SyncArc::clone(&running);
        let state = SyncArc::clone(&failed);
        std::thread::spawn(move || run(&running, &state, &LevelMeter::new()))
    };
    let _ = worker.join();
    Ok(())
}

#[cfg(test)]
mod tests {
    use echo_core::Config;

    use super::*;

    #[test]
    fn disabled_and_enabled_agree() {
        assert_eq!(enabled(), !hud_disabled());
    }

    #[test]
    fn hud_disable_tokens_and_file_false() {
        let enabled_file = Config::default();
        let disabled_file = Config {
            hud: Some(false),
            ..Config::default()
        };
        assert!(hud_is_disabled(Some("0"), &enabled_file));
        assert!(hud_is_disabled(Some("false"), &enabled_file));
        assert!(hud_is_disabled(Some("off"), &enabled_file));
        assert!(hud_is_disabled(None, &disabled_file));
        assert!(!hud_is_disabled(None, &enabled_file));
        assert!(!hud_is_disabled(Some("1"), &disabled_file));
        assert!(!hud_is_disabled(Some("true"), &disabled_file));
        assert!(!hud_is_disabled(Some("on"), &disabled_file));
    }

    #[test]
    fn smoothing_attacks_faster_than_it_releases() {
        let risen = smooth(0.0, 1.0);
        let fallen = smooth(1.0, 0.0);
        assert!(risen > 0.5, "attack closes most of the gap in one frame");
        assert!(fallen > risen, "release retains more than attack gains");
        assert!((fallen - RELEASE).abs() < f32::EPSILON);
    }

    #[test]
    fn silence_flattens_and_full_scale_saturates() {
        assert_eq!(bar_height(0.0), BAR_MIN);
        assert_eq!(bar_height(1.0), BAR_MAX);
        assert!(bar_height(2.0) == BAR_MAX, "over-scale clamps");
        let quiet = bar_height(0.05);
        assert!(quiet > BAR_MIN && quiet < BAR_MAX);
    }

    #[test]
    fn hud_state_round_trips_through_bits() {
        for state in [
            HudState::Recording,
            HudState::Transcribing,
            HudState::Done,
            HudState::Failed,
        ] {
            assert_eq!(HudState::from_bits(state.bits()), state);
        }
    }

    #[test]
    fn frames_render_in_both_modes_for_every_state() {
        let mut frame = FrameBuffer::new(WIDTH, HEIGHT).unwrap();
        let bars = [0.4f32; BAR_COUNT];
        for state in [
            HudState::Recording,
            HudState::Transcribing,
            HudState::Done,
            HudState::Failed,
        ] {
            for translucent in [true, false] {
                render_frame(&mut frame, state, &bars, 1.2, 1.0, translucent);
                assert!(
                    frame.pixmap.data().iter().any(|byte| *byte != 0),
                    "{state:?} translucent={translucent} drew nothing"
                );
            }
        }
    }

    #[test]
    fn fade_to_zero_leaves_an_empty_frame() {
        let mut frame = FrameBuffer::new(WIDTH, HEIGHT).unwrap();
        render_frame(
            &mut frame,
            HudState::Done,
            &[0.0; BAR_COUNT],
            0.0,
            0.0,
            true,
        );
        assert!(frame.pixmap.data().iter().all(|byte| *byte == 0));
    }

    #[test]
    fn compositor_detection_matches_the_selection_owner() {
        if std::env::var_os("DISPLAY").is_none() {
            return;
        }
        let (conn, screen_num) = x11rb::connect(None).expect("connect");
        let atom = conn
            .intern_atom(false, b"_NET_WM_CM_S0")
            .unwrap()
            .reply()
            .unwrap()
            .atom;
        let has_compositor = conn
            .get_selection_owner(atom)
            .unwrap()
            .reply()
            .unwrap()
            .owner
            != x11rb::NONE;
        assert_eq!(
            argb_visual(&conn, screen_num).unwrap().is_some(),
            has_compositor,
            "no _NET_WM_CM_S0 owner must select the SHAPE fallback"
        );
    }

    #[test]
    fn pixels_pack_per_the_visual_masks() {
        let red = tiny_skia::ColorU8::from_rgba(0xff, 0x00, 0x00, 0xff).premultiply();
        let packed = pack_pixel(red, (0xff0000, 0xff00, 0xff), true);
        assert_eq!(packed, 0xffff_0000);
        let dim = tiny_skia::ColorU8::from_rgba(0x80, 0x00, 0x00, 0xff).premultiply();
        let packed = pack_pixel(dim, (0xff0000, 0xff00, 0xff), false);
        assert_eq!(packed, 0x0080_0000);
        let half_alpha = tiny_skia::ColorU8::from_rgba(0xff, 0x00, 0x00, 0x80).premultiply();
        let packed = pack_pixel(half_alpha, (0xff0000, 0xff00, 0xff), true);
        assert_eq!(packed, 0x8080_0000, "ARGB channels stay premultiplied");
    }
}
