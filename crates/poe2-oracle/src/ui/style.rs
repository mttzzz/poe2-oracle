//! The game-styled look the windows share, drawn by the app itself -- no game art: the double
//! gold frame with corner ornaments of the game's tooltips and inventory ([`game_frame`]; a bar
//! too low for them has a diamond at each end, [`bar_frame`]), rules with a diamond at their
//! centre ([`ornament_rule`], [`section_heading`]), headings in the game's
//! face ([`heading`], the faces in `ui::fonts`), a warm gradient on title bars
//! ([`title_gradient`]), VibeTools' three neutral black shadows by height, and restrained motion:
//! every hover and state change eases over [`TRANSITION`] ([`ease_hover`], [`ease_state`]), a
//! panel appears over [`APPEAR`] ([`appear`]). The controls built from these -- buttons,
//! switches, checkboxes, segmented choices, selects and their menus, keycaps, the hotkey recorder,
//! tooltips -- live here too.
//!
//! Sizes are rems (`theme::rems_from_px`): in the price panel and the overlays they follow the
//! player's UI scale, elsewhere a rem is 16 px. Hairlines, shadows and glows stay in pixels.

use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
    Anchor, Animation, AnimationElement, AnimationExt as _, AnyView, App, Background, BorderStyle,
    Bounds, BoxShadow, Div, ElementId, Font, FontWeight, Hsla, MouseButton, MouseDownEvent,
    PathBuilder, Pixels, Point, Rgba, SharedString, Stateful, TextRun, Window, anchored, canvas,
    deferred, div, fill, hsla, linear_color_stop, linear_gradient, outline, point, prelude::*, px,
    rgb, size,
};

use crate::ui::fonts::NameFont;
use crate::ui::theme::{
    BASE_REM_SIZE, BG_BUTTON_HOVER, BG_CARD, BG_CLOSE_HOVER, BG_FIELD, BG_MENU, BG_TITLE,
    BORDER_CARD, BORDER_DANGER, BORDER_FIELD, BORDER_GOLD, BORDER_ROW, GOLD, GOLD_LIGHT, KEY_TOP,
    PLATE_BOTTOM, PLATE_TOP, TEXT, TEXT_DIM, TEXT_MUTED, TEXT_WARNING, TITLE_BOTTOM, TITLE_TOP,
    blend, rems_from_px,
};

/// Every hover and state change: VibeTools' one timing, `0.12s ease`, used everywhere.
pub(crate) const TRANSITION: Duration = Duration::from_millis(120);
/// A panel, dialog or menu appearing.
pub(crate) const APPEAR: Duration = Duration::from_millis(150);
/// How far an appearing panel rises into place, px.
const APPEAR_RISE: f32 = 6.;
/// How far content switched in -- a settings section -- rises into place, px.
const SWITCH_RISE: f32 = 4.;

/// A card's corners: VibeTools' 8 px.
pub(crate) const CARD_RADIUS: f32 = 8.;
/// A control's corners.
pub(crate) const CONTROL_RADIUS: f32 = 4.;
/// A control's height in a window; the panel's are shorter.
pub(crate) const CONTROL_HEIGHT: f32 = 30.;

/// The frame's inner line: this far inside the outer one, px, at this share of the gold.
const FRAME_GAP: f32 = 3.;
const FRAME_INNER_OPACITY: f32 = 0.3;
/// A frame corner's gold arms run this far along each edge, px.
const FRAME_ARM: f32 = 26.;
/// Half the diagonal of a frame corner's diamond, px: its tips rest on the outer lines.
const FRAME_DIAMOND: f32 = 4.;

/// The gold glow at full hover: its strength and reach, px.
const GLOW_OPACITY: f32 = 0.35;
const GLOW_BLUR: f32 = 12.;
/// The black behind a modal dialog.
pub(crate) const SCRIM_OPACITY: f32 = 0.55;

/// CSS's `ease` curve, `cubic-bezier(0.25, 0.1, 0.25, 1)`: a quick start that settles gently --
/// what VibeTools' `0.12s ease` runs on.
pub(crate) fn ease(t: f32) -> f32 {
    cubic_bezier((0.25, 0.1), (0.25, 1.), t)
}

/// The unit cubic Bézier through `p1` and `p2` at x = `x`, solved as browsers solve CSS's
/// `cubic-bezier()`: Newton's method on x(s), bisection where it stalls.
fn cubic_bezier((x1, y1): (f32, f32), (x2, y2): (f32, f32), x: f32) -> f32 {
    let x = x.clamp(0., 1.);
    let (cx, cy) = (3. * x1, 3. * y1);
    let (bx, by) = (3. * (x2 - x1) - cx, 3. * (y2 - y1) - cy);
    let (ax, ay) = (1. - cx - bx, 1. - cy - by);
    let sample_x = |s: f32| ((ax * s + bx) * s + cx) * s;
    let slope_x = |s: f32| (3. * ax * s + 2. * bx) * s + cx;
    const EPSILON: f32 = 1e-5;
    let mut s = x;
    let mut solved = false;
    for _ in 0..8 {
        let error = sample_x(s) - x;
        if error.abs() < EPSILON {
            solved = true;
            break;
        }
        let slope = slope_x(s);
        if slope.abs() < 1e-6 {
            break;
        }
        s -= error / slope;
    }
    if !solved {
        let (mut low, mut high) = (0f32, 1f32);
        s = x;
        for _ in 0..32 {
            let error = sample_x(s) - x;
            if error.abs() < EPSILON {
                break;
            }
            if error > 0. {
                high = s;
            } else {
                low = s;
            }
            s = (low + high) / 2.;
        }
    }
    ((ay * s + by) * s + cy) * s
}

/// `color` at `opacity`.
pub(crate) fn alpha(color: u32, opacity: f32) -> Rgba {
    Rgba {
        a: opacity,
        ..rgb(color)
    }
}

