// Derived from Coucou (https://github.com/Louis-CFM/coucou), MIT © 2026 Louis
// Raillé: the island window's cursor poll, click-through toggle and
// display-change check in windows/src-tauri/src/island.rs. See
// THIRD-PARTY-NOTICES.

//! The overlay window (`main`): placement, dragging, and the cursor poll
//! that makes everything but the creature and its cards click-through.
//!
//! The window is transparent, always on top and never takes focus
//! (`focusable: false` in tauri.conf.json). It is much larger than what it
//! draws, so whether it takes the mouse is decided here, on one thread, in
//! the same tick as the cursor read. The frontend only reports which
//! rectangles are interactive. The geometry lives in
//! `ottid_core::overlay`, where it is unit-tested.
//!
//! Never hold the state lock while calling a Tauri window or monitor API:
//! the getters round-trip to the main thread, which may itself be waiting
//! on the lock.

use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use ottid_core::overlay::{
    self, decide, default_anchor, gaze_toward, legacy_anchor, pick_monitor, pointer, snap,
    switch_anchor, ClickThrough, Drags, Frame, Gaze, Layout, Monitor, Placement, Point, Rect,
    Region, RegionSet, Sample, Stamp, GAZE_FALLOFF,
};
use serde::{Deserialize, Serialize};
use tauri::menu::CheckMenuItem;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, Wry};
use tauri_plugin_store::StoreExt;

/// The overlay window's label.
pub const WINDOW: &str = "main";

const SETTINGS: &str = "settings.json";
const KEY_PLACEMENT: &str = "overlay.placement";
const KEY_ANCHOR: &str = "overlay.anchor";
/// Where the old overlay saved its window's top-left (physical pixels).
const KEY_LEGACY_POSITION: &str = "tongue.position";

/// Menu ids of the placement items are this prefix plus the placement code.
pub const MENU_PREFIX: &str = "placement:";

/// Poll period while the window is visible: one display frame.
const TICK: Duration = Duration::from_millis(16);
/// Poll period while dragging, so the creature keeps up with the cursor.
const DRAG_TICK: Duration = Duration::from_millis(8);
/// Poll period while the window is hidden: only watching for it to return.
const HIDDEN_TICK: Duration = Duration::from_millis(200);
/// Ask whether the window is visible every this many ticks.
const VISIBILITY_EVERY: u32 = 15;
/// Look for display changes, and for the window having been moved behind
/// our back, every this many ticks (about twice a second).
const DISPLAYS_EVERY: u32 = 30;
/// Gaze changes smaller than this aren't worth an event.
const GAZE_STEP: f64 = 0.02;

#[derive(Default)]
pub struct OverlayState {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    placement: Placement,
    /// The stage's centre in physical screen pixels. `None` until settings
    /// are read; the default then depends on the monitors.
    anchor: Option<Point>,
    regions: RegionSet,
    clicks: ClickThrough,
    drags: Drags,
    monitors: Vec<Monitor>,
    primary: usize,
    layout: Option<Layout>,
    hover: Option<String>,
    gaze: Option<Gaze>,
}

/// What `relayout` decided the window and the frontend need.
#[derive(Default)]
struct Plan {
    /// Where the window goes, when it has to move or resize.
    window: Option<Rect>,
    resize: bool,
    /// The new frame, when the frontend has to lay itself out again.
    frame: Option<Frame>,
}

