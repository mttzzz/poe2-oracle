//! The geometry of a vertical scrollbar: whether a scroll area gets a bar, how long its thumb is,
//! where the thumb stands for the area's scroll position, and the scroll position a drag of the
//! thumb or a press on the track gives. Kept apart from GPUI, so it builds and is tested on every
//! target; `ui::scrollbar` lays the bar out and draws it from these.
//!
//! Lengths are logical pixels along the scrolled axis. A *scroll position* is how far the content
//! has moved up: 0 at its top, the content's length past the viewport's at its bottom -- the
//! opposite sign of GPUI's scroll offset. The *track* is the strip the thumb slides on, which may
//! be shorter than the area it stands beside; a position along it is measured from its top.

/// A scroll area's bar: how far the content scrolls, and the track and thumb that show it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bar {
    /// How far the content scrolls: its length past the viewport's.
    max_scroll: f32,
    /// The length of the track, and of the thumb on it.
    track: f32,
    thumb: f32,
}

impl Bar {
    /// The bar for a viewport `viewport` long onto `content`, drawn on a track `track` long, its
    /// thumb the viewport's share of the track but never shorter than `min_thumb` -- so a long page
    /// leaves one to take hold of. `None` where there is nothing to show: the content fits in the
    /// viewport, exactly fitting included; the area is not laid out yet (a length of 0, or none at
    /// all); or the track has no room for a thumb that could still slide.
    pub fn new(viewport: f32, content: f32, track: f32, min_thumb: f32) -> Option<Bar> {
        let laid_out = viewport > 0. && track > 0. && content.is_finite();
        let max_scroll = content - viewport;
        if !laid_out || max_scroll <= 0. {
            return None;
        }
        let thumb = (track * viewport / content).max(min_thumb).min(track);
        (thumb < track).then_some(Bar {
            max_scroll,
            track,
            thumb,
        })
    }

    /// The thumb's length.
    pub fn thumb(&self) -> f32 {
        self.thumb
    }

    /// How far the thumb slides along the track, from the top of the content to its bottom.
    fn travel(&self) -> f32 {
        self.track - self.thumb
    }

    /// Where the thumb's top stands on the track for the content scrolled to `scroll`: on the
    /// track's top at the content's top, with the thumb's bottom on the track's bottom at the
    /// content's bottom. A `scroll` past either end is taken as that end, as GPUI clamps its own
    /// offset only when it next lays the area out.
    pub fn thumb_top(&self, scroll: f32) -> f32 {
        (scroll / self.max_scroll).clamp(0., 1.) * self.travel()
    }

    /// Whether `at`, a position along the track, is on the thumb of the content scrolled to
    /// `scroll`. The thumb has its top edge and not its bottom one, so two thumbs end to end hold
    /// every position once.
    pub fn on_thumb(&self, scroll: f32, at: f32) -> bool {
        let top = self.thumb_top(scroll);
        top <= at && at < top + self.thumb
    }

    /// The scroll position that stands the thumb's top at `top` on the track, the end of the
    /// content when `top` is past either end of the thumb's way.
    fn scroll_at(&self, top: f32) -> f32 {
        (top / self.travel()).clamp(0., 1.) * self.max_scroll
    }

    /// The scroll position of a drag: the pointer took the thumb with the content scrolled to
    /// `grabbed_at` and has moved `moved` since, along the track and downward when positive. The
    /// thumb goes with it, one for one, and stops at the ends of the track -- going back from past
    /// one, it takes hold again where the pointer meets it.
    pub fn dragged(&self, grabbed_at: f32, moved: f32) -> f32 {
        self.scroll_at(self.thumb_top(grabbed_at) + moved)
    }

