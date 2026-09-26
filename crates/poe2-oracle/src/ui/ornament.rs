//! The game-styled ornaments `ui::style` draws -- the double gold frame, diamonds, the fading
//! rules -- as exact device pixels: where each part lands on the device-pixel grid, and the
//! pixels of every part that isn't a straight solid line, rasterised at device resolution with
//! exact area coverage. Platform-free, so its tests and `examples/ornaments.rs` run on every
//! target; `ui::style` paints the parts through GPUI.
//!
//! **A frame corner.** The outer line runs round the corner in bright gold, which fades evenly
//! into the frame's own dim gold along both edges within [`FRAME_ARM`]. The inner line,
//! [`FRAME_GAP`] in, ends in a diamond set on its corner: the diamond is centred on the inner
//! line's corner point, the inner line's two arms begin at its two inner tips, its two outer
//! tips touch the outer line from inside -- the gold's fade begins there -- and a dark core sits
//! at its heart. Between the diamond and the outer corner lies a small triangle of whatever is
//! under the frame; no line runs behind the diamond. Everything stays inside the frame's bounds,
//! so a window's edge cuts nothing.
//!
//! **The keep-out.** Nothing inside a frame -- no control's border or fill, no hover fill, no
//! text, no scrolled content -- comes nearer the frame's edge than [`FRAME_CLEAR`]: the inner
//! line and a margin of the surface's own background past it, at least 4 logical px at every
//! display and UI scale, so the inner line always reads as a line of its own. A surface's own
//! background -- a title bar's bronze, an item's banner -- may run under the frame; a soft glow
//! may spill into the margin, being no edge. A title bar centres its content between the frame's
//! inner line and its own bottom rule, which puts a 22 px line of it just clear.
//!
//! **Device pixels.** A length is the app's px times the UI scale (a rem over 16 px) times the
//! display's scale factor, rounded as GPUI rounds a layout's; a hairline is one logical px in
//! whole device pixels, as GPUI draws a 1 px border. A diamond takes the parity of the line it
//! sits on, so the two share one axis: odd on an odd line, centred on a pixel's centre with a
//! single-pixel tip; even on an even one (2 px lines at 200 %), centred between two pixels with
//! a two-pixel tip the line's two rows run into. A free diamond is odd.
//!
//! **Sprites and quads.** A straight solid line is a quad on whole device pixels. A corner (the
//! diamond, the ends of both lines and the fade), a diamond and a fading line are sprites painted
//! 1:1 on the device grid, their pixels BGRA with straight alpha -- what GPUI's `RenderImage`
//! takes: its sprite shader returns the texel's colour with only the alpha scaled, and the
//! renderer blends `SRC_ALPHA, INV_SRC_ALPHA` into a `B8G8R8A8_UNORM` target, in the
//! sRGB-encoded bytes, exactly as it blends a quad ([`Picture`] composes the same way). A corner
//! sprite ends where its lines' quads begin, the same colour and alpha on both sides of the seam,
//! and no pixel is painted twice. [`SpriteCache`] keeps the sprites by what determines their
//! pixels.

use std::collections::HashMap;

use image::{Rgba, RgbaImage};

use crate::ui::theme::{BG_TITLE, BORDER_GOLD, GOLD};

/// The frame's inner line: this far inside its outer edge, px.
pub const FRAME_GAP: f32 = 4.;
/// The least of a framed surface's own background past the inner line, px.
const FRAME_MARGIN: f32 = 6.;
/// The frame's keep-out, px from its edge: the inner line, its hairline, and [`FRAME_MARGIN`]
/// of background. At the smallest UI scale on any display that still leaves 4 logical px.
pub const FRAME_CLEAR: f32 = FRAME_GAP + 1. + FRAME_MARGIN;
/// A frame corner's bright gold has faded into the frame's own this far from the corner, px.
pub const FRAME_ARM: f32 = 26.;
/// The frame's inner line is this share of the gold, 30 %, as the alpha byte its quads and its
/// sprite pixels both carry: the two meet without a seam.
pub const FRAME_INNER_ALPHA: u8 = 77;
/// A diamond's dark core, as a share of its size.
const CORE_SHARE: f32 = 0.4;
/// A rule's diamond, px across.
pub const RULE_DIAMOND: f32 = 7.;
/// A rule's lines stop this far short of its diamond, px.
const RULE_GAP: f32 = 6.;
/// Sprites kept at once: every window's corners, diamonds and rule lines at a couple of scales,
/// with room to spare.
pub const CACHE_CAPACITY: usize = 128;

/// GPUI's rounding of device lengths and edges: to the nearest whole pixel, halves toward zero.
pub fn round_half_toward_zero(value: f32) -> f32 {
    (value.abs() - 0.5).ceil().copysign(value)
}

/// How the app's px land on the device: `device` is the display's scale factor, `unit` the UI
/// scale (a rem over 16 px).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scale {
    pub device: f32,
    pub unit: f32,
}

impl Scale {
    /// `px` app px in whole device pixels, rounded as GPUI rounds a layout's lengths.
    pub fn length(self, px: f32) -> i32 {
        round_half_toward_zero(px * self.unit * self.device) as i32
    }

    /// A hairline: one logical px in whole device pixels, at least one, as GPUI draws a 1 px
    /// border. It doesn't follow the UI scale.
    pub fn hairline(self) -> i32 {
        (round_half_toward_zero(self.device) as i32).max(1)
    }