impl OverlayState {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        // A panic while holding the lock leaves plain geometry behind, which
        // is still safe to use.
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl Inner {
    fn compute(&self) -> Option<Layout> {
        let primary = self.monitors.get(self.primary).or(self.monitors.first())?;
        let anchor = self.anchor.unwrap_or_else(|| default_anchor(primary));
        let index = pick_monitor(anchor, &self.monitors)?;
        Some(overlay::layout(
            self.placement,
            anchor,
            &self.monitors[index],
            self.drags.is_active(),
        ))
    }

    /// Recompute the layout and record it. Outside a drag the anchor is
    /// normalised to where the stage really is, so the next drag starts
    /// from what the user sees.
    fn relayout(&mut self) -> Plan {
        let Some(next) = self.compute() else {
            return Plan::default();
        };
        if !self.drags.is_active() {
            self.anchor = Some(next.stage.center());
        }
        let prev = self.layout.replace(next.clone());
        let moved = prev.as_ref().is_none_or(|p| p.window != next.window);
        let resize = prev.as_ref().is_none_or(|p| {
            p.window.width != next.window.width || p.window.height != next.window.height
        });
        let reframed = prev.as_ref().is_none_or(|p| p.frame != next.frame);
        if moved {
            self.clicks.invalidate();
        }
        Plan {
            window: moved.then_some(next.window),
            resize,
            frame: reframed.then_some(next.frame),
        }
    }
}

/// The current frame, for a frontend that mounted after the last
/// `overlay:layout` event.
#[tauri::command]
pub async fn overlay_layout(
    state: tauri::State<'_, OverlayState>,
) -> Result<Option<Frame>, String> {
    Ok(state.lock().layout.as_ref().map(|l| l.frame.clone()))
}

/// The frontend's interactive rectangles: physical pixels relative to the
/// window, measured at `scale`. The cursor poll hit-tests against them from
/// the next tick on. `source` and `seq` stamp the report, so one handled
/// after a report the page sent later is dropped.
#[tauri::command]
pub async fn overlay_set_regions(
    state: tauri::State<'_, OverlayState>,
    regions: Vec<Region>,
    scale: f64,
    source: u32,
    seq: u64,
) -> Result<(), String> {
    state
        .lock()
        .regions
        .replace_in_order(regions, scale, Stamp { source, seq })
        .map(|_| ())
        .map_err(|err| err.to_string())
}

/// Start dragging the creature. The poll moves the window with the cursor
/// until `overlay_drag_end` brings back the token this returns (or until
/// the poll sees the button released). The frontend sends the end only once
/// this has answered: the runtime may handle the two in either order.
#[tauri::command]
pub async fn overlay_drag_start(app: AppHandle) -> Result<u64, String> {
    let Some(cursor) = read_cursor(&app) else {
        return Err("the cursor position is not available".into());
    };
    let state = app.state::<OverlayState>();
    let (plan, token) = {
        let mut inner = state.lock();
        let Some(center) = inner.layout.as_ref().map(|l| l.stage.center()) else {
            return Err("the overlay has no layout yet".into());
        };
        let token = inner.drags.start(center - cursor);
        (inner.relayout(), token)
    };
    carry_out(&app, plan);
    Ok(token)
}

/// Drop the creature dragged since the start that returned `token`: snap to
/// the taskbar or the ceiling when close to either, and remember where it
/// is. The end of an older drag changes nothing.
#[tauri::command]
pub async fn overlay_drag_end(app: AppHandle, token: u64) -> Result<(), String> {
    end_drag(&app, token);
    Ok(())
}

/// Move Ottid to `placement` (the Hub's control).
#[tauri::command]
pub async fn overlay_set_placement(app: AppHandle, placement: Placement) -> Result<(), String> {
    set_placement(&app, placement);
    Ok(())
}

/// Read the saved placement, place the window, show it and start the poll.
/// Call once from `setup`, after the menu is managed.
pub fn init(app: &AppHandle) {
    let monitors = read_monitors(app);
    let saved = read_settings(app);
    let state = app.state::<OverlayState>();
    let (plan, placement, anchor, migrated) = {
        let mut inner = state.lock();
        if let Some((monitors, primary)) = monitors {
            inner.monitors = monitors;
            inner.primary = primary;
        }
        let mut migrated = false;
        match saved {
            Saved::Current { placement, anchor } => {
                inner.placement = placement;
                inner.anchor = anchor;
            }
            Saved::Legacy { top_left } => {
                // The old overlay floated freely: keep it where the user left
                // it, and pick the placement its position suggests.
                if let Some(index) = pick_monitor(top_left, &inner.monitors) {
                    let monitor = inner.monitors[index];
                    let anchor = legacy_anchor(top_left, monitor.scale);
                    let float = overlay::layout(Placement::Float, anchor, &monitor, false);
                    inner.placement = snap(&float.stage, &monitor);
                    inner.anchor = Some(anchor);
                    migrated = true;
                }
            }
            Saved::Nothing => {}
        }
        let plan = inner.relayout();
        (plan, inner.placement, inner.anchor, migrated)
    };
    carry_out(app, plan);
    if migrated {
        tracing::info!(
            target: "ottid::overlay",
            "moved the old overlay position to the {} placement",
            placement.code()
        );
        persist(app, placement, anchor);
    }
    sync_menu(app, placement);
    if let Some(window) = app.get_webview_window(WINDOW) {
        // Until the poll first looks at the cursor, the large transparent
        // window would take every click on what lies under it: let them
        // through from the start.
        let ignore = state.lock().clicks.update(false);
        set_click_through(&state, &window, ignore);
        // The window is created hidden so it never flashes at a default
        // position. `focus: false` makes this first show non-activating.
        let _ = window.show();
    }
    spawn_poll(app.clone());
}

/// Apply a placement chosen from the menu or the Hub.
pub fn set_placement(app: &AppHandle, placement: Placement) {
    let state = app.state::<OverlayState>();
    let (plan, anchor) = {
        let mut inner = state.lock();
        if let Some(layout) = inner.layout.as_ref() {
            let anchor = switch_anchor(layout, placement);
            inner.anchor = Some(anchor);
        }
        inner.drags.cancel();
        inner.placement = placement;
        let plan = inner.relayout();
        (plan, inner.anchor)
    };
    carry_out(app, plan);
    persist(app, placement, anchor);
    announce(app, placement);
}

/// The menu's placement items, kept so their checks follow the placement.
pub struct PlacementMenu(pub Vec<(Placement, CheckMenuItem<Wry>)>);

/// Check the item for `placement` and uncheck the others. The OS toggles an
/// item when it is clicked, so this also re-checks a re-selected item.
pub fn sync_menu(app: &AppHandle, placement: Placement) {
    let Some(menu) = app.try_state::<PlacementMenu>() else {
        return;
    };
    for (item_placement, item) in &menu.0 {
        let _ = item.set_checked(*item_placement == placement);
    }
}

fn end_drag(app: &AppHandle, token: u64) {
    let state = app.state::<OverlayState>();
    let (plan, placement, anchor) = {
        let mut inner = state.lock();
        if !inner.drags.end(token) {
            return;
        }
        if let Some(layout) = inner.layout.clone() {
            inner.placement = snap(&layout.stage, &layout.monitor);
            inner.anchor = Some(layout.stage.center());
        }
        let plan = inner.relayout();
        (plan, inner.placement, inner.anchor)
    };
    carry_out(app, plan);
    persist(app, placement, anchor);
    announce(app, placement);
}

/// Tell the Hub and the menu that the placement changed.
fn announce(app: &AppHandle, placement: Placement) {
    let _ = app.emit(
        "settings:changed",
        serde_json::json!({ "key": KEY_PLACEMENT, "value": placement.code() }),
    );
    sync_menu(app, placement);
}

/// Move and resize the real window, and tell the frontend its new frame.
fn carry_out(app: &AppHandle, plan: Plan) {
    let Some(window) = app.get_webview_window(WINDOW) else {
        return;
    };
    if let Some(rect) = plan.window {
        let size = PhysicalSize::new(rect.width as u32, rect.height as u32);
        let position = PhysicalPosition::new(rect.x as i32, rect.y as i32);
        if plan.resize {
            // GTK never shrinks a non-resizable window below its natural
            // size, and tao re-applies `resizable: false` after each
            // configure, so ask every time. Undecorated, the window still
            // offers the user nothing to resize it by.
            #[cfg(target_os = "linux")]
            let _ = window.set_resizable(true);
            let _ = window.set_size(size);
        }
        let _ = window.set_position(position);
        if plan.resize {
            // Moving onto a display with another scale makes the OS rescale
            // the window: assert the physical size again.
            let _ = window.set_size(size);
        }
    }
    if let Some(frame) = plan.frame {
        let _ = app.emit_to(WINDOW, "overlay:layout", frame);
    }
}

#[derive(Clone, Serialize)]
struct HoverPayload {
    region: Option<String>,
}

fn spawn_poll(app: AppHandle) {
    let spawned = std::thread::Builder::new()
        .name("overlay-poll".into())
        .spawn(move || poll(app));
    if let Err(err) = spawned {
        tracing::error!(target: "ottid::overlay", "could not start the cursor poll: {err}");
    }
}

/// The cursor poll. Each tick reads the cursor, moves a dragged window,
/// decides click-through and tells the frontend what the cursor is over and
/// where the creature should look.
fn poll(app: AppHandle) {
    let state = app.state::<OverlayState>();
    let mut tick: u32 = 0;
    let mut visible = false;

    loop {
        let dragging = state.lock().drags.is_active();
        std::thread::sleep(if !visible {
            HIDDEN_TICK
        } else if dragging {
            DRAG_TICK
        } else {
            TICK
        });
        tick = tick.wrapping_add(1);

        let Some(window) = app.get_webview_window(WINDOW) else {
            continue;
        };
        if !visible || tick % VISIBILITY_EVERY == 0 {
            let now = window.is_visible().unwrap_or(false);
            if now != visible {
                // Hiding and showing can reset the window's styles: decide
                // the flag afresh, and start over with nothing hovered. A
                // hidden window lets clicks through, so that when it is shown
                // again it takes none until the poll has looked.
                let (was_hovering, ignore) = {
                    let mut inner = state.lock();
                    inner.clicks.invalidate();
                    let ignore = if now {
                        None
                    } else {
                        inner.clicks.update(false)
                    };
                    (inner.hover.take().is_some(), ignore)
                };
                set_click_through(&state, &window, ignore);
                if was_hovering {
                    let _ = app.emit_to(WINDOW, "overlay:hover", HoverPayload { region: None });
                }
                visible = now;
            }
        }
        if !visible {
            continue;
        }

        if tick % DISPLAYS_EVERY == 0 {
            check_displays(&app, &window);
        }

        let Some(cursor) = read_cursor(&app) else {
            // No global cursor (Wayland): a window that can't tell where the
            // cursor is must not block the desktop under it. Ottid stays
            // visible and its hotkeys work; only the mouse passes through.
            let ignore = state.lock().clicks.update(false);
            set_click_through(&state, &window, ignore);
            continue;
        };
        let button = pointer::primary_button_down();

        let (released, plan, ignore, hover, gaze) = 'decide: {
            let mut inner = state.lock();
            let mut plan = None;
            let released = inner.drags.observe_button(button);
            if released.is_none() {
                if let Some(offset) = inner.drags.offset() {
                    inner.anchor = Some(cursor + offset);
                    plan = Some(inner.relayout());
                }
            }
            let Some(layout) = inner.layout.clone() else {
                // No layout to hit-test against (no displays read yet): the
                // window must not block the desktop under it.
                break 'decide (released, plan, inner.clicks.update(false), None, None);
            };
            let sample = Sample {
                cursor,
                origin: Point::new(layout.window.x, layout.window.y),
                scale: layout.monitor.scale,
            };
            let decision = decide(&inner.regions, &sample, inner.drags.is_active());
            let ignore = inner.clicks.update(decision.accept);
            let hover = if inner.hover != decision.region {
                inner.hover = decision.region.clone();
                Some(decision.region)
            } else {
                None
            };
            let gaze = gaze_toward(layout.eye(), cursor, GAZE_FALLOFF * layout.monitor.scale)
                .quantized(GAZE_STEP);
            let gaze = if inner.gaze != Some(gaze) {
                inner.gaze = Some(gaze);
                Some(gaze)
            } else {
                None
            };
            (released, plan, ignore, hover, gaze)
        };

        if let Some(token) = released {
            end_drag(&app, token);
        }
        if let Some(plan) = plan {
            carry_out(&app, plan);
        }
        set_click_through(&state, &window, ignore);
        if let Some(region) = hover {
            let _ = app.emit_to(WINDOW, "overlay:hover", HoverPayload { region });
        }
        if let Some(gaze) = gaze {
            let _ = app.emit_to(WINDOW, "overlay:gaze", gaze);
        }
    }
}