    /// The scroll position of a press at `at` on the track, off the thumb: the thumb jumps to
    /// stand centred on the press, or as near as the track's ends let it, and the press goes on as
    /// a drag of it.
    pub fn pressed(&self, at: f32) -> f32 {
        self.scroll_at(at - self.thumb / 2.)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A bar on a track as long as its viewport, the thumb at least 20 px.
    fn bar(viewport: f32, content: f32) -> Bar {
        Bar::new(viewport, content, viewport, 20.).expect("content taller than the viewport")
    }

    /// Floats that took different routes to the same length differ in their last bits: equal to
    /// within a hundred-thousandth of it.
    fn assert_close(left: f32, right: f32) {
        assert!(
            (left - right).abs() <= 1e-5 * (1. + right.abs()),
            "{left} is not {right}"
        );
    }

    #[test]
    fn a_bar_shows_for_content_taller_than_the_viewport_and_only_then() {
        assert!(Bar::new(400., 401., 400., 20.).is_some());
        assert_eq!(Bar::new(400., 400., 400., 20.), None, "exactly fitting");
        assert_eq!(
            Bar::new(400., 399.5, 400., 20.),
            None,
            "fitting with room over"
        );
        assert_eq!(
            Bar::new(400., 120., 400., 20.),
            None,
            "far less than the viewport"
        );
    }

    #[test]
    fn an_area_not_laid_out_yet_has_no_bar() {
        // GPUI's scroll handle reports a viewport of 0 and no overflow before its first layout.
        assert_eq!(Bar::new(0., 0., 400., 20.), None);
        assert_eq!(
            Bar::new(0., 900., 400., 20.),
            None,
            "content but no viewport"
        );
        assert_eq!(Bar::new(400., 900., 0., 20.), None, "no track");
        assert_eq!(Bar::new(f32::NAN, 900., 400., 20.), None);
        assert_eq!(Bar::new(400., f32::NAN, 400., 20.), None);
        assert_eq!(Bar::new(400., f32::INFINITY, 400., 20.), None);
    }

    #[test]
    fn a_track_with_no_room_for_a_sliding_thumb_has_no_bar() {
        // A thumb as long as the track can't move, so it would only mislead.
        assert_eq!(
            Bar::new(400., 900., 30., 40.),
            None,
            "the least thumb fills the track"
        );
        assert_eq!(
            Bar::new(400., 900., 30., 30.),
            None,
            "the least thumb is the track"
        );
        assert!(Bar::new(400., 900., 30., 20.).is_some());
    }

    #[test]
    fn the_thumb_is_the_viewports_share_of_the_track() {
        // A quarter of the content is in view; the track, shorter than the viewport, is 192 long.
        let bar = Bar::new(200., 800., 192., 20.).unwrap();
        assert_eq!(bar.thumb(), 48.);
        assert_eq!(bar.travel(), 144.);
        assert_eq!(bar.max_scroll, 600.);
    }

    #[test]
    fn the_thumb_stands_at_the_ends_of_the_track_at_the_ends_of_the_scroll() {
        let bar = bar(200., 800.);
        assert_eq!(bar.thumb_top(0.), 0.);
        // The very bottom: the thumb's end on the track's end, and nothing short of it.
        assert_eq!(bar.thumb_top(bar.max_scroll), bar.travel());
        assert_close(bar.thumb_top(bar.max_scroll) + bar.thumb(), 200.);
        assert!(bar.thumb_top(bar.max_scroll - 1.) < bar.travel());
        assert!(bar.thumb_top(1.) > 0.);
    }

    #[test]
    fn the_thumb_moves_in_proportion_to_the_scroll() {
        let bar = bar(200., 800.);
        assert_eq!(bar.thumb_top(300.), bar.travel() / 2.);
        assert_eq!(bar.thumb_top(150.), bar.travel() / 4.);
    }

    #[test]
    fn a_long_page_keeps_a_thumb_to_take_hold_of_and_the_ends_still_reach() {
        // A million px of content would make a 0.04 px thumb; the 20 px least is kept, and the
        // way the thumb slides is what is left of the track, not what the share would have left.
        let bar = bar(200., 1_000_000.);
        assert_eq!(bar.thumb(), 20.);
        assert_eq!(bar.thumb_top(0.), 0.);
        assert_close(bar.thumb_top(bar.max_scroll) + bar.thumb(), 200.);
    }

    #[test]
    fn a_scroll_past_either_end_leaves_the_thumb_on_the_track() {
        let bar = bar(200., 800.);
        assert_eq!(bar.thumb_top(-40.), 0.);
        assert_eq!(bar.thumb_top(bar.max_scroll + 40.), bar.travel());
        assert!(!bar.on_thumb(-40., bar.travel() + 1.));
        assert!(bar.on_thumb(bar.max_scroll + 40., bar.travel() + 1.));
    }

    #[test]
    fn the_thumb_is_where_its_top_edge_is_and_its_bottom_edge_is_not() {
        let bar = bar(200., 800.);
        // Scrolled 300: the 50 px thumb stands from 75 to 125.
        assert!(!bar.on_thumb(300., 74.9));
        assert!(bar.on_thumb(300., 75.));
        assert!(bar.on_thumb(300., 124.9));
        assert!(!bar.on_thumb(300., 125.));
    }

    #[test]
    fn a_drag_that_has_not_moved_keeps_the_scroll() {
        for bar in [bar(200., 800.), bar(200., 1_000_000.), bar(333.3, 1000.)] {
            for part in [0., 0.1, 0.5, 0.9, 1.] {
                let scroll = bar.max_scroll * part;
                assert_close(bar.dragged(scroll, 0.), scroll);
            }
        }
    }

    #[test]
    fn a_drag_scrolls_the_content_by_the_thumbs_way_over_the_tracks() {
        let bar = bar(200., 800.);
        // 600 px of scroll over a thumb that slides 150: 4 px of content to each px of thumb.
        assert_close(bar.dragged(0., 10.), 40.);
        assert_close(bar.dragged(100., 10.), 140.);
        assert_close(bar.dragged(300., -25.), 200.);
    }

    #[test]
    fn a_drag_to_where_the_thumb_stood_gives_back_that_scroll() {
        // Taking the thumb at the top and drawing it to where it stands for `scroll` -- or back
        // from there -- ends on `scroll`, or the top.
        let bar = bar(333.3, 1000.);
        for part in [0.05, 0.3, 0.5, 0.77, 1.] {
            let scroll = bar.max_scroll * part;
            let way = bar.thumb_top(scroll);
            assert_close(bar.dragged(0., way), scroll);
            assert_close(bar.dragged(scroll, -way), 0.);
        }
    }

    #[test]
    fn a_drag_stops_at_both_ends() {
        let bar = bar(200., 800.);
        assert_eq!(bar.dragged(300., 1_000_000.), bar.max_scroll);
        assert_eq!(bar.dragged(300., -1_000_000.), 0.);
        assert_eq!(bar.dragged(0., -5.), 0., "up from the top");
        assert_eq!(
            bar.dragged(bar.max_scroll, 5.),
            bar.max_scroll,
            "down from the bottom"
        );
        // The pointer is measured from where it took the thumb, not from where the thumb
        // stopped: back from past the end, it takes hold again where it meets it.
        assert_close(bar.dragged(300., 20.), 380.);
    }

    #[test]
    fn a_press_on_the_track_centres_the_thumb_on_it() {
        let bar = bar(200., 800.);
        for at in [30., 100., 160.] {
            let scroll = bar.pressed(at);
            assert_close(bar.thumb_top(scroll) + bar.thumb() / 2., at);
        }
    }

    #[test]
    fn a_press_near_an_end_of_the_track_goes_to_that_end() {
        let bar = bar(200., 800.);
        // The centred thumb would hang over the end: it stops at it.
        assert_eq!(bar.pressed(0.), 0.);
        assert_eq!(bar.pressed(10.), 0.);
        assert_eq!(bar.pressed(200.), bar.max_scroll);
        assert_eq!(bar.pressed(190.), bar.max_scroll);
        assert_eq!(bar.pressed(-30.), 0., "above the track");
        assert_eq!(bar.pressed(230.), bar.max_scroll, "below it");
    }

    #[test]
    fn a_press_at_the_thumbs_centre_keeps_the_scroll() {
        let bar = bar(200., 800.);
        let scroll = 300.;
        let centre = bar.thumb_top(scroll) + bar.thumb() / 2.;
        assert_close(bar.pressed(centre), scroll);
        // And a drag from there goes on from where the press put the thumb.
        assert_close(bar.dragged(bar.pressed(centre), 10.), scroll + 40.);
    }
}