    /// The device pixel a logical coordinate of GPUI's layout lands on.
    pub fn device(self, logical: f32) -> i32 {
        round_half_toward_zero(logical * self.device) as i32
    }

    /// Device coordinate `device` as the logical one GPUI snaps back to exactly it.
    pub fn logical(self, device: i32) -> f32 {
        device as f32 / self.device
    }
}

/// A rectangle of whole device pixels: columns `left..right`, rows `top..bottom`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct DeviceRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl DeviceRect {
    pub const fn new(left: i32, top: i32, right: i32, bottom: i32) -> DeviceRect {
        DeviceRect {
            left,
            top,
            right,
            bottom,
        }
    }

    pub fn width(self) -> i32 {
        self.right - self.left
    }

    pub fn height(self) -> i32 {
        self.bottom - self.top
    }

    pub fn is_empty(self) -> bool {
        self.right <= self.left || self.bottom <= self.top
    }
}

/// A frame's lengths in device pixels at one scale: everything a corner's pixels depend on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FrameMetrics {
    /// Both lines' thickness: a hairline.
    pub line: i32,
    /// From the frame's edge to the inner line.
    pub gap: i32,
    /// From the frame's edge to where a corner's gold has faded: a corner sprite's side.
    pub arm: i32,
    /// The corner diamond, across: centred on the inner line's corner, its outer tips on the
    /// outer line's inner edge.
    pub diamond: i32,
    /// Its dark core, across.
    pub core: i32,
}

impl FrameMetrics {
    pub fn new(scale: Scale) -> FrameMetrics {
        let line = scale.hairline();
        // A line's width of room between the lines, at the least.
        let gap = scale.length(FRAME_GAP).max(2 * line);
        let diamond = 2 * gap - line;
        FrameMetrics {
            line,
            gap,
            arm: scale.length(FRAME_ARM).max(2 * gap + 1),
            diamond,
            core: with_parity(diamond as f32 * CORE_SHARE, diamond % 2),
        }
    }

    /// The inner line's corner, where the diamond is centred: this far from the frame's edge,
    /// both ways -- the middle of the inner line.
    fn centre(self) -> f64 {
        f64::from(self.gap) + f64::from(self.line) / 2.
    }
}

/// A frame's corner.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// A solid line: a quad on whole device pixels, `color` at `alpha`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Line {
    pub rect: DeviceRect,
    pub color: u32,
    pub alpha: u8,
}

/// A sprite on the device grid: its pixels cover `rect` 1:1, and the part in `shown` is painted
/// -- all of it, but where a small frame's corners meet halfway.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placed {
    pub sprite: Sprite,
    pub rect: DeviceRect,
    pub shown: DeviceRect,
}

impl Placed {
    fn whole(sprite: Sprite, rect: DeviceRect) -> Placed {
        Placed {
            sprite,
            rect,
            shown: rect,
        }
    }
}

/// A frame's parts: the lines' straight runs between the corners, the outer line's four then
/// the inner line's four (empty where a small frame's corners meet), and the corners.
pub struct FrameParts {
    pub lines: [Line; 8],
    pub corners: [Placed; 4],
}

/// The double gold frame over `rect`, its lines on its edge and `metrics.gap` inside it.
pub fn frame(rect: DeviceRect, metrics: FrameMetrics) -> FrameParts {
    let FrameMetrics { line, gap, arm, .. } = metrics;
    let (width, height) = (rect.width(), rect.height());
    // Each corner keeps to its quarter: on a frame too small for two arms, they meet halfway.
    let left = arm.min(width / 2);
    let right = arm.min(width - width / 2);
    let top = arm.min(height / 2);
    let bottom = arm.min(height - height / 2);
    let (run_left, run_right) = (rect.left + left, rect.right - right);
    let (run_top, run_bottom) = (rect.top + top, rect.bottom - bottom);
    let outer = |rect| Line {
        rect,
        color: BORDER_GOLD,
        alpha: u8::MAX,
    };
    let inner = |rect| Line {
        rect,
        color: GOLD,
        alpha: FRAME_INNER_ALPHA,
    };
    let lines = [
        outer(DeviceRect::new(
            run_left,
            rect.top,
            run_right,
            rect.top + line,
        )),
        outer(DeviceRect::new(
            run_left,
            rect.bottom - line,
            run_right,
            rect.bottom,
        )),
        outer(DeviceRect::new(
            rect.left,
            run_top,
            rect.left + line,
            run_bottom,
        )),
        outer(DeviceRect::new(
            rect.right - line,
            run_top,
            rect.right,
            run_bottom,
        )),
        inner(DeviceRect::new(
            run_left,
            rect.top + gap,
            run_right,
            rect.top + gap + line,
        )),
        inner(DeviceRect::new(
            run_left,
            rect.bottom - gap - line,
            run_right,
            rect.bottom - gap,
        )),
        inner(DeviceRect::new(
            rect.left + gap,
            run_top,
            rect.left + gap + line,
            run_bottom,
        )),
        inner(DeviceRect::new(
            rect.right - gap - line,
            run_top,
            rect.right - gap,
            run_bottom,
        )),
    ];
    let corner = |corner, x: i32, y: i32, shown| Placed {
        sprite: Sprite::Corner { corner, metrics },
        rect: DeviceRect::new(x, y, x + arm, y + arm),
        shown,
    };
    let corners = [
        corner(
            Corner::TopLeft,
            rect.left,
            rect.top,
            DeviceRect::new(rect.left, rect.top, run_left, run_top),
        ),
        corner(
            Corner::TopRight,
            rect.right - arm,
            rect.top,
            DeviceRect::new(run_right, rect.top, rect.right, run_top),
        ),
        corner(
            Corner::BottomLeft,
            rect.left,
            rect.bottom - arm,
            DeviceRect::new(rect.left, run_bottom, run_left, rect.bottom),
        ),
        corner(
            Corner::BottomRight,
            rect.right - arm,
            rect.bottom - arm,
            DeviceRect::new(run_right, run_bottom, rect.right, rect.bottom),
        ),
    ];
    FrameParts { lines, corners }
}

