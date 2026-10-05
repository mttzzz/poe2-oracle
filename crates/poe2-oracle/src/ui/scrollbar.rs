//! A vertical scroll area with a scrollbar: the settings window's sections ([`scroll_area`]). The
//! wheel scrolls the area as GPUI scrolls any div; beside it, a thin bar shows how much of the
//! content is in view and where, whenever there is more of it than the area holds -- always, not
//! only under the pointer, so a player can see there is more. The thumb can be dragged, and a press
//! on the track jumps the thumb there and goes on as a drag of it. Nothing is drawn while the
//! content fits. The numbers -- the thumb's length and place, what a drag or a press scrolls to --
//! are `ui::scrollbar_geometry`'s.
//!
//! **The bar follows in the same frame.** The area tracks a [`ScrollHandle`]
//! (`StatefulInteractiveElement::track_scroll`), the one place GPUI keeps its scroll offset: the
//! wheel moves it, a drag of the bar moves it. The bar is the area's next sibling, so GPUI
//! prepaints it after the area, which has by then clamped the offset and published its own bounds
//! and overflow for this frame into the handle; the bar reads them from the handle in its own
//! prepaint, and paints the thumb where the content is drawn this frame -- after a wheel notch, a
//! drag, a resize, rows that came or went. Nothing is kept a frame behind.
//!
//! The bar is not a child of the area: GPUI shifts an area's children by its scroll offset and
//! counts every one of them in its content's size, so a bar inside would scroll away and make
//! the area scrollable by its own padding.

use std::rc::Rc;

use gpui::{
    App, Bounds, Corners, DispatchPhase, Div, ElementId, Entity, Hitbox, HitboxBehavior,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, ScrollHandle,
    StyleRefinement, Window, canvas, div, fill, point, prelude::*, px, rgb, size,
};

use crate::ui::scrollbar_geometry::Bar;
use crate::ui::style::Channel;
use crate::ui::theme::{BORDER_CARD, BORDER_GOLD, GOLD, blend, rems_from_px};

/// The thumb's and the track's width, px at 100 % UI scale.
const THUMB_WIDTH: f32 = 6.;
/// How much further either side of the thumb the bar can be taken by: a 6 px line is hard to hit.
const GRIP_MARGIN: f32 = 4.;
/// How far short of the area's top and bottom the track stops.
const END_INSET: f32 = 4.;
/// The shortest the thumb gets, on a page many windows long.
const MIN_THUMB: f32 = 28.;

/// The track, a faint rail; the thumb at rest, the bronze of the frame's lines; and the thumb
/// under the pointer or held, the gold of the window's other lit controls.
const TRACK: u32 = BORDER_CARD;
const THUMB: u32 = BORDER_GOLD;
const THUMB_LIT: u32 = GOLD;

/// `content` -- a div, which brings its own padding -- scrolled vertically in an area that is
/// sized and placed by the styles set on the result (`.flex_1().min_h_0()`...), with a scrollbar
/// beside its right edge. `id` names the area and keeps its scroll position for as long as it is
/// drawn in consecutive frames: a section the player leaves starts at its top when they come back.
pub(crate) fn scroll_area(id: impl Into<ElementId>, content: Div) -> ScrollArea {
    ScrollArea {
        id: id.into(),
        area: div(),
        content,
        inset: 0.,
    }
}

/// A scroll area with its scrollbar, made by [`scroll_area`]. The styles set on it size and place
/// the area's box.
#[derive(IntoElement)]
pub(crate) struct ScrollArea {
    id: ElementId,
    /// The area's own box, which takes the styles set on this.
    area: Div,
    /// What scrolls inside it.
    content: Div,
    inset: f32,
}

impl ScrollArea {
    /// Keeps the bar `inset` px (at 100 % UI scale) clear of the area's right edge: the settings
    /// window's frame lies there, and nothing inside it comes nearer than its keep-out.
    pub(crate) fn bar_inset(mut self, inset: f32) -> Self {
        self.inset = inset;
        self
    }
}

impl Styled for ScrollArea {
    fn style(&mut self) -> &mut StyleRefinement {
        self.area.style()
    }
}