fn black_shadow(offset_y: f32, blur: f32, opacity: f32) -> Vec<BoxShadow> {
    vec![BoxShadow::new(px(0.), px(offset_y), hsla(0., 0., 0., opacity)).blur_radius(px(blur))]
}

/// The shadow of a small surface floating over the window: a menu, a dropdown.
pub(crate) fn popup_shadow() -> Vec<BoxShadow> {
    black_shadow(8., 28., 0.55)
}

/// The shadow of a modal dialog.
pub(crate) fn modal_shadow() -> Vec<BoxShadow> {
    black_shadow(12., 40., 0.6)
}

/// The shadow of a tooltip: the closest to its surface, the darkest.
pub(crate) fn tooltip_shadow() -> Vec<BoxShadow> {
    black_shadow(5., 16., 0.65)
}

/// A soft `color` glow around a control under the pointer, `amount` (0 to 1) of the way in --
/// gold for most, red for a destructive one. None at rest.
pub(crate) fn glow(color: u32, amount: f32) -> Vec<BoxShadow> {
    if amount <= 0. {
        return Vec::new();
    }
    vec![
        BoxShadow::new(px(0.), px(0.), alpha(color, GLOW_OPACITY * amount).into())
            .blur_radius(px(GLOW_BLUR)),
    ]
}

/// The gold glow cast inward, for rows that sit edge to edge, where an outer glow would spill
/// onto the neighbours.
pub(crate) fn inner_glow(amount: f32) -> Vec<BoxShadow> {
    if amount <= 0. {
        return Vec::new();
    }
    vec![
        BoxShadow::new(px(0.), px(0.), alpha(GOLD, 0.22 * amount).into())
            .blur_radius(px(14.))
            .inset(),
    ]
}

/// One eased 0-to-1 channel: how far an element is into its hover, or into its "on" look.
struct Channel {
    on: bool,
    /// The amount when it last turned, and when: it eases from there to its new end.
    from: f32,
    turned: Option<Instant>,
}

impl Channel {
    fn new(on: bool) -> Self {
        Channel {
            on,
            from: end(on),
            turned: None,
        }
    }

    fn amount(&self, now: Instant) -> f32 {
        let end = end(self.on);
        match self.turned {
            Some(turned) => {
                let progress =
                    now.saturating_duration_since(turned).as_secs_f32() / TRANSITION.as_secs_f32();
                if progress >= 1. {
                    end
                } else {
                    self.from + (end - self.from) * ease(progress)
                }
            }
            None => end,
        }
    }

    /// Heads for `on` from wherever the channel is now; `false` when it already heads there.
    fn turn(&mut self, on: bool) -> bool {
        if self.on == on {
            return false;
        }
        let now = Instant::now();
        self.from = self.amount(now);
        self.on = on;
        self.turned = Some(now);
        true
    }

    /// The amount to draw this frame, asking for the next one while the channel still moves --
    /// as GPUI's own animations do -- or its end at once when the system asks for reduced motion.
    fn frame(&mut self, window: &Window, cx: &App) -> f32 {
        let Some(turned) = self.turned else {
            return end(self.on);
        };
        let now = Instant::now();
        if cx.reduce_motion() || now.saturating_duration_since(turned) >= TRANSITION {
            self.turned = None;
            return end(self.on);
        }
        window.request_animation_frame();
        self.amount(now)
    }
}

fn end(on: bool) -> f32 {
    if on { 1. } else { 0. }
}

/// An element easing into its hover look while the pointer is over it and back out when it
/// leaves, over [`TRANSITION`]: `style(element, amount)` draws it `amount` (0 to 1) of the way
/// in. `key` names its channel, unique among its siblings; the element keeps its own id and
/// state.
#[derive(IntoElement)]
pub(crate) struct HoverEase<
    E: StatefulInteractiveElement + IntoElement + 'static,
    F: Fn(E, f32) -> E + 'static,
> {
    key: ElementId,
    element: E,
    style: F,
}

pub(crate) fn ease_hover<E, F>(key: impl Into<ElementId>, element: E, style: F) -> HoverEase<E, F>
where
    E: StatefulInteractiveElement + IntoElement + 'static,
    F: Fn(E, f32) -> E + 'static,
{
    HoverEase {
        key: key.into(),
        element,
        style,
    }
}

impl<E, F> RenderOnce for HoverEase<E, F>
where
    E: StatefulInteractiveElement + IntoElement + 'static,
    F: Fn(E, f32) -> E + 'static,
{
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let channel = window.use_keyed_state(self.key, cx, |_, _| Channel::new(false));
        let amount = channel.update(cx, |channel, cx| channel.frame(window, cx));
        let element = self.element.on_hover(move |hovered, _window, cx| {
            channel.update(cx, |channel, cx| {
                if channel.turn(*hovered) {
                    cx.notify();
                }
            });
        });
        (self.style)(element, amount)
    }
}

/// An element easing between its off and on looks over [`TRANSITION`] as `on` changes:
/// `style(element, amount)` draws it `amount` (0 to 1) of the way to on. `key` names its channel,
/// unique among its siblings.
#[derive(IntoElement)]
pub(crate) struct StateEase<E: IntoElement + 'static, F: Fn(E, f32) -> E + 'static> {
    key: ElementId,
    on: bool,
    element: E,
    style: F,
}

pub(crate) fn ease_state<E, F>(
    key: impl Into<ElementId>,
    on: bool,
    element: E,
    style: F,
) -> StateEase<E, F>
where
    E: IntoElement + 'static,
    F: Fn(E, f32) -> E + 'static,
{
    StateEase {
        key: key.into(),
        on,
        element,
        style,
    }
}

impl<E, F> RenderOnce for StateEase<E, F>
where
    E: IntoElement + 'static,
    F: Fn(E, f32) -> E + 'static,
{
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let on = self.on;
        let channel = window.use_keyed_state(self.key, cx, move |_, _| Channel::new(on));
        let amount = channel.update(cx, |channel, cx| {
            channel.turn(on);
            channel.frame(window, cx)
        });
        (self.style)(self.element, amount)
    }
}