/// A `size` px diamond of `color` centred in `rect`: `on_line` for one sharing a line's axis
/// (a rule's, a heading's), which takes the line's parity; odd otherwise.
pub fn diamond(rect: DeviceRect, size: f32, scale: Scale, color: u32, on_line: bool) -> Placed {
    let parity = if on_line { scale.hairline() % 2 } else { 1 };
    let size = with_parity(size * scale.unit * scale.device, parity);
    let (left, top) = (
        centred(rect.left, rect.right, size),
        centred(rect.top, rect.bottom, size),
    );
    Placed::whole(
        Sprite::Diamond { size, color },
        DeviceRect::new(left, top, left + size, top + size),
    )
}

/// A hairline of `color` across `rect` on its axis, fading in from nothing along its length
/// (`rising`) or out to nothing.
pub fn fading_line(rect: DeviceRect, scale: Scale, color: u32, rising: bool) -> Option<Placed> {
    let thickness = scale.hairline();
    let top = centred(rect.top, rect.bottom, thickness);
    let length = rect.width();
    (length > 0).then(|| {
        Placed::whole(
            Sprite::Fade {
                length,
                thickness,
                color,
                rising,
            },
            DeviceRect::new(rect.left, top, rect.right, top + thickness),
        )
    })
}

/// A rule across `rect`: a [`RULE_DIAMOND`] px diamond of `diamond_color` at its centre and a
/// line of `color` either side, fading out toward the ends, all on one axis.
pub fn rule(rect: DeviceRect, scale: Scale, color: u32, diamond_color: u32) -> [Option<Placed>; 3] {
    let middle = diamond(rect, RULE_DIAMOND, scale, diamond_color, true);
    let gap = scale.length(RULE_GAP);
    let side = |left, right, rising| {
        fading_line(
            DeviceRect::new(left, rect.top, right, rect.bottom),
            scale,
            color,
            rising,
        )
    };
    [
        side(rect.left, middle.rect.left - gap, true),
        Some(middle),
        side(middle.rect.right + gap, rect.right, false),
    ]
}

/// A sprite, by everything its pixels depend on: [`rasterize`] draws it, [`SpriteCache`] keeps
/// it by this.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sprite {
    /// A frame's corner, `metrics.arm` square: the diamond, the lines' ends and the gold's fade.
    Corner {
        corner: Corner,
        metrics: FrameMetrics,
    },
    /// A solid diamond `size` px across in `color`, its tips at the sprite's edges.
    Diamond { size: i32, color: u32 },
    /// A line `length` by `thickness` px in `color`, fading in from nothing along its length
    /// (`rising`) or out to nothing.
    Fade {
        length: i32,
        thickness: i32,
        color: u32,
        rising: bool,
    },
}

/// `sprite`'s pixels: BGRA with straight alpha, as GPUI's `RenderImage` takes them.
pub fn rasterize(sprite: &Sprite) -> RgbaImage {
    match *sprite {
        Sprite::Corner { corner, metrics } => corner_pixels(corner, metrics),
        Sprite::Diamond { size, color } => {
            let half = f64::from(size) / 2.;
            RgbaImage::from_fn(side(size), side(size), |x, y| {
                let cover = coverage(f64::from(x), f64::from(y), (half, half), half);
                Paint::CLEAR.layered(channels(color), cover).bgra()
            })
        }
        Sprite::Fade {
            length,
            thickness,
            color,
            rising,
        } => RgbaImage::from_fn(side(length), side(thickness), |x, _| {
            let along = (f64::from(x) + 0.5) / f64::from(length);
            let alpha = if rising { along } else { 1. - along };
            Paint::CLEAR.layered(channels(color), alpha).bgra()
        }),
    }
}

fn side(length: i32) -> u32 {
    length.max(0) as u32
}

/// A corner's pixels: the top left one's, mirrored into place.
fn corner_pixels(corner: Corner, metrics: FrameMetrics) -> RgbaImage {
    let top_left = top_left_corner(metrics);
    let (mirror_x, mirror_y) = match corner {
        Corner::TopLeft => return top_left,
        Corner::TopRight => (true, false),
        Corner::BottomLeft => (false, true),
        Corner::BottomRight => (true, true),
    };
    let last = side(metrics.arm) - 1;
    RgbaImage::from_fn(side(metrics.arm), side(metrics.arm), |x, y| {
        let x = if mirror_x { last - x } else { x };
        let y = if mirror_y { last - y } else { y };
        *top_left.get_pixel(x, y)
    })
}