/// Tell the OS what `ClickThrough::update` decided, if anything. A failed
/// call leaves the flag unknown, so the next tick sets it again.
fn set_click_through(state: &OverlayState, window: &tauri::WebviewWindow, ignore: Option<bool>) {
    let Some(ignore) = ignore else {
        return;
    };
    if window.set_ignore_cursor_events(ignore).is_err() {
        state.lock().clicks.failed();
    }
}

/// Place the window again when the displays changed (plugged, unplugged,
/// rearranged, rescaled, the taskbar moved) or when something else moved the
/// window, such as the OS rescuing it from a display that went away.
fn check_displays(app: &AppHandle, window: &tauri::WebviewWindow) {
    let monitors = read_monitors(app);
    // A minimized window sits far off-screen; that isn't a move to undo.
    let origin = match window.is_minimized() {
        Ok(false) => window.outer_position().ok(),
        _ => None,
    };
    let state = app.state::<OverlayState>();
    let plan = {
        let mut inner = state.lock();
        let mut changed = false;
        if let Some((monitors, primary)) = monitors {
            if inner.monitors != monitors || inner.primary != primary {
                inner.monitors = monitors;
                inner.primary = primary;
                changed = true;
            }
        }
        let displaced = match (origin, inner.layout.as_ref()) {
            (Some(o), Some(l)) => o.x as f64 != l.window.x || o.y as f64 != l.window.y,
            _ => false,
        };
        if displaced {
            // Forget the window so the plan puts it back.
            inner.layout = None;
        }
        let reason = if changed {
            "the displays changed"
        } else {
            "the window was moved"
        };
        (changed || displaced).then(|| (reason, inner.relayout()))
    };
    if let Some((reason, plan)) = plan {
        tracing::info!(target: "ottid::overlay", "{reason}; placing the overlay again");
        carry_out(app, plan);
    }
}

