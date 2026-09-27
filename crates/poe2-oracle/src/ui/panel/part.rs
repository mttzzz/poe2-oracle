//! The price panel in parts GPUI draws from their last frame while nothing in them changes
//! (`Entity::cached`): the title bar, the item's header and chips, each section's heading, each
//! filter row, the search row, what the search found and each listing row. Measured live on
//! 2026-09-27, a frame of the whole panel took 8-15 ms of the UI thread's CPU, and a check the
//! player moved the pointer over drew hundreds of them: every hover eases in and out over a few
//! frames, every tooltip fades in. Now such a frame draws afresh the part the hover or the tooltip
//! is in, and lays the others out from their last frame.
//!
//! How it holds together:
//! - `PriceCheckApp`'s own render lays out only the panel's skeleton -- its columns, the scroll
//!   area, the frame -- with a slot for each part in it ([`Placer`]). The skeleton holds nothing
//!   that eases, hints or loads: those tell the view they're drawn in, and a notice to
//!   `PriceCheckApp` is a change of the panel, which draws every part afresh.
//! - A part is a view of its own ([`Part`]) that draws its [`Piece`] from `PriceCheckApp`'s state
//!   with the render functions the whole panel used to call. It hears of every change of that state
//!   and draws itself afresh in the next frame; a hover, a tooltip or an icon in it tell it alone.
//! - GPUI lays a part drawn from its last frame out as a box of a height given up front, without
//!   measuring its content. A listing row and the title bar are of fixed height; any other part is
//!   laid out with the rest of the panel in a frame after a change, and its box then takes the
//!   height its content took (`ui::part_height`).
//! - While the tour is under way, every part is laid out and drawn with the rest each frame: the
//!   tour's spotlight finds its targets as they draw.

use std::collections::HashMap;
use std::rc::Rc;

use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, IntoElement, Length, Render,
    StyleRefinement, Subscription, WeakEntity, Window, canvas, div, prelude::*, px,
};

use crate::platform::check_clock;
use crate::price_check::PriceCheckApp;
use crate::session::SessionStatus;
use crate::ui::part_height::PartHeight;
use crate::ui::style::TITLE_BAR_HEIGHT;
use crate::ui::theme::rems_from_px;
use crate::ui::tour;

use super::filters::{self, Section};
use super::nameplate::{render_chips, render_nameplate};
use super::results::{self, LISTING_ROW_HEIGHT};
use super::title_bar::render_title_bar;
use super::waystone::render_waystone_marks;

/// What a part of the panel draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Piece {
    /// The strip at the top: the league select, the rate, ⚙ and ×.
    TitleBar,
    /// The item's name and art, and the links under them.
    Nameplate,
    /// The chips under the name.
    Chips,
    /// The profile select and «Минимум тира», for an item that has either.
    Toolbar,
    /// A section of the filter rows: its heading.
    Heading(Section),
    /// A filter row, by its index.
    Filter(usize),
    /// A section's unchecked properties, folded into chips under its rows.
    PropertyChips(Section),
    /// The toggle unfolding the rows kept out of sight.
    HiddenToggle,
    /// A waystone's marked modifiers.
    WaystoneMarks,
    /// The «Поиск» plate and the selects beside it.
    SearchRow,
    /// What the search found, down to the listings table's header.
    Outcome,
    /// A listing row, by its index.
    Listing(usize),
}

impl Piece {
    /// The height, in the panel's px, of a part that always stands as tall; `None` for one whose
    /// content decides.
    fn fixed_height(self) -> Option<f32> {
        match self {
            Piece::TitleBar => Some(TITLE_BAR_HEIGHT),
            Piece::Listing(_) => Some(LISTING_ROW_HEIGHT),
            _ => None,
        }
    }

    /// The piece drawn from `state`: nothing where `state` has no such piece now.
    fn draw(
        self,
        state: &PriceCheckApp,
        window: &Window,
        cx: &Context<PriceCheckApp>,
    ) -> AnyElement {
        let item = state.item.as_ref();
        let drawn = match (self, item) {
            (Piece::TitleBar, _) => Some(render_title_bar(state, window, cx).into_any_element()),
            (Piece::Nameplate, Some(item)) => {
                Some(render_nameplate(item, state.trade_site(), cx).into_any_element())
            }
            (Piece::Chips, Some(item)) => Some(render_chips(state, item, cx).into_any_element()),
            (Piece::Toolbar, Some(_)) => {
                results::render_toolbar(state, window, cx).map(IntoElement::into_any_element)
            }
            (Piece::Heading(section), Some(item)) => {
                Some(filters::render_heading(state, item, section).into_any_element())
            }
            (Piece::Filter(row), Some(item)) => {
                filters::render_filter(state, item, row, window, cx)
            }
            (Piece::PropertyChips(section), Some(_)) => {
                Some(filters::render_property_chips(state, section, cx).into_any_element())
            }
            (Piece::HiddenToggle, Some(_)) => {
                Some(filters::render_hidden_toggle(state, cx).into_any_element())
            }
            (Piece::WaystoneMarks, Some(item)) => {
                render_waystone_marks(state, item).map(IntoElement::into_any_element)
            }
            (Piece::SearchRow, Some(_)) => {
                Some(results::render_search_row(state, cx).into_any_element())
            }
            (Piece::Outcome, Some(item)) => Some(results::render_outcome(state, item, cx)),
            (Piece::Listing(index), Some(_)) => results::render_listing(state, index, cx),
            (_, None) => None,
        };
        drawn.unwrap_or_else(|| div().into_any_element())
    }
}