fn top_left_corner(metrics: FrameMetrics) -> RgbaImage {
    let FrameMetrics {
        line,
        gap,
        arm,
        diamond,
        core,
    } = metrics;
    let centre = metrics.centre();
    // The outer line's gold, `along` px from the corner (a pixel's centre): bright up to the
    // diamond's outer tips, then fading evenly to the frame's own at the sprite's last pixel,
    // where the line's quad carries it on.
    let fade_end = f64::from(arm) - 0.5;
    let outer = |along: f64| {
        let bright = ((fade_end - along) / (fade_end - centre)).clamp(0., 1.);
        mix(BORDER_GOLD, GOLD, bright)
    };
    // The inner line runs in under the diamond as far as its centre.
    let inner_from = centre.floor() as i32;
    let on_inner =
        |across: i32, along: i32| (gap..gap + line).contains(&across) && along >= inner_from;
    let inner_alpha = f64::from(FRAME_INNER_ALPHA) / 255.;
    let (half, core_half) = (f64::from(diamond) / 2., f64::from(core) / 2.);
    RgbaImage::from_fn(side(arm), side(arm), |x, y| {
        let (x, y) = (x as i32, y as i32);
        let mut paint = Paint::CLEAR;
        if x < line || y < line {
            let along = if x < line && y < line {
                0.5
            } else if y < line {
                f64::from(x) + 0.5
            } else {
                f64::from(y) + 0.5
            };
            paint = paint.layered(outer(along), 1.);
        }
        if on_inner(y, x) || on_inner(x, y) {
            paint = paint.layered(channels(GOLD), inner_alpha);
        }
        let (px, py) = (f64::from(x), f64::from(y));
        paint = paint.layered(channels(GOLD), coverage(px, py, (centre, centre), half));
        paint = paint.layered(
            channels(BG_TITLE),
            coverage(px, py, (centre, centre), core_half),
        );
        paint.bgra()
    })
}

/// The sprites in use, as whatever the painter makes of them (GPUI images in the app): each built
/// on first use and kept while it's among the `capacity` used last. A new device geometry -- the
/// window moved to a screen of another scale, the UI scale changed, a rule of a new length --
/// makes new sprites; ones no longer drawn age out, and [`SpriteCache::get`] hands back the one it
/// pushes out, for the painter to free. The map is sized for `capacity` up front, so a lookup or a
/// replacement never allocates.
pub struct SpriteCache<V> {
    kept: HashMap<Sprite, Kept<V>>,
    capacity: usize,
    clock: u64,
}

struct Kept<V> {
    value: V,
    used: u64,
}

impl<V: Clone> SpriteCache<V> {
    pub fn new(capacity: usize) -> SpriteCache<V> {
        let capacity = capacity.max(1);
        SpriteCache {
            kept: HashMap::with_capacity(capacity),
            capacity,
            clock: 0,
        }
    }

    /// `sprite`'s value -- `build` makes it if it isn't kept -- and, when the cache was full, the
    /// value of the sprite used longest ago, which this put out to make room.
    pub fn get(&mut self, sprite: Sprite, build: impl FnOnce(&Sprite) -> V) -> (V, Option<V>) {
        self.clock += 1;
        if let Some(kept) = self.kept.get_mut(&sprite) {
            kept.used = self.clock;
            return (kept.value.clone(), None);
        }
        let evicted = if self.kept.len() < self.capacity {
            None
        } else {
            let oldest = self
                .kept
                .iter()
                .min_by_key(|(_, kept)| kept.used)
                .map(|(sprite, _)| *sprite);
            oldest
                .and_then(|oldest| self.kept.remove(&oldest))
                .map(|kept| kept.value)
        };
        let value = build(&sprite);
        self.kept.insert(
            sprite,
            Kept {
                value: value.clone(),
                used: self.clock,
            },
        );
        (value, evicted)
    }

    pub fn len(&self) -> usize {
        self.kept.len()
    }

    pub fn is_empty(&self) -> bool {
        self.kept.is_empty()
    }
}

impl<V: Clone> Default for SpriteCache<V> {
    fn default() -> SpriteCache<V> {
        SpriteCache::new(CACHE_CAPACITY)
    }
}

/// Device pixels composed as GPUI's renderer composes the ornaments' parts: each quad and sprite
/// blended over what is there by its straight alpha, in the sRGB-encoded bytes, and kept as bytes
/// after each, as its `B8G8R8A8_UNORM` target keeps them. The tests check the ornaments on it, and
/// `examples/ornaments.rs` draws them with it.
pub struct Picture {
    width: i32,
    height: i32,
    pixels: Vec<[u8; 3]>,
}

impl Picture {
    pub fn new(width: i32, height: i32, background: u32) -> Picture {
        let [r, g, b] = bytes(background);
        Picture {
            width,
            height,
            pixels: vec![[r, g, b]; (width.max(0) * height.max(0)) as usize],
        }
    }

    pub fn width(&self) -> i32 {
        self.width
    }

    pub fn height(&self) -> i32 {
        self.height
    }

    /// A quad of `color` at `alpha` over `rect`.
    pub fn fill(&mut self, rect: DeviceRect, color: u32, alpha: u8) {
        let color = bytes(color);
        for y in rect.top.max(0)..rect.bottom.min(self.height) {
            for x in rect.left.max(0)..rect.right.min(self.width) {
                let i = (y * self.width + x) as usize;
                self.pixels[i] = blend(self.pixels[i], color, alpha);
            }
        }
    }

    pub fn line(&mut self, line: &Line) {
        self.fill(line.rect, line.color, line.alpha);
    }