/// The cursor in physical screen pixels: straight from the OS where it can
/// answer from this thread, through Tauri otherwise. `None` where there is
/// no global cursor (Wayland).
fn read_cursor(app: &AppHandle) -> Option<Point> {
    pointer::cursor_physical()
        .or_else(|| app.cursor_position().ok().map(|p| Point::new(p.x, p.y)))
        .filter(|p| p.is_finite())
}

fn read_monitors(app: &AppHandle) -> Option<(Vec<Monitor>, usize)> {
    let list = app.available_monitors().ok()?;
    if list.is_empty() {
        return None;
    }
    let primary = app.primary_monitor().ok().flatten();
    let index = primary
        .and_then(|p| {
            list.iter()
                .position(|m| m.position() == p.position() && m.size() == p.size())
        })
        .unwrap_or(0);
    Some((list.iter().map(to_core).collect(), index))
}

fn to_core(monitor: &tauri::Monitor) -> Monitor {
    let position = monitor.position();
    let size = monitor.size();
    let work = monitor.work_area();
    Monitor {
        bounds: Rect::new(
            position.x as f64,
            position.y as f64,
            size.width as f64,
            size.height as f64,
        ),
        work: Rect::new(
            work.position.x as f64,
            work.position.y as f64,
            work.size.width as f64,
            work.size.height as f64,
        ),
        scale: monitor.scale_factor(),
    }
}