/// An element drawn at a number that eases to each new value over [`TRANSITION`] -- a slider's
/// thumb that a profile or «Минимум тира» moves -- or jumps straight to it while `snap` (the
/// player drags it): `style(element, value)` draws it at the value reached. `key` names its
/// channel, unique among its siblings.
#[derive(IntoElement)]
pub(crate) struct ValueEase<E: IntoElement + 'static, F: Fn(E, f32) -> E + 'static> {
    key: ElementId,
    value: f32,
    snap: bool,
    element: E,
    style: F,
}

pub(crate) fn ease_value<E, F>(
    key: impl Into<ElementId>,
    value: f32,
    snap: bool,
    element: E,
    style: F,
) -> ValueEase<E, F>
where
    E: IntoElement + 'static,
    F: Fn(E, f32) -> E + 'static,
{
    ValueEase {
        key: key.into(),
        value,
        snap,
        element,
        style,
    }
}

/// One eased number: where it set off from, where it heads, and when it set off (`None` once
/// there).
struct Glide {
    from: f32,
    to: f32,
    set_off: Option<Instant>,
}

impl Glide {
    fn at(&self, now: Instant) -> f32 {
        let Some(set_off) = self.set_off else {
            return self.to;
        };
        let progress =
            now.saturating_duration_since(set_off).as_secs_f32() / TRANSITION.as_secs_f32();
        if progress >= 1. {
            self.to
        } else {
            self.from + (self.to - self.from) * ease(progress)
        }
    }
}

impl<E, F> RenderOnce for ValueEase<E, F>
where
    E: IntoElement + 'static,
    F: Fn(E, f32) -> E + 'static,
{
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let (value, snap) = (self.value, self.snap);
        let glide = window.use_keyed_state(self.key, cx, move |_, _| Glide {
            from: value,
            to: value,
            set_off: None,
        });
        let drawn = glide.update(cx, |glide, cx| {
            let now = Instant::now();
            if glide.to != value {
                glide.from = glide.at(now);
                glide.to = value;
                glide.set_off = (!snap).then_some(now);
            }
            let Some(set_off) = glide.set_off else {
                return value;
            };
            // As `Channel::frame`: frames only while it moves, none with reduced motion.
            if snap || cx.reduce_motion() || now.saturating_duration_since(set_off) >= TRANSITION {
                glide.set_off = None;
                return value;
            }
            window.request_animation_frame();
            glide.at(now)
        });
        (self.style)(self.element, drawn)
    }
}

/// `element` fading in as it rises into place over [`APPEAR`]: a panel, dialog or menu showing
/// up. A new `key` plays it again.
pub(crate) fn appear<E: Styled + IntoElement + 'static>(
    key: impl Into<ElementId>,
    element: E,
) -> AnimationElement<E> {
    rise_in(key, APPEAR, APPEAR_RISE, element)
}

/// `element` switched in -- a settings section, a checked row's roll slider -- the same way,
/// quicker and shorter: over [`TRANSITION`], so switching never jumps.
pub(crate) fn switch_in<E: Styled + IntoElement + 'static>(
    key: impl Into<ElementId>,
    element: E,
) -> AnimationElement<E> {
    rise_in(key, TRANSITION, SWITCH_RISE, element)
}

fn rise_in<E: Styled + IntoElement + 'static>(
    key: impl Into<ElementId>,
    duration: Duration,
    rise: f32,
    element: E,
) -> AnimationElement<E> {
    element.with_animation(
        key,
        Animation::new(duration).with_easing(ease),
        move |element, t| element.relative().top(px(rise * (1. - t))).opacity(t),
    )
}