    /// `placed`'s `pixels` ([`rasterize`]'s), where it shows.
    pub fn sprite(&mut self, placed: &Placed, pixels: &RgbaImage) {
        let shown = placed.shown;
        for y in shown.top.max(0)..shown.bottom.min(self.height) {
            for x in shown.left.max(0)..shown.right.min(self.width) {
                let Rgba([b, g, r, a]) =
                    *pixels.get_pixel((x - placed.rect.left) as u32, (y - placed.rect.top) as u32);
                let i = (y * self.width + x) as usize;
                self.pixels[i] = blend(self.pixels[i], [r, g, b], a);
            }
        }
    }

    /// The pixel at (`x`, `y`), RGB.
    pub fn pixel(&self, x: i32, y: i32) -> [u8; 3] {
        self.pixels[(y * self.width + x) as usize]
    }

    /// The picture as an RGBA image, for a PNG.
    pub fn to_image(&self) -> RgbaImage {
        RgbaImage::from_fn(side(self.width), side(self.height), |x, y| {
            let [r, g, b] = self.pixel(x as i32, y as i32);
            Rgba([r, g, b, u8::MAX])
        })
    }
}

/// `src` at `alpha` over `dst`, per channel, as the GPU blends and rounds it. The exact value is
/// a whole number of 255ths, never a half, so the rounding is the GPU's too.
fn blend(dst: [u8; 3], src: [u8; 3], alpha: u8) -> [u8; 3] {
    let alpha = u32::from(alpha);
    [0, 1, 2].map(|i| {
        ((u32::from(src[i]) * alpha + u32::from(dst[i]) * (255 - alpha) + 127) / 255) as u8
    })
}

/// The first of `size` pixels centred in `start..end`: half a pixel toward `start` when the two
/// can't share a centre.
fn centred(start: i32, end: i32, size: i32) -> i32 {
    start + (end - start - size).div_euclid(2)
}

/// The whole length nearest `ideal` of parity `parity` (1 odd, 0 even), halves to the larger: a
/// menu's 7 px diamond and a title bar's 8 px one stay apart at 125 and 150 %.
fn with_parity(ideal: f32, parity: i32) -> i32 {
    let pairs = ((ideal - parity as f32) / 2. + 0.5).floor() as i32;
    (2 * pairs + parity).max(2 - parity)
}

fn bytes(color: u32) -> [u8; 3] {
    [16, 8, 0].map(|shift| ((color >> shift) & 0xff) as u8)
}

fn channels(color: u32) -> [f64; 3] {
    bytes(color).map(f64::from)
}

/// `from` taken `amount` (0 to 1) of the way to `to`.
fn mix(from: u32, to: u32, amount: f64) -> [f64; 3] {
    let (from, to) = (channels(from), channels(to));
    [0, 1, 2].map(|i| from[i] + (to[i] - from[i]) * amount)
}

/// A pixel being composed: its straight colour, 0 to 255 a channel, and its alpha, 0 to 1.
#[derive(Clone, Copy)]
struct Paint {
    rgb: [f64; 3],
    alpha: f64,
}

impl Paint {
    const CLEAR: Paint = Paint {
        rgb: [0.; 3],
        alpha: 0.,
    };

    /// This with `rgb` at `alpha` laid over it: one straight colour that blends over anything as
    /// the two would in turn.
    fn layered(self, rgb: [f64; 3], alpha: f64) -> Paint {
        if alpha <= 0. {
            return self;
        }
        let total = alpha + self.alpha * (1. - alpha);
        let channel = |i: usize| (rgb[i] * alpha + self.rgb[i] * self.alpha * (1. - alpha)) / total;
        Paint {
            rgb: [channel(0), channel(1), channel(2)],
            alpha: total,
        }
    }

    fn bgra(self) -> Rgba<u8> {
        let alpha = (self.alpha * 255.).round() as u8;
        if alpha == 0 {
            return Rgba([0; 4]);
        }
        let [r, g, b] = self.rgb.map(|c| c.round().clamp(0., 255.) as u8);
        Rgba([b, g, r, alpha])
    }
}

/// How much of the pixel with its top left corner at (`x`, `y`) the diamond centred at `centre`,
/// `half` from its centre to each tip, covers: the exact area, 0 to 1.
fn coverage(x: f64, y: f64, centre: (f64, f64), half: f64) -> f64 {
    let dx = (x + 0.5 - centre.0).abs();
    let dy = (y + 0.5 - centre.1).abs();
    if dx + dy + 1. <= half {
        return 1.;
    }
    if (dx - 0.5).max(0.) + (dy - 0.5).max(0.) >= half {
        return 0.;
    }
    let mut polygon = Polygon::square(x, y);
    for (a, b) in [(1., 1.), (1., -1.), (-1., 1.), (-1., -1.)] {
        polygon = polygon.clip(a, b, half + a * centre.0 + b * centre.1);
    }
    polygon.area()
}

/// A convex polygon: a pixel's square, cut by a diamond's four sides.
#[derive(Clone, Copy)]
struct Polygon {
    points: [(f64, f64); 8],
    len: usize,
}

impl Polygon {
    fn square(x: f64, y: f64) -> Polygon {
        let mut polygon = Polygon {
            points: [(0., 0.); 8],
            len: 0,
        };
        for point in [(x, y), (x + 1., y), (x + 1., y + 1.), (x, y + 1.)] {
            polygon.push(point);
        }
        polygon
    }

    fn push(&mut self, point: (f64, f64)) {
        self.points[self.len] = point;
        self.len += 1;
    }