impl RenderOnce for ScrollArea {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let state = window.use_keyed_state((self.id.clone(), "bar"), cx, |_, _| ScrollState::new());
        let handle = state.read(cx).handle.clone();
        self.area
            .relative()
            .flex()
            .flex_col()
            .child(
                self.content
                    .id(self.id)
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&handle),
            )
            .child(bar(state, self.inset))
    }
}

/// What an area keeps between frames, for as long as it is drawn in consecutive ones.
struct ScrollState {
    /// Where the area is scrolled, and how big it and its content are: written by the area
    /// itself (GPUI), and by the bar when it is dragged.
    handle: ScrollHandle,
    /// The thumb is held.
    drag: Option<Drag>,
    /// How lit the thumb is, eased over the pointer coming on to it and going off.
    glow: Channel,
}

impl ScrollState {
    fn new() -> Self {
        ScrollState {
            handle: ScrollHandle::new(),
            drag: None,
            glow: Channel::new(false),
        }
    }
}

/// A held thumb: where the pointer took it, and how far the content was scrolled then. Measured
/// from there, a drag does not wander off by the rounding of each step.
#[derive(Clone, Copy)]
struct Drag {
    pointer: Pixels,
    scroll: f32,
}

/// The strip the bar is gripped by, beside the area's right edge: a thumb's width and a margin of
/// [`GRIP_MARGIN`] either side, and from [`END_INSET`] under the area's top to the same above its
/// bottom. A canvas, laid out after the area; [`place`] and [`paint`] do the rest.
fn bar(state: Entity<ScrollState>, inset: f32) -> impl IntoElement {
    canvas(
        {
            let state = state.clone();
            move |strip, window, cx| place(&state, strip, window, cx)
        },
        move |_, placed, window, cx| paint(&state, placed, window, cx),
    )
    .absolute()
    .top(rems_from_px(END_INSET))
    .bottom(rems_from_px(END_INSET))
    .right(rems_from_px(inset))
    .w(rems_from_px(THUMB_WIDTH + 2. * GRIP_MARGIN))
}

/// The bar as this frame draws it.
struct Placed {
    bar: Bar,
    /// How far the content is scrolled this frame.
    scroll: f32,
    strip: Bounds<Pixels>,
    /// Over the strip; the wheel goes through it to the area.
    hitbox: Hitbox,
    /// The thumb's width this frame, in px.
    width: Pixels,
}

impl Placed {
    /// How far down the track `position` is.
    fn along(&self, position: Point<Pixels>) -> f32 {
        f32::from(position.y - self.strip.top())
    }

    /// Whether the pointer at `position` is over the thumb's length of the strip, which is wider
    /// than the thumb itself.
    fn over_thumb(&self, position: Point<Pixels>, window: &Window) -> bool {
        self.hitbox.is_hovered(window) && self.bar.on_thumb(self.scroll, self.along(position))
    }

    /// The `width` wide column of the strip, its middle.
    fn column(&self) -> Bounds<Pixels> {
        let left = self.strip.left() + (self.strip.size.width - self.width) / 2.;
        Bounds::new(
            point(left, self.strip.top()),
            size(self.width, self.strip.size.height),
        )
    }

    /// The thumb, in the column.
    fn thumb(&self) -> Bounds<Pixels> {
        let column = self.column();
        let top = column.top() + px(self.bar.thumb_top(self.scroll));
        Bounds::new(
            point(column.left(), top),
            size(self.width, px(self.bar.thumb())),
        )
    }
}

/// Prepaint: the bar for the area as it stands this frame -- `None` where the content fits, and
/// before the area has been laid out -- and the hitbox that takes the mouse over its strip.
///
/// The area has prepainted before the bar does, so its handle holds this frame's viewport,
/// overflow and offset; the offset is the one it draws its content at.
fn place(
    state: &Entity<ScrollState>,
    strip: Bounds<Pixels>,
    window: &mut Window,
    cx: &mut App,
) -> Option<Placed> {
    let handle = state.read(cx).handle.clone();
    let rem_size = window.rem_size();
    let viewport = f32::from(handle.bounds().size.height);
    let content = viewport + f32::from(handle.max_offset().y);
    let min_thumb = f32::from(rems_from_px(MIN_THUMB).to_pixels(rem_size));
    let Some(bar) = Bar::new(viewport, content, f32::from(strip.size.height), min_thumb) else {
        // Nothing left to hold: the content shrank under a held thumb.
        state.update(cx, |state, _| state.drag = None);
        return None;
    };
    Some(Placed {
        bar,
        scroll: -f32::from(handle.offset().y),
        strip,
        hitbox: window.insert_hitbox(strip, HitboxBehavior::BlockMouseExceptScroll),
        width: rems_from_px(THUMB_WIDTH).to_pixels(rem_size),
    })
}