/// The game's double gold frame over a window, panel or dialog: a line on the edge, a fainter
/// one just inside it, and at each corner a gold diamond with arms fading along the edges. Lay it
/// last, absolutely over the whole surface; it only draws and takes no clicks. A window drawing
/// it needs Windows 11's rounded corners off (`Win32Overlay::disable_dwm_frame`), or they cut
/// the corner diamonds.
pub(crate) fn game_frame() -> impl IntoElement {
    canvas(
        |_, _, _| {},
        |bounds, (), window, _| paint_frame(bounds, window),
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

fn paint_frame(bounds: Bounds<Pixels>, window: &mut Window) {
    let unit = window.rem_size() / px(BASE_REM_SIZE);
    window.paint_quad(outline(bounds, rgb(BORDER_GOLD), BorderStyle::Solid));
    window.paint_quad(outline(
        bounds.inset(px(FRAME_GAP * unit)),
        alpha(GOLD, FRAME_INNER_OPACITY),
        BorderStyle::Solid,
    ));
    let arm = px(FRAME_ARM * unit);
    let half = px(FRAME_DIAMOND * unit);
    for (corner, sx, sy) in [
        (bounds.origin, 1., 1.),
        (bounds.top_right(), -1., 1.),
        (bounds.bottom_left(), 1., -1.),
        (bounds.bottom_right(), -1., -1.),
    ] {
        paint_corner(corner, (sx, sy), arm, half, window);
    }
}

/// One frame corner at `corner`, its arms running along `direction`'s signs.
fn paint_corner(
    corner: Point<Pixels>,
    (sx, sy): (f32, f32),
    arm: Pixels,
    half: Pixels,
    window: &mut Window,
) {
    let hairline = px(1.);
    let reach = arm - half * 2.;
    let (arm_x, line_x) = if sx > 0. {
        (corner.x + half * 2., corner.x)
    } else {
        (corner.x - arm, corner.x - hairline)
    };
    let (arm_y, line_y) = if sy > 0. {
        (corner.y + half * 2., corner.y)
    } else {
        (corner.y - arm, corner.y - hairline)
    };
    // Each arm fades from the diamond outwards: CSS angles, 90 is left to right, 180 top down.
    let fading = |angle: f32| {
        linear_gradient(
            angle,
            linear_color_stop(rgb(GOLD), 0.),
            linear_color_stop(alpha(GOLD, 0.), 1.),
        )
    };
    window.paint_quad(fill(
        Bounds::new(point(arm_x, line_y), size(reach, hairline)),
        fading(if sx > 0. { 90. } else { 270. }),
    ));
    window.paint_quad(fill(
        Bounds::new(point(line_x, arm_y), size(hairline, reach)),
        fading(if sy > 0. { 180. } else { 0. }),
    ));
    let center = point(corner.x + half * sx, corner.y + half * sy);
    paint_diamond(center, half, rgb(GOLD), window);
    paint_diamond(center, half * 0.4, rgb(BG_TITLE), window);
}

fn paint_diamond(
    center: Point<Pixels>,
    half: Pixels,
    color: impl Into<Background>,
    window: &mut Window,
) {
    let mut path = PathBuilder::fill();
    path.move_to(point(center.x, center.y - half));
    path.line_to(point(center.x + half, center.y));
    path.line_to(point(center.x, center.y + half));
    path.line_to(point(center.x - half, center.y));
    path.close();
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

/// A small diamond `size` px across: the ornament at the heart of rules and frame corners.
pub(crate) fn diamond(size: f32, color: u32) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, (), window, _| {
            paint_diamond(bounds.center(), bounds.size.width / 2., rgb(color), window);
        },
    )
    .flex_none()
    .size(rems_from_px(size))
}

/// A rule with a diamond at its centre, its lines fading out towards the ends.
pub(crate) fn ornament_rule(color: u32) -> impl IntoElement {
    let line = |angle: f32| {
        div().flex_1().h(px(1.)).bg(linear_gradient(
            angle,
            linear_color_stop(alpha(color, 0.), 0.),
            linear_color_stop(rgb(color), 1.),
        ))
    };
    div()
        .flex()
        .items_center()
        .gap(rems_from_px(6.))
        .child(line(90.))
        .child(diamond(7., GOLD))
        .child(line(270.))
}

/// A `div` in the game's heading `face` (`ui::fonts`).
pub(crate) fn heading(face: &NameFont) -> Div {
    div().font_family(face.family).font_weight(face.weight)
}

/// A group's heading: a small diamond, the name in capitals in the heading face, and a rule
/// fading out to the right.
pub(crate) fn section_heading(face: &NameFont, title: &str) -> Div {
    div()
        .flex()
        .items_center()
        .gap(rems_from_px(7.))
        .child(diamond(6., GOLD))
        .child(
            heading(face)
                .flex_none()
                .text_size(rems_from_px(12.5))
                .text_color(rgb(GOLD))
                .child(title.to_uppercase()),
        )
        .child(div().flex_1().h(px(1.)).bg(linear_gradient(
            90.,
            linear_color_stop(rgb(BORDER_GOLD), 0.),
            linear_color_stop(alpha(BORDER_GOLD, 0.), 1.),
        )))
}

/// A title bar's background: the game's bronze at the top fading into black.
pub(crate) fn title_gradient() -> Background {
    linear_gradient(
        180.,
        linear_color_stop(rgb(TITLE_TOP), 0.),
        linear_color_stop(rgb(TITLE_BOTTOM), 1.),
    )
}

/// A title bar's button -- `⚙`, `×` -- as tall as the bar, lighting up under the pointer: red for
/// `close`, bronze otherwise.
pub(crate) fn title_button(
    key: impl Into<ElementId>,
    glyph: &'static str,
    width: f32,
    close: bool,
    on_press: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let key = key.into();
    let element = div()
        .id(key.clone())
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .w(rems_from_px(width))
        .h_full()
        .cursor_pointer()
        .on_mouse_down(MouseButton::Left, on_press)
        .text_size(rems_from_px(16.))
        .child(glyph);
    let lit = if close {
        BG_CLOSE_HOVER
    } else {
        BG_BUTTON_HOVER
    };
    ease_hover(key, element, move |element, hover| {
        element
            .bg(alpha(lit, hover))
            .text_color(rgb(blend(TEXT_DIM, TEXT, hover)))
    })
}

/// A card grouping rows: a step above the window, VibeTools' 8 px corners, a hairline between
/// its rows.
pub(crate) fn card(rows: impl IntoIterator<Item = gpui::AnyElement>) -> Div {
    let mut card = div()
        .flex()
        .flex_col()
        .p(rems_from_px(4.))
        .rounded(rems_from_px(CARD_RADIUS))
        .bg(rgb(BG_CARD))
        .border_1()
        .border_color(rgb(BORDER_CARD));
    for (index, row) in rows.into_iter().enumerate() {
        if index > 0 {
            card = card.child(div().h(px(1.)).mx(rems_from_px(14.)).bg(rgb(BORDER_ROW)));
        }
        card = card.child(row);
    }
    card
}

/// What a button does, by how it looks.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ButtonKind {
    /// The main action: a bronze plate in a gold edge, its label in the heading face.
    Primary,
    /// Any other action: dark, gold only under the pointer.
    Secondary,
    /// An action that erases something: a red label, red edge and glow under the pointer.
    Danger,
}

/// A button's size, px: its height, the space either side of its label, and the label's size --
/// a primary button's, in the heading face, a pixel larger.
#[derive(Clone, Copy)]
struct ButtonSize {
    height: f32,
    padding: f32,
    text: f32,
}

/// A window's buttons.
const BUTTON: ButtonSize = ButtonSize {
    height: CONTROL_HEIGHT,
    padding: 16.,
    text: 13.,
};
/// Buttons in a tight row: a card's actions.
const SMALL_BUTTON: ButtonSize = ButtonSize {
    height: 24.,
    padding: 10.,
    text: 12.,
};