    /// The part where `a x + b y <= c`. A side's cut adds a corner at most, so the square's
    /// four grow to eight at most.
    fn clip(&self, a: f64, b: f64, c: f64) -> Polygon {
        let mut clipped = Polygon {
            points: [(0., 0.); 8],
            len: 0,
        };
        for i in 0..self.len {
            let (p, q) = (self.points[i], self.points[(i + 1) % self.len]);
            let (side_p, side_q) = (a * p.0 + b * p.1 - c, a * q.0 + b * q.1 - c);
            if side_p <= 0. {
                clipped.push(p);
            }
            if (side_p < 0. && side_q > 0.) || (side_p > 0. && side_q < 0.) {
                let t = side_p / (side_p - side_q);
                clipped.push((p.0 + t * (q.0 - p.0), p.1 + t * (q.1 - p.1)));
            }
        }
        clipped
    }

    fn area(&self) -> f64 {
        let twice: f64 = (0..self.len)
            .map(|i| {
                let (p, q) = (self.points[i], self.points[(i + 1) % self.len]);
                p.0 * q.1 - q.0 * p.1
            })
            .sum();
        twice.abs() / 2.
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use super::*;
    use crate::settings::{MAX_UI_SCALE, MIN_UI_SCALE};
    use crate::ui::theme::{BG_CARD, BG_ITEM_CARD, BG_MENU, BG_PANEL};

    /// The display scale factors Windows offers, and the UI scale across its range.
    const DEVICE_SCALES: [f32; 8] = [1., 1.25, 1.5, 1.75, 2., 2.25, 2.5, 3.];
    const UI_SCALES: [f32; 6] = [0.8, 0.9, 1., 1.15, 1.25, 1.5];

    fn scales() -> impl Iterator<Item = Scale> {
        DEVICE_SCALES.into_iter().flat_map(|device| {
            UI_SCALES
                .into_iter()
                .map(move |unit| Scale { device, unit })
        })
    }

    /// Where GPUI paints a rect handed to it as logical bounds: `Window::paint_quad` and
    /// `paint_image` round the origin and the origin plus the size to the device grid.
    fn painted(rect: DeviceRect, scale: Scale) -> DeviceRect {
        let (left, top) = (scale.logical(rect.left), scale.logical(rect.top));
        let width = scale.logical(rect.right) - left;
        let height = scale.logical(rect.bottom) - top;
        let snap = |logical: f32| round_half_toward_zero(logical * scale.device) as i32;
        DeviceRect::new(
            snap(left),
            snap(top),
            snap(left + width),
            snap(top + height),
        )
    }

    /// Framed surfaces as layout puts them on the device grid, px at 100 %: a one-line tooltip,
    /// a one-row menu, a menu, a window -- the last with long runs between its corners.
    fn frames(scale: Scale) -> [DeviceRect; 4] {
        [(40., 34.), (240., 38.), (243., 157.), (720., 540.)].map(|(width, height)| {
            let (x, y) = (scale.length(13.3), scale.length(7.7));
            DeviceRect::new(x, y, x + scale.length(width), y + scale.length(height))
        })
    }

    fn compose(rect: DeviceRect, metrics: FrameMetrics, background: u32) -> Picture {
        let mut picture = Picture::new(rect.right + 3, rect.bottom + 3, background);
        let parts = frame(rect, metrics);
        for line in &parts.lines {
            picture.line(line);
        }
        for corner in &parts.corners {
            picture.sprite(corner, &rasterize(&corner.sprite));
        }
        picture
    }

    fn alpha(image: &RgbaImage, x: i32, y: i32) -> u8 {
        image.get_pixel(x as u32, y as u32).0[3]
    }

    #[test]
    fn frame_parts_land_on_whole_device_pixels_inside_the_frame_and_never_overlap() {
        for scale in scales() {
            let metrics = FrameMetrics::new(scale);
            for rect in frames(scale) {
                let parts = frame(rect, metrics);
                let mut times = vec![0u8; (rect.width() * rect.height()) as usize];
                let mut paint = |part: DeviceRect| {
                    assert_eq!(painted(part, scale), part, "{scale:?}: GPUI moves {part:?}");
                    assert!(
                        part.left >= rect.left
                            && part.top >= rect.top
                            && part.right <= rect.right
                            && part.bottom <= rect.bottom,
                        "{scale:?}: {part:?} leaves the frame {rect:?}"
                    );
                    for y in part.top..part.bottom {
                        for x in part.left..part.right {
                            times[((y - rect.top) * rect.width() + x - rect.left) as usize] += 1;
                        }
                    }
                };
                for (index, line) in parts.lines.iter().enumerate() {
                    if line.rect.is_empty() {
                        continue;
                    }
                    let across = if matches!(index % 4, 0 | 1) {
                        line.rect.height()
                    } else {
                        line.rect.width()
                    };
                    assert_eq!(across, metrics.line, "{scale:?}: {line:?}");
                    paint(line.rect);
                }
                for corner in &parts.corners {
                    assert_eq!(painted(corner.rect, scale), corner.rect);
                    assert_eq!(corner.rect.width(), metrics.arm);
                    assert_eq!(corner.rect.height(), metrics.arm);
                    paint(corner.shown);
                }
                assert!(
                    times.iter().all(|&count| count <= 1),
                    "{scale:?}: {rect:?} has a pixel painted twice"
                );
            }
        }
    }

    #[test]
    fn corners_meet_their_lines_without_a_seam() {
        for scale in scales() {
            let metrics = FrameMetrics::new(scale);
            let FrameMetrics { line, gap, arm, .. } = metrics;
            let rect = frames(scale)[3];
            for background in [BG_MENU, BG_PANEL, BG_CARD, BG_ITEM_CARD] {
                let picture = compose(rect, metrics, background);
                let outer = bytes(BORDER_GOLD);
                let inner = blend(bytes(background), bytes(GOLD), FRAME_INNER_ALPHA);
                // Each row of both lines along each edge is one colour, from the last pixel of
                // one corner's sprite to the first of the next corner's.
                for (offset, colour) in (0..line)
                    .map(|offset| (offset, outer))
                    .chain((gap..gap + line).map(|offset| (offset, inner)))
                {
                    let along_x = rect.left + arm - 1..=rect.right - arm;
                    let along_y = rect.top + arm - 1..=rect.bottom - arm;
                    for y in [rect.top + offset, rect.bottom - 1 - offset] {
                        for x in along_x.clone() {
                            assert_eq!(picture.pixel(x, y), colour, "{scale:?} at ({x}, {y})");
                        }
                    }
                    for x in [rect.left + offset, rect.right - 1 - offset] {
                        for y in along_y.clone() {
                            assert_eq!(picture.pixel(x, y), colour, "{scale:?} at ({x}, {y})");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn small_frames_corners_meet_halfway_in_step() {
        for scale in scales() {
            let metrics = FrameMetrics::new(scale);
            let rect = frames(scale)[0];
            let picture = compose(rect, metrics, BG_MENU);
            // The fading gold of two corners mirrors about the frame's middle.
            for y in rect.top..rect.top + metrics.line {
                for x in rect.left..rect.right {
                    let mirrored = rect.right - 1 - (x - rect.left);
                    assert_eq!(picture.pixel(x, y), picture.pixel(mirrored, y), "{scale:?}");
                }
            }
        }
    }

    #[test]
    fn a_corner_diamond_sits_on_the_inner_lines_corner_and_touches_the_outer_line() {
        for scale in scales() {
            let metrics = FrameMetrics::new(scale);
            let FrameMetrics {
                line, gap, diamond, ..
            } = metrics;
            let image = rasterize(&Sprite::Corner {
                corner: Corner::TopLeft,
                metrics,
            });
            // Its axis is the inner line's: one tip pixel on an odd line, two on an even one.
            let tips: &[i32] = if line % 2 == 1 {
                &[gap + line / 2]
            } else {
                &[gap + line / 2 - 1, gap + line / 2]
            };
            let tip_alpha = if diamond % 2 == 1 { 191 } else { 128 };
            for &tip in tips {
                // The outer tips rest on the outer line: the line solid right over them, and
                // nothing but the diamond itself there -- no line runs behind it.
                assert_eq!(alpha(&image, tip, line - 1), u8::MAX, "{scale:?}");
                assert_eq!(alpha(&image, tip, line), tip_alpha, "{scale:?}");
                assert_eq!(alpha(&image, line, tip), tip_alpha, "{scale:?}");
                // The inner line runs into the inner tips.
                assert!(alpha(&image, 2 * gap - 1, tip) > tip_alpha, "{scale:?}");
            }
            // Between the diamond and the outer corner, whatever the frame lies over.
            assert_eq!(alpha(&image, line, line), 0, "{scale:?}");
            // Past the inner tips, the inner line alone, its quad's colour.
            let gold = bytes(GOLD);
            for across in gap..gap + line {
                for along in 2 * gap..metrics.arm {
                    let expected = Rgba([gold[2], gold[1], gold[0], FRAME_INNER_ALPHA]);
                    assert_eq!(*image.get_pixel(along as u32, across as u32), expected);
                    assert_eq!(*image.get_pixel(across as u32, along as u32), expected);
                }
            }
        }
    }

    #[test]
    fn corners_mirror_one_another_and_their_diagonal() {
        for scale in scales() {
            let metrics = FrameMetrics::new(scale);
            let pixels = |corner| rasterize(&Sprite::Corner { corner, metrics });
            let top_left = pixels(Corner::TopLeft);
            let others = [
                (pixels(Corner::TopRight), true, false),
                (pixels(Corner::BottomLeft), false, true),
                (pixels(Corner::BottomRight), true, true),
            ];
            let last = metrics.arm as u32 - 1;
            for y in 0..=last {
                for x in 0..=last {
                    let pixel = top_left.get_pixel(x, y);
                    assert_eq!(pixel, top_left.get_pixel(y, x), "{scale:?}");
                    for (other, mirror_x, mirror_y) in &others {
                        let ox = if *mirror_x { last - x } else { x };
                        let oy = if *mirror_y { last - y } else { y };
                        assert_eq!(pixel, other.get_pixel(ox, oy), "{scale:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn diamonds_are_symmetric_with_sharp_tips() {
        for size in 1..=40 {
            let image = rasterize(&Sprite::Diamond { size, color: GOLD });
            let last = size - 1;
            for y in 0..size {
                for x in 0..size {
                    let here = alpha(&image, x, y);
                    assert_eq!(here, alpha(&image, last - x, y), "{size} px");
                    assert_eq!(here, alpha(&image, x, last - y), "{size} px");
                    assert_eq!(here, alpha(&image, y, x), "{size} px");
                }
            }
            // Its tips on the sprite's edges: an odd one's single pixel three quarters covered,
            // its neighbours an eighth; an even one's two pixels half.
            let middle = size / 2;
            if size % 2 == 1 && size > 1 {
                assert_eq!(alpha(&image, middle, 0), 191, "{size} px");
                assert_eq!(alpha(&image, middle - 1, 0), 32, "{size} px");
            } else if size % 2 == 0 {
                assert_eq!(alpha(&image, middle - 1, 0), 128, "{size} px");
                assert_eq!(alpha(&image, middle, 0), 128, "{size} px");
            }
        }
    }

    #[test]
    fn free_diamonds_are_odd_and_a_lines_take_its_parity() {
        for scale in scales() {
            let rect = DeviceRect::new(3, 5, 3 + scale.length(8.), 5 + scale.length(8.));
            for size in [6., 7., 8., 10.] {
                let Sprite::Diamond { size: free, .. } =
                    diamond(rect, size, scale, GOLD, false).sprite
                else {
                    unreachable!()
                };
                assert_eq!(free % 2, 1, "{scale:?}");
                let Sprite::Diamond { size: on_line, .. } =
                    diamond(rect, size, scale, GOLD, true).sprite
                else {
                    unreachable!()
                };
                assert_eq!(on_line % 2, scale.hairline() % 2, "{scale:?}");
            }
        }
    }

    #[test]
    fn a_rule_and_a_heading_keep_their_diamond_on_their_lines_axis() {
        for scale in scales() {
            for (top, height) in [(11, scale.length(RULE_DIAMOND)), (4, 17), (4, 18)] {
                let rect = DeviceRect::new(5, top, 5 + scale.length(300.), top + height);
                let [left, middle, right] = rule(rect, scale, BORDER_GOLD, GOLD);
                let (left, middle, right) = (left.unwrap(), middle.unwrap(), right.unwrap());
                let axis = |part: Placed| part.rect.top + part.rect.bottom;
                assert_eq!(axis(left), axis(middle), "{scale:?}");
                assert_eq!(axis(right), axis(middle), "{scale:?}");
                for part in [left, middle, right] {
                    assert_eq!(painted(part.rect, scale), part.rect, "{scale:?}");
                }
                // A heading: its diamond in a box of its own, its line after the title, both as
                // tall as the heading's row.
                let row = |left, right| DeviceRect::new(left, rect.top, right, rect.bottom);
                let mark = diamond(row(5, 5 + scale.length(6.)), 6., scale, GOLD, true);
                let line = fading_line(row(80, 300), scale, BORDER_GOLD, false).unwrap();
                assert_eq!(axis(mark), axis(line), "{scale:?}");
            }
        }
    }

    #[test]
    fn a_fading_line_fades_evenly_end_to_end() {
        for length in [1, 2, 7, 150, 1333] {
            let sprite = |rising| Sprite::Fade {
                length,
                thickness: 2,
                color: BORDER_GOLD,
                rising,
            };
            let (rising, falling) = (rasterize(&sprite(true)), rasterize(&sprite(false)));
            for x in 0..length {
                let here = alpha(&rising, x, 0);
                assert_eq!(here, alpha(&rising, x, 1));
                assert_eq!(here, alpha(&falling, length - 1 - x, 0));
                let exact = (f64::from(x) + 0.5) / f64::from(length) * 255.;
                assert!((f64::from(here) - exact).abs() <= 0.5, "{length}: {x}");
            }
        }
    }

    #[test]
    fn the_keep_out_leaves_four_logical_px_of_background_past_the_inner_line() {
        let steps = ((MAX_UI_SCALE - MIN_UI_SCALE) / 0.05).round() as i32;
        for device in DEVICE_SCALES.into_iter().chain([2.75, 3.5]) {
            for step in 0..=steps {
                let scale = Scale {
                    device,
                    unit: MIN_UI_SCALE + 0.05 * step as f32,
                };
                let metrics = FrameMetrics::new(scale);
                let background = scale.length(FRAME_CLEAR) - metrics.gap - metrics.line;
                assert!(
                    background as f32 >= 4. * device,
                    "{scale:?}: {background} device px past the inner line"
                );
            }
        }
    }

    #[test]
    fn a_sprite_is_built_once_and_the_one_used_longest_ago_makes_room() {
        let built = Cell::new(0);
        let build = |sprite: &Sprite| {
            built.set(built.get() + 1);
            Rc::new(*sprite)
        };
        let corner = |device| Sprite::Corner {
            corner: Corner::TopLeft,
            metrics: FrameMetrics::new(Scale { device, unit: 1. }),
        };
        let mut cache = SpriteCache::new(3);
        let (first, pushed_out) = cache.get(corner(1.), &build);
        assert!(pushed_out.is_none());
        let (again, _) = cache.get(corner(1.), &build);
        assert!(Rc::ptr_eq(&first, &again));
        assert_eq!(built.get(), 1);
        // Another scale is another sprite, the first kept beside it.
        cache.get(corner(2.), &build);
        cache.get(corner(1.5), &build);
        assert_eq!(built.get(), 3);
        // Full: the next pushes out the one used longest ago -- 200 %, as 100 % was just used.
        cache.get(corner(1.), &build);
        let (_, pushed_out) = cache.get(corner(1.25), &build);
        assert_eq!(pushed_out.as_deref(), Some(&corner(2.)));
        assert_eq!(cache.len(), 3);
        assert_eq!(built.get(), 4);
    }
}