enum Saved {
    Current {
        placement: Placement,
        anchor: Option<Point>,
    },
    Legacy {
        top_left: Point,
    },
    Nothing,
}

#[derive(Deserialize)]
struct XY {
    x: f64,
    y: f64,
}

fn read_point(value: Option<serde_json::Value>) -> Option<Point> {
    let xy: XY = serde_json::from_value(value?).ok()?;
    Some(Point::new(xy.x, xy.y)).filter(|p| p.is_finite())
}

fn read_settings(app: &AppHandle) -> Saved {
    let store = match app.store(SETTINGS) {
        Ok(store) => store,
        Err(err) => {
            tracing::warn!(target: "ottid::overlay", "could not open the settings store: {err:#}");
            return Saved::Nothing;
        }
    };
    let placement = store
        .get(KEY_PLACEMENT)
        .and_then(|v| v.as_str().and_then(Placement::from_code));
    if let Some(placement) = placement {
        return Saved::Current {
            placement,
            anchor: read_point(store.get(KEY_ANCHOR)),
        };
    }
    match read_point(store.get(KEY_LEGACY_POSITION)) {
        Some(top_left) => Saved::Legacy { top_left },
        None => Saved::Nothing,
    }
}

fn persist(app: &AppHandle, placement: Placement, anchor: Option<Point>) {
    let store = match app.store(SETTINGS) {
        Ok(store) => store,
        Err(err) => {
            tracing::warn!(target: "ottid::overlay", "could not open the settings store: {err:#}");
            return;
        }
    };
    store.set(KEY_PLACEMENT, placement.code());
    if let Some(anchor) = anchor {
        store.set(
            KEY_ANCHOR,
            serde_json::json!({ "x": anchor.x.round(), "y": anchor.y.round() }),
        );
    }
    if let Err(err) = store.save() {
        tracing::warn!(target: "ottid::overlay", "could not save the overlay placement: {err:#}");
    }
}