/// A button, glowing under the pointer; `on_press` runs on the press. `face` sets a primary
/// button's label.
pub(crate) fn button(
    key: impl Into<ElementId>,
    label: impl Into<SharedString>,
    kind: ButtonKind,
    face: &NameFont,
    on_press: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    sized_button(key, label, kind, face, BUTTON, on_press)
}

/// A [`button`] for a tight row -- a card's actions: shorter, its label smaller.
pub(crate) fn small_button(
    key: impl Into<ElementId>,
    label: impl Into<SharedString>,
    kind: ButtonKind,
    face: &NameFont,
    on_press: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    sized_button(key, label, kind, face, SMALL_BUTTON, on_press)
}

fn sized_button(
    key: impl Into<ElementId>,
    label: impl Into<SharedString>,
    kind: ButtonKind,
    face: &NameFont,
    size: ButtonSize,
    on_press: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let key = key.into();
    let element = div()
        .id(key.clone())
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .h(rems_from_px(size.height))
        .px(rems_from_px(size.padding))
        .rounded(rems_from_px(CONTROL_RADIUS))
        .border_1()
        .cursor_pointer()
        .map(|this| match kind {
            ButtonKind::Primary => this
                .font_family(face.family)
                .font_weight(face.weight)
                .text_size(rems_from_px(size.text + 1.)),
            ButtonKind::Secondary | ButtonKind::Danger => this.text_size(rems_from_px(size.text)),
        })
        .on_mouse_down(MouseButton::Left, on_press)
        .child(label.into());
    ease_hover(key, element, move |element, hover| match kind {
        ButtonKind::Primary => element
            .bg(plate(hover))
            .border_color(rgb(blend(GOLD, GOLD_LIGHT, hover)))
            .text_color(rgb(GOLD_LIGHT))
            .shadow(glow(GOLD, hover)),
        ButtonKind::Secondary => element
            .bg(rgb(blend(BG_FIELD, BG_BUTTON_HOVER, hover)))
            .border_color(rgb(blend(BORDER_FIELD, GOLD, hover)))
            .text_color(rgb(blend(TEXT, GOLD_LIGHT, hover)))
            .shadow(glow(GOLD, 0.7 * hover)),
        ButtonKind::Danger => element
            .bg(rgb(blend(BG_FIELD, TEXT_WARNING, 0.1 * hover)))
            .border_color(rgb(blend(BORDER_DANGER, TEXT_WARNING, hover)))
            .text_color(rgb(TEXT_WARNING))
            .shadow(glow(TEXT_WARNING, 0.6 * hover)),
    })
}

/// A primary button's bronze plate, lit `hover` of the way.
pub(crate) fn plate(hover: f32) -> Background {
    linear_gradient(
        180.,
        linear_color_stop(rgb(blend(PLATE_TOP, GOLD, 0.16 * hover)), 0.),
        linear_color_stop(rgb(blend(PLATE_BOTTOM, GOLD, 0.08 * hover)), 1.),
    )
}

/// A switch, gold when on: the knob slides and the track fills over [`TRANSITION`]. It only
/// shows the state -- the row it sits in takes the click.
pub(crate) fn switch(key: impl Into<ElementId>, on: bool) -> impl IntoElement {
    let track = div()
        .relative()
        .flex_none()
        .w(rems_from_px(34.))
        .h(rems_from_px(18.))
        .rounded_full()
        .border_1();
    ease_state(key, on, track, |track, on| {
        track
            .bg(rgb(blend(BG_FIELD, blend(BG_FIELD, GOLD, 0.3), on)))
            .border_color(rgb(blend(BORDER_FIELD, GOLD, on)))
            .child(
                div()
                    .absolute()
                    .top(rems_from_px(2.))
                    .left(rems_from_px(2. + 16. * on))
                    .size(rems_from_px(12.))
                    .rounded_full()
                    .bg(rgb(blend(TEXT_DIM, GOLD_LIGHT, on))),
            )
    })
}

/// A checkbox: a gold square with a dark tick when checked, easing in over [`TRANSITION`].
pub(crate) fn checkbox(key: impl Into<ElementId>, checked: bool) -> impl IntoElement {
    let square = div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(rems_from_px(15.))
        .rounded(rems_from_px(3.))
        .border_1()
        .text_size(rems_from_px(11.))
        .line_height(rems_from_px(13.))
        .font_weight(FontWeight::BOLD);
    ease_state(key, checked, square, |square, on| {
        square
            .bg(alpha(GOLD, on))
            .border_color(rgb(blend(TEXT_MUTED, GOLD, on)))
            .text_color(alpha(BG_TITLE, on))
            .child("✓")
    })
}

/// Choices side by side in one frame, the picked one lit gold -- its light eases in over
/// [`TRANSITION`] as the last one's eases out. `on_pick` gets the index pressed (a
/// `cx.listener` fits).
pub(crate) fn segmented(
    key: impl Into<ElementId>,
    options: impl IntoIterator<Item = SharedString>,
    picked: usize,
    on_pick: impl Fn(&usize, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let on_pick = Rc::new(on_pick);
    div()
        .id(key)
        .flex()
        .flex_none()
        .gap(rems_from_px(2.))
        .p(rems_from_px(2.))
        .rounded(rems_from_px(CONTROL_RADIUS))
        .bg(rgb(BG_FIELD))
        .border_1()
        .border_color(rgb(BORDER_FIELD))
        .children(options.into_iter().enumerate().map(|(index, label)| {
            let on_pick = on_pick.clone();
            let lit = index == picked;
            let option = div()
                .id(index)
                .relative()
                .flex()
                .items_center()
                .h(rems_from_px(CONTROL_HEIGHT - 6.))
                .px(rems_from_px(12.))
                .rounded(rems_from_px(CONTROL_RADIUS - 1.))
                .text_size(rems_from_px(13.))
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    on_pick(&index, window, cx);
                });
            ease_hover(index, option, move |option, hover| {
                let label = label.clone();
                option
                    .child(ease_state(
                        "light",
                        lit,
                        div()
                            .absolute()
                            .inset_0()
                            .rounded(rems_from_px(CONTROL_RADIUS - 1.))
                            .border_1(),
                        |light, on| {
                            light
                                .bg(alpha(GOLD, 0.16 * on))
                                .border_color(alpha(GOLD, 0.6 * on))
                        },
                    ))
                    .child(ease_state(
                        "label",
                        lit,
                        div().relative(),
                        move |text, on| {
                            text.text_color(rgb(blend(
                                blend(TEXT_DIM, TEXT, hover),
                                GOLD_LIGHT,
                                on,
                            )))
                            .child(label.clone())
                        },
                    ))
            })
        }))
}

