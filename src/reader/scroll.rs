//! Scrolling math for the reading screen, as pure functions: visible pages, what to load, easing.

use std::ops::Range;
use std::time::Duration;

use crate::kitty::Geo;

/// Pages are at most this wide, like on the site.
const MAX_WIDTH_PX: u32 = 900;
/// Pages decoded on each side of the visible ones.
const WINDOW: usize = 2;
/// Pages downloaded ahead of the last visible one, and at the start of the next chapter once
/// the view gets that close to the end.
pub const AHEAD: usize = 5;
/// Decoded pixels (3 bytes each) kept in kitty. Kitty stores at most 320 MB of images and
/// silently drops the rest, so stay well under it; farther pages are freed first.
const KITTY_BUDGET: u64 = 200 * 1024 * 1024;
/// Time constant of the scroll ease-out, like CSS `scroll-behavior: smooth`: 95 % done after 3 of them.
const GLIDE: Duration = Duration::from_millis(80);

/// Reader sizes.
impl Geo {
    /// Width of the page column, in cells.
    pub fn page_cols(self) -> u16 {
        self.cols.min((MAX_WIDTH_PX / self.cell_w).max(1) as u16)
    }

    /// Width of the page column, in pixels: pages are decoded at this width and shown 1:1.
    pub fn page_px(self) -> u32 {
        u32::from(self.page_cols()) * self.cell_w
    }
}

/// Top of the view: a page and a pixel inside it, so pages loading above don't move what is read.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pos {
    pub page: usize,
    pub px: u32,
}

/// Moves the view by `delta` px, never above the first page nor below the end line (`end` px high).
pub fn scroll(heights: &[u32], view: u32, end: u32, pos: Pos, delta: i64) -> Pos {
    let above: u32 = heights[..pos.page.min(heights.len())].iter().sum();
    let max = (heights.iter().sum::<u32>() + end).saturating_sub(view);
    let mut offset = (i64::from(above + pos.px) + delta).clamp(0, i64::from(max)) as u32;
    let mut page = 0;
    while page + 1 < heights.len() && offset >= heights[page] {
        offset -= heights[page];
        page += 1;
    }
    Pos { page, px: offset }
}

/// The slices of pages the view shows from `pos`, top to bottom: (page, first px, height px).
pub fn visible(heights: &[u32], view: u32, pos: Pos) -> Vec<(usize, u32, u32)> {
    let mut slices = Vec::new();
    let (mut page, mut top, mut left) = (pos.page, pos.px, view);
    while left > 0 && page < heights.len() {
        let height = heights[page].saturating_sub(top).min(left);
        if height > 0 {
            slices.push((page, top, height));
        }
        left -= height;
        page += 1;
        top = 0;
    }
    slices
}

/// The pages to decode: the visible ones, `first` to `last`, and `WINDOW` on each side.
pub fn window(first: usize, last: usize, len: usize) -> Range<usize> {
    first.saturating_sub(WINDOW)..(last + WINDOW + 1).min(len)
}

/// The pages to have downloaded: those of the window, and `AHEAD` more after the last visible one.
pub fn to_download(first: usize, last: usize, len: usize) -> Range<usize> {
    window(first, last, len).start..(last + AHEAD + 1).min(len)
}

/// The last visible page is one of the last `AHEAD`: time to download the next chapter's start.
pub fn near_end(last: usize, len: usize) -> bool {
    last + AHEAD >= len
}

/// Which pages stored in kitty, as (page, bytes), to free: the farthest from `near` first, until
/// the rest fits in `KITTY_BUDGET`. Pages in `near` are always kept. Distance rather than
/// "least recently seen": a reader rarely jumps far back, so far pages are the least useful.
pub fn to_free(stored: &[(usize, u64)], near: &Range<usize>) -> Vec<usize> {
    // How many pages `page` is from `near`: 0 inside it.
    let distance = |page: usize| {
        if page < near.start {
            near.start - page
        } else if page >= near.end {
            page + 1 - near.end
        } else {
            0
        }
    };
    let mut stored = stored.to_vec();
    stored.sort_by_key(|&(page, _)| distance(page));
    let mut total = 0;
    stored
        .into_iter()
        .filter_map(|(page, bytes)| {
            total += bytes;
            (total > KITTY_BUDGET && !near.contains(&page)).then_some(page)
        })
        .collect()
}

