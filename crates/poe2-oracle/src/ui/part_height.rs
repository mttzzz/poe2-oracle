//! How tall a part of the price panel stands while GPUI draws it from its last frame
//! (`ui::panel::part`). A part drawn that way is laid out as a box of a height given up front --
//! GPUI doesn't measure what's in it -- so the box takes the height the part's content took when
//! it was last laid out with the rest of the panel, which it keeps while its content doesn't
//! change. Kept apart from GPUI, so it builds and is tested on every target.
//!
//! Heights are logical pixels, compared exactly: GPUI lays every length of the panel out in whole
//! device pixels (`gpui::taffy`), so a part's content takes the same height to the bit whether it
//! was laid out with the rest or in its box, and a box of that height leaves everything below it
//! where it was.

use std::cell::Cell;

/// One part's height: the one its content took when last laid out, and the box it was given this
/// frame.
#[derive(Debug, Default)]
pub struct PartHeight {
    /// The height its content took when last laid out; `None` before its first layout.
    took: Cell<Option<f32>>,
    /// The height of the box it was given this frame; `None` when it's laid out with the rest.
    boxed: Cell<Option<f32>>,
}

impl PartHeight {
    /// The height of the box the part is drawn in this frame: the one its content last took,
    /// while GPUI may draw it from its last frame (`cached`) and nothing it shows changed since
    /// (`!changed`). `None` has it laid out with the rest of the panel, which measures it anew: its
    /// first frame, and any after a change, which may have made it taller or shorter.
    pub fn box_for_frame(&self, cached: bool, changed: bool) -> Option<f32> {
        let boxed = self.took.get().filter(|_| cached && !changed);
        self.boxed.set(boxed);
        boxed
    }

    /// The part's content took `height` as it was laid out this frame. `true` when it was drawn
    /// in a box of another height -- it changed without saying so -- which the next frame must
    /// lay out again at the height it takes now: till then the panel below it is where it was.
    pub fn took(&self, height: f32) -> bool {
        self.took.set(Some(height));
        self.boxed.get().is_some_and(|boxed| boxed != height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_part_is_laid_out_with_the_panel_until_it_has_a_height() {
        let height = PartHeight::default();
        assert_eq!(height.box_for_frame(true, false), None);
        assert!(!height.took(57.0));
        assert_eq!(height.box_for_frame(true, false), Some(57.0));
    }

    #[test]
    fn a_change_has_the_part_measured_again_with_the_panel() {
        let height = PartHeight::default();
        height.took(57.0);
        assert_eq!(height.box_for_frame(true, true), None);
        // Laid out with the rest, a new height is simply what it takes now.
        assert!(!height.took(71.0));
        assert_eq!(height.box_for_frame(true, false), Some(71.0));
    }

    #[test]
    fn with_caching_off_every_frame_lays_the_part_out_with_the_panel() {
        let height = PartHeight::default();
        height.took(57.0);
        assert_eq!(height.box_for_frame(false, false), None);
        assert!(!height.took(64.0));
    }

    #[test]
    fn content_outgrowing_its_box_asks_for_a_frame_at_its_new_height() {
        let height = PartHeight::default();
        height.took(57.0);
        assert_eq!(height.box_for_frame(true, false), Some(57.0));
        assert!(height.took(60.0));
        assert_eq!(height.box_for_frame(true, false), Some(60.0));
        assert!(!height.took(60.0));
    }
}