/// A select: the current choice in a field with a gold ▾, glowing under the pointer; `on_press`
/// opens or closes its [`menu`]. It answers in the capture phase, before an open menu's own
/// outside-press handling, so pressing it again closes the menu instead of reopening it.
pub(crate) fn select(
    key: impl Into<ElementId>,
    choice: impl IntoElement,
    compact: bool,
    on_press: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let key = key.into();
    let element = div()
        .id(key.clone())
        .flex()
        .items_center()
        .gap(rems_from_px(8.))
        .rounded(rems_from_px(CONTROL_RADIUS))
        .border_1()
        .cursor_pointer()
        .map(|this| {
            if compact {
                // Gives way on a row too narrow for it, its choice truncating.
                this.min_w_0()
                    .h(rems_from_px(22.))
                    .px(rems_from_px(8.))
                    .text_size(rems_from_px(12.))
            } else {
                this.flex_none()
                    .h(rems_from_px(CONTROL_HEIGHT))
                    .min_w(rems_from_px(240.))
                    .px(rems_from_px(12.))
                    .text_size(rems_from_px(13.))
            }
        })
        .capture_any_mouse_down(move |event, window, cx| {
            if event.button == MouseButton::Left {
                on_press(event, window, cx);
            }
        })
        .child(div().flex_1().min_w_0().truncate().child(choice))
        .child(div().flex_none().text_color(rgb(GOLD)).child("▾"));
    ease_hover(key, element, move |element, hover| {
        element
            .bg(rgb(blend(BG_FIELD, BG_BUTTON_HOVER, 0.6 * hover)))
            .border_color(rgb(blend(BORDER_FIELD, GOLD, hover)))
            .text_color(rgb(blend(TEXT, GOLD_LIGHT, hover)))
            .shadow(glow(GOLD, 0.6 * hover))
    })
}

/// A select's open list, framed and casting the popup shadow, the current choice marked with a
/// diamond; it appears over [`APPEAR`], just below whatever it follows (a zero-height slot keeps
/// the anchor at the select's bottom-left) and over everything else. `on_pick` gets the index
/// pressed, `on_dismiss` any press outside the list (a `cx.listener` fits either).
pub(crate) fn menu(
    key: impl Into<ElementId>,
    options: Vec<SharedString>,
    picked: usize,
    width: f32,
    on_pick: impl Fn(&usize, &mut Window, &mut App) + 'static,
    on_dismiss: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let on_pick = Rc::new(on_pick);
    let rows = options.into_iter().enumerate().map(|(index, label)| {
        let on_pick = on_pick.clone();
        menu_row(index, index == picked, label, move |_, window, cx| {
            on_pick(&index, window, cx);
        })
    });
    let list = menu_list(key, rows)
        .w(rems_from_px(width))
        .on_mouse_down_out(on_dismiss);
    div().h_0().child(
        deferred(
            anchored()
                .anchor(Anchor::TopLeft)
                .offset(point(px(0.), px(4.)))
                .child(appear("menu", list)),
        )
        .with_priority(1),
    )
}

/// A menu's list: `rows` ([`menu_row`]) framed, over the popup shadow. [`menu`] makes one from
/// labels; a menu whose rows say more, or that closes some other way (the price panel's, through
/// a backdrop), builds its own from this and sizes it.
pub(crate) fn menu_list(
    key: impl Into<ElementId>,
    rows: impl IntoIterator<Item = impl IntoElement>,
) -> Stateful<Div> {
    div()
        .id(key)
        .relative()
        .flex()
        .flex_col()
        .p(rems_from_px(5.))
        .bg(rgb(BG_MENU))
        .shadow(popup_shadow())
        .occlude()
        .children(rows)
        .child(game_frame())
}

/// One choice in a [`menu_list`]: `content` behind the diamond that marks the `current` choice,
/// lit gold under the pointer; `on_pick` runs on the press. A row taller than one line -- a note
/// under the choice -- grows to fit.
pub(crate) fn menu_row(
    index: usize,
    current: bool,
    content: impl IntoElement,
    on_pick: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let item = div()
        .id(index)
        .flex()
        .items_center()
        .gap(rems_from_px(8.))
        .min_h(rems_from_px(28.))
        .py(rems_from_px(3.))
        .px(rems_from_px(10.))
        .rounded(rems_from_px(CONTROL_RADIUS - 1.))
        .text_size(rems_from_px(13.))
        .cursor_pointer()
        .on_mouse_down(MouseButton::Left, on_pick)
        .child(
            div()
                .flex()
                .flex_none()
                .w(rems_from_px(7.))
                .when(current, |this| this.child(diamond(7., GOLD))),
        )
        .child(content);
    ease_hover(index, item, move |item, hover| {
        item.bg(alpha(GOLD, 0.1 * hover))
            .text_color(rgb(if current {
                GOLD_LIGHT
            } else {
                blend(TEXT, GOLD_LIGHT, hover)
            }))
    })
}