/// Paint: the track and the thumb, and the mouse handling for them.
fn paint(state: &Entity<ScrollState>, placed: Option<Placed>, window: &mut Window, cx: &mut App) {
    let Some(placed) = placed else {
        return;
    };
    let placed = Rc::new(placed);
    // Hit-tested against this frame's hitboxes, which GPUI has just collected. Once the pointer
    // has left the window no move says so, but GPUI redraws and the window reads as not hovered.
    let lit = window.is_window_hovered() && placed.over_thumb(window.mouse_position(), window);
    let glow = state.update(cx, |state, cx| {
        state.glow.turn(state.drag.is_some() || lit);
        state.glow.frame(window, cx)
    });
    // The track and the thumb, rounded ends and all, on whole device pixels, as the frame's lines
    // are: at 125 % a 6 px line is 7.5 device pixels wide and would blur at its edges.
    for (bounds, color) in [
        (placed.column(), TRACK),
        (placed.thumb(), blend(THUMB, THUMB_LIT, glow)),
    ] {
        let bounds = window.pixel_snap_bounds(bounds);
        let pill = Corners::all(Pixels::MAX).clamp_radii_for_quad_size(bounds.size);
        window.paint_quad(fill(bounds, rgb(color)).corner_radii(pill));
    }

    // A press on the thumb takes it where it stands; elsewhere on the track the thumb jumps to
    // the press, which goes on as a drag of it.
    window.on_mouse_event({
        let (state, placed) = (state.clone(), placed.clone());
        move |event: &MouseDownEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble
                || event.button != MouseButton::Left
                || !placed.hitbox.is_hovered(window)
            {
                return;
            }
            let at = placed.along(event.position);
            let scroll = if placed.bar.on_thumb(placed.scroll, at) {
                placed.scroll
            } else {
                placed.bar.pressed(at)
            };
            state.update(cx, |state, cx| {
                state.drag = Some(Drag {
                    pointer: event.position.y,
                    scroll,
                });
                scroll_to(&state.handle, scroll);
                cx.notify();
            });
            cx.stop_propagation();
        }
    });
    // A held thumb follows the pointer wherever it goes, in or out of the window (Windows
    // captures the mouse for a pressed button). Without a held thumb, a move that brings the
    // pointer on to the thumb or off it draws it lit or not.
    window.on_mouse_event({
        let (state, placed) = (state.clone(), placed.clone());
        move |event: &MouseMoveEvent, phase, window, cx| {
            if phase != DispatchPhase::Capture {
                return;
            }
            let Some(drag) = state.read(cx).drag else {
                if placed.over_thumb(event.position, window) != lit {
                    state.update(cx, |_, cx| cx.notify());
                }
                return;
            };
            state.update(cx, |state, cx| {
                if event.pressed_button == Some(MouseButton::Left) {
                    let moved = f32::from(event.position.y - drag.pointer);
                    scroll_to(&state.handle, placed.bar.dragged(drag.scroll, moved));
                } else {
                    // The button came up where this window did not see it, as when the system
                    // takes the capture away mid-drag: the next move without it ends the drag.
                    state.drag = None;
                }
                cx.notify();
            });
        }
    });
    window.on_mouse_event({
        let state = state.clone();
        move |event: &MouseUpEvent, phase, _, cx| {
            if phase == DispatchPhase::Capture
                && event.button == MouseButton::Left
                && state.read(cx).drag.is_some()
            {
                state.update(cx, |state, cx| {
                    state.drag = None;
                    cx.notify();
                });
            }
        }
    });
}

/// Scrolls the area to `scroll` px from its top; GPUI clamps it to the content when it next lays
/// the area out.
fn scroll_to(handle: &ScrollHandle, scroll: f32) {
    handle.set_offset(point(handle.offset().x, px(-scroll)));
}