/// A part of the panel: a view drawing its [`Piece`], which GPUI draws from its last frame while
/// nothing in it changes.
pub(super) struct Part {
    app: WeakEntity<PriceCheckApp>,
    piece: Piece,
    /// How tall it stands, for a piece of no fixed height.
    height: Rc<PartHeight>,
    /// `PriceCheckApp` changed since the part was last drawn, which may have made it taller or
    /// shorter.
    changed: bool,
    _app_changed: Subscription,
    /// A listing row's trade-site button is the signed-in player's (`results::render_listing`).
    _session_changed: Option<Subscription>,
}

impl Part {
    fn new(app: &Entity<PriceCheckApp>, piece: Piece, cx: &mut Context<Part>) -> Part {
        Part {
            app: app.downgrade(),
            piece,
            height: Rc::default(),
            changed: true,
            _app_changed: cx.observe(app, |part, _, cx| {
                part.changed = true;
                cx.notify();
            }),
            _session_changed: matches!(piece, Piece::Listing(_))
                .then(|| cx.observe_global::<SessionStatus>(|_, cx| cx.notify())),
        }
    }
}

impl Render for Part {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        check_clock::part_drawn();
        self.changed = false;
        let piece = self.piece;
        let content = match self.app.upgrade() {
            Some(app) => app.update(cx, |state, cx| piece.draw(state, window, cx)),
            // Gone with the panel's window.
            None => div().into_any_element(),
        };
        let part = div().flex().flex_col().flex_none().w_full().child(content);
        if piece.fixed_height().is_some() {
            return part;
        }
        // The height the content took, for the part's box in the frames drawing it from this one.
        let height = self.height.clone();
        part.child(
            canvas(
                move |bounds, window, _| {
                    if height.took(f32::from(bounds.size.height)) {
                        window.request_animation_frame();
                    }
                },
                |_, (), _, _| {},
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        )
    }
}

/// The panel's parts, made as a frame first places them and dropped once a frame no longer does.
#[derive(Default)]
pub(crate) struct Parts(HashMap<Piece, Entity<Part>>);

/// Places the panel's parts in a frame of `PriceCheckApp`'s render.
pub(super) struct Placer<'a> {
    parts: &'a mut Parts,
    app: Entity<PriceCheckApp>,
    /// Off while the tour is under way (see the module doc).
    cached: bool,
    placed: Vec<Piece>,
}

impl<'a> Placer<'a> {
    pub(super) fn new(parts: &'a mut Parts, app: Entity<PriceCheckApp>, cx: &App) -> Placer<'a> {
        Placer {
            parts,
            app,
            cached: !tour::under_way(cx),
            placed: Vec::new(),
        }
    }

    /// `piece`'s slot: its part drawn from its last frame in a box of its height where it can be,
    /// laid out and drawn with the rest of the panel otherwise.
    pub(super) fn place(&mut self, piece: Piece, cx: &mut App) -> AnyElement {
        self.placed.push(piece);
        let app = &self.app;
        let part = self
            .parts
            .0
            .entry(piece)
            .or_insert_with(|| cx.new(|cx| Part::new(app, piece, cx)))
            .clone();
        // Read every part, every frame. A part made in this frame is already marked accessed
        // (gpui's `EntityMap::insert`), so the cached view's first render leaves its own id out
        // of the entities it replays when reused; without this read, a frame reusing it would
        // drop it from the window's tracked set, and its notifies (hover, a copied note, an icon
        // load, the session) would no longer redraw it.
        let state = part.read(cx);
        let boxed: Option<Length> = match piece.fixed_height() {
            Some(height) => self.cached.then(|| rems_from_px(height).into()),
            None => state
                .height
                .box_for_frame(self.cached, state.changed)
                .map(|height| px(height).into()),
        };
        match boxed {
            Some(height) => part
                .cached(StyleRefinement::default().w_full().flex_none().h(height))
                .into_any_element(),
            None => part.into_any_element(),
        }
    }

    /// The frame placed all its parts: those it didn't are dropped.
    pub(super) fn finish(self) {
        check_clock::parts_placed(self.placed.len());
        let placed = self.placed;
        self.parts.0.retain(|piece, _| placed.contains(piece));
    }
}