/// A key as a keycap: `Ctrl`, `E`.
pub(crate) fn keycap(label: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .min_w(rems_from_px(24.))
        .h(rems_from_px(22.))
        .px(rems_from_px(7.))
        .rounded(rems_from_px(CONTROL_RADIUS))
        .bg(linear_gradient(
            180.,
            linear_color_stop(rgb(KEY_TOP), 0.),
            linear_color_stop(rgb(BG_FIELD), 1.),
        ))
        .border_1()
        .border_b_2()
        .border_color(rgb(BORDER_FIELD))
        .text_size(rems_from_px(12.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(TEXT))
        .child(label.into())
}

/// A hotkey as keycaps joined by `+`.
pub(crate) fn keycaps<S: Into<SharedString> + Clone>(keys: &[S]) -> Div {
    let mut row = div().flex().items_center().gap(rems_from_px(4.));
    for (index, key) in keys.iter().enumerate() {
        if index > 0 {
            row = row.child(div().text_color(rgb(TEXT_MUTED)).child("+"));
        }
        row = row.child(keycap(key.clone()));
    }
    row
}

/// A hotkey recorder showing `content` -- the hotkey as [`keycaps`], or what it waits for; its
/// edge warms to gold under the pointer, and while `recording` a gold ring with a glow eases in
/// around it. `on_press` starts or stops the recording.
pub(crate) fn recorder(
    key: impl Into<ElementId>,
    content: impl IntoElement,
    recording: bool,
    on_press: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let key = key.into();
    let element = div()
        .id(key.clone())
        .relative()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .min_w(rems_from_px(150.))
        .h(rems_from_px(CONTROL_HEIGHT))
        .px(rems_from_px(8.))
        .rounded(rems_from_px(CONTROL_RADIUS))
        .bg(rgb(BG_FIELD))
        .border_1()
        .text_size(rems_from_px(13.))
        .cursor_pointer()
        .on_mouse_down(MouseButton::Left, on_press)
        .child(content)
        .child(ease_state(
            "recording",
            recording,
            div()
                .absolute()
                .inset_0()
                .rounded(rems_from_px(CONTROL_RADIUS))
                .border_1()
                .border_color(rgb(GOLD)),
            |ring, on| ring.opacity(on).shadow(glow(GOLD, 0.8 * on)),
        ));
    ease_hover(key, element, |element, hover| {
        element
            .border_color(rgb(blend(BORDER_FIELD, GOLD, 0.7 * hover)))
            .shadow(glow(GOLD, 0.4 * hover))
    })
}

/// A number with − and + either side; a button glows under the pointer and dims at its end of
/// the range. `on_step` gets `true` for + (a `cx.listener` fits).
pub(crate) fn stepper(
    key: impl Into<ElementId>,
    value: impl Into<SharedString>,
    (can_decrease, can_increase): (bool, bool),
    on_step: impl Fn(&bool, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let on_step = Rc::new(on_step);
    let step_button = |up: bool, enabled: bool| {
        let on_step = on_step.clone();
        let element = div()
            .id(usize::from(up))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .size(rems_from_px(26.))
            .rounded(rems_from_px(CONTROL_RADIUS))
            .bg(rgb(BG_FIELD))
            .border_1()
            .text_size(rems_from_px(15.))
            .child(if up { "+" } else { "−" })
            .when(enabled, |this| {
                this.cursor_pointer()
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        on_step(&up, window, cx);
                    })
            });
        ease_hover(usize::from(up), element, move |element, hover| {
            let hover = if enabled { hover } else { 0. };
            element
                .border_color(rgb(blend(BORDER_FIELD, GOLD, hover)))
                .text_color(rgb(if enabled {
                    blend(GOLD, GOLD_LIGHT, hover)
                } else {
                    TEXT_MUTED
                }))
                .shadow(glow(GOLD, 0.6 * hover))
        })
    };
    div()
        .id(key)
        .flex()
        .flex_none()
        .items_center()
        .gap(rems_from_px(4.))
        .child(step_button(false, can_decrease))
        .child(
            div()
                .w(rems_from_px(56.))
                .text_center()
                .text_size(rems_from_px(14.))
                .child(value.into()),
        )
        .child(step_button(true, can_increase))
}

/// A small square button with a glyph -- `×` removing a row: red under the pointer when it
/// erases, bronze otherwise.
pub(crate) fn icon_button(
    key: impl Into<ElementId>,
    glyph: &'static str,
    erases: bool,
    on_press: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let key = key.into();
    let element = div()
        .id(key.clone())
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(rems_from_px(26.))
        .rounded(rems_from_px(CONTROL_RADIUS))
        .text_size(rems_from_px(15.))
        .cursor_pointer()
        .on_mouse_down(MouseButton::Left, on_press)
        .child(glyph);
    let lit = if erases {
        BG_CLOSE_HOVER
    } else {
        BG_BUTTON_HOVER
    };
    ease_hover(key, element, move |element, hover| {
        element
            .bg(alpha(lit, hover))
            .text_color(rgb(blend(TEXT_MUTED, TEXT, hover)))
    })
}

/// A fact about the item as a chip: `label` dimmed, then `value` in `value_color`.
pub(crate) fn chip(
    label: Option<&'static str>,
    value: impl Into<SharedString>,
    value_color: u32,
) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(rems_from_px(4.))
        .h(rems_from_px(22.))
        .px(rems_from_px(8.))
        .rounded(rems_from_px(CONTROL_RADIUS))
        .bg(rgb(BG_CARD))
        .text_size(rems_from_px(12.))
        .children(label.map(|label| div().text_color(rgb(TEXT_DIM)).child(label)))
        .child(div().text_color(rgb(value_color)).child(value.into()))
}

/// A chip a press turns to its other state -- edged, with a gold ↔, glowing under the pointer.
pub(crate) fn toggle_chip(
    key: impl Into<ElementId>,
    label: Option<&'static str>,
    value: impl Into<SharedString>,
    on_press: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let key = key.into();
    let chip = pressable_chip(key.clone(), on_press)
        .gap(rems_from_px(4.))
        .children(label.map(|label| div().text_color(rgb(TEXT_DIM)).child(label)))
        .child(div().text_color(rgb(TEXT)).child(value.into()))
        .child(div().text_color(rgb(GOLD)).child("↔"));
    chip_hover(key, chip)
}