/// Part of the pending scroll to do after `dt`: by elapsed time, so the glide is the same at any
/// frame rate. Never past `pending`.
pub fn ease(pending: i64, dt: Duration) -> i64 {
    let share = 1.0 - (-dt.as_secs_f64() / GLIDE.as_secs_f64()).exp();
    match (pending as f64 * share).round() as i64 {
        // Rounds to 0 when little is left: move 1 px instead, so the glide always ends.
        0 => pending.signum(),
        step => step,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEIGHTS: [u32; 3] = [100, 200, 50];
    const END: u32 = 10;

    fn at(page: usize, px: u32) -> Pos {
        Pos { page, px }
    }

    #[test]
    fn scrolling() {
        assert_eq!(scroll(&HEIGHTS, 80, END, at(0, 0), 30), at(0, 30));
        assert_eq!(scroll(&HEIGHTS, 80, END, at(0, 90), 30), at(1, 20));
        assert_eq!(scroll(&HEIGHTS, 80, END, at(1, 20), -30), at(0, 90));
        assert_eq!(scroll(&HEIGHTS, 80, END, at(0, 10), -50), at(0, 0));
        // 350 px of pages + the end line, in a view of 80: the bottom is 280 px down.
        assert_eq!(scroll(&HEIGHTS, 80, END, at(2, 0), 1000), at(1, 180));
        assert_eq!(scroll(&[30], 80, END, at(0, 0), 50), at(0, 0));
    }

    #[test]
    fn visible_slices() {
        assert_eq!(visible(&HEIGHTS, 80, at(0, 70)), [(0, 70, 30), (1, 0, 50)]);
        assert_eq!(
            visible(&HEIGHTS, 80, at(1, 180)),
            [(1, 180, 20), (2, 0, 50)]
        );
    }

    #[test]
    fn pages_around_the_view() {
        assert_eq!(window(0, 0, 10), 0..3);
        assert_eq!(window(5, 6, 10), 3..9);
        assert_eq!(window(8, 9, 10), 6..10);
        // Pages 3 and 4 on screen in a 20-page chapter: 1 to 9 (the window 1..=6, then 5 ahead).
        assert_eq!(to_download(3, 4, 20), 1..10);
        assert_eq!(to_download(18, 19, 20), 16..20);
        assert!(!near_end(14, 20));
        assert!(near_end(15, 20));
        // A chapter shorter than AHEAD: its end is near from the start.
        assert!(near_end(0, 3));
    }

    #[test]
    fn kitty_budget() {
        // Pages of 50 MB, 4 and 5 near the view: with 4 and 5, only the two closest others fit.
        let stored: Vec<_> = (0..10).map(|page| (page, 50 * 1024 * 1024)).collect();
        assert_eq!(to_free(&stored, &(4..6)), [2, 7, 1, 8, 0, 9]);
        // Small pages: everything fits, nothing is freed.
        let small: Vec<_> = (0..10).map(|page| (page, 1024)).collect();
        assert!(to_free(&small, &(4..6)).is_empty());
        // The view's own pages are never freed, even over the budget.
        let huge: Vec<_> = (0..3).map(|page| (page, 300 * 1024 * 1024)).collect();
        assert_eq!(to_free(&huge, &(0..2)), [2]);
    }

    #[test]
    fn easing() {
        // After one time constant: 1 - e^-1 of the way.
        assert_eq!(ease(100, GLIDE), 63);
        assert_eq!(ease(-100, GLIDE), -63);
        assert_eq!(ease(3, Duration::from_millis(1)), 1);
        assert_eq!(ease(-1, Duration::from_millis(1)), -1);
        assert_eq!(ease(100, Duration::from_secs(10)), 100);
    }

    #[test]
    fn page_width() {
        let geo = Geo {
            cols: 200,
            rows: 50,
            cell_w: 10,
            cell_h: 20,
        };
        assert_eq!(geo.page_cols(), 90);
        assert_eq!(geo.page_px(), 900);
    }
}