/// A chip a press checks -- a row left out of the search, folded until it's picked: an empty
/// checkbox before `content`, in [`toggle_chip`]'s edged look.
pub(crate) fn check_chip(
    key: impl Into<ElementId>,
    content: impl IntoElement,
    on_press: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let key = key.into();
    let chip = pressable_chip(key.clone(), on_press)
        .gap(rems_from_px(6.))
        .child(
            div()
                .flex_none()
                .size(rems_from_px(12.))
                .rounded(rems_from_px(2.))
                .border_1()
                .border_color(rgb(TEXT_MUTED)),
        )
        .child(content);
    chip_hover(key, chip)
}

/// The frame [`toggle_chip`] and [`check_chip`] share: edged, and a press runs `on_press`.
fn pressable_chip(
    key: ElementId,
    on_press: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(key)
        .flex()
        .flex_none()
        .items_center()
        .h(rems_from_px(22.))
        .px(rems_from_px(8.))
        .rounded(rems_from_px(CONTROL_RADIUS))
        .bg(rgb(BG_FIELD))
        .border_1()
        .text_size(rems_from_px(12.))
        .cursor_pointer()
        .on_mouse_down(MouseButton::Left, on_press)
}

/// A [`pressable_chip`]'s edge warming to gold, with a glow, under the pointer.
fn chip_hover(key: ElementId, chip: Stateful<Div>) -> impl IntoElement {
    ease_hover(key, chip, |chip, hover| {
        chip.border_color(rgb(blend(BORDER_FIELD, GOLD, hover)))
            .shadow(glow(GOLD, 0.6 * hover))
    })
}

/// A text link, warming from dim to gold under the pointer; `on_press` follows it.
pub(crate) fn link(
    key: impl Into<ElementId>,
    label: impl Into<SharedString>,
    on_press: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let key = key.into();
    let element = div()
        .id(key.clone())
        .flex_none()
        .text_size(rems_from_px(12.))
        .cursor_pointer()
        .on_mouse_down(MouseButton::Left, on_press)
        .child(label.into());
    ease_hover(key, element, |element, hover| {
        element.text_color(rgb(blend(TEXT_DIM, GOLD_LIGHT, hover)))
    })
}

/// A tooltip in the game's frame: an optional heading line over its lines of text in their
/// colours, casting the tooltip shadow; it fades in over [`TRANSITION`]. For `tooltip(...)`.
pub(crate) fn game_hint(
    face: &'static NameFont,
    title: Option<&'static str>,
    lines: Vec<(SharedString, u32)>,
) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    move |_window, cx| {
        let lines = lines.clone();
        cx.new(|_| GameHint { face, title, lines }).into()
    }
}

/// A tooltip is at most this wide, px at 100 % scale; a longer line wraps.
const HINT_MAX_WIDTH: f32 = 300.;
const HINT_PADDING_X: f32 = 12.;
const HINT_TEXT_SIZE: f32 = 12.;
const HINT_TITLE_SIZE: f32 = 13.5;

struct GameHint {
    face: &'static NameFont,
    title: Option<&'static str>,
    lines: Vec<(SharedString, u32)>,
}

impl GameHint {
    /// The box's width: its widest line's, up to [`HINT_MAX_WIDTH`] -- given outright, not as a
    /// `max_w`. GPUI lays a tooltip out with no width to fit into (`AvailableSpace::MinContent`),
    /// where text doesn't wrap: under a mere `max_w` the box kept one line's height while its
    /// text, wrapped at the capped width when painted, ran on below the frame.
    fn width(&self, window: &Window) -> Pixels {
        let rem = window.rem_size();
        let body = window.text_style().font();
        let heading = Font {
            family: self.face.family.into(),
            weight: self.face.weight,
            ..body.clone()
        };
        let widest = |text: &str, font: &Font, size: f32| {
            let size = rems_from_px(size).to_pixels(rem);
            text.split('\n')
                .map(|line| {
                    let run = TextRun {
                        len: line.len(),
                        font: font.clone(),
                        color: Hsla::default(),
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    };
                    window
                        .text_system()
                        .shape_line(SharedString::from(line.to_owned()), size, &[run], None)
                        .width
                })
                .fold(px(0.), Pixels::max)
        };
        let text = self
            .lines
            .iter()
            .map(|(line, _)| widest(line, &body, HINT_TEXT_SIZE))
            .chain(
                self.title
                    .map(|title| widest(title, &heading, HINT_TITLE_SIZE)),
            )
            .fold(px(0.), Pixels::max);
        // A pixel over the measure: a line exactly as wide as its box may still wrap.
        let padded = text + rems_from_px(2. * HINT_PADDING_X).to_pixels(rem) + px(1.);
        padded.min(rems_from_px(HINT_MAX_WIDTH).to_pixels(rem))
    }
}

impl Render for GameHint {
    fn render(&mut self, window: &mut Window, _cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let width = self.width(window);
        switch_in(
            "hint",
            div()
                .relative()
                .flex()
                .flex_col()
                .gap(rems_from_px(3.))
                .w(width)
                .px(rems_from_px(HINT_PADDING_X))
                .py(rems_from_px(9.))
                .bg(rgb(BG_MENU))
                .shadow(tooltip_shadow())
                .text_size(rems_from_px(HINT_TEXT_SIZE))
                .children(self.title.map(|title| {
                    heading(self.face)
                        .text_size(rems_from_px(HINT_TITLE_SIZE))
                        .text_color(rgb(GOLD_LIGHT))
                        .child(title)
                }))
                .children(
                    self.lines
                        .iter()
                        .map(|(line, color)| div().text_color(rgb(*color)).child(line.clone())),
                )
                .child(game_frame()),
        )
    }
}
