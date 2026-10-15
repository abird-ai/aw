//! Local, side-effect-free viewport intent for native scrolling.
//! External/background work must never be triggered from these methods.

pub const WHEEL_ROWS: usize = 6;
pub const SCROLLOFF_ROWS: usize = 3;
pub const MAX_TAIL_OVERSCROLL: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Viewport {
    offset: usize,
    follow_end: bool,
    total: usize,
    height: usize,
    tail_overscroll: usize,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            offset: 0,
            follow_end: false,
            total: 0,
            height: 1,
            tail_overscroll: 0,
        }
    }
}

impl Viewport {
    pub fn following_end() -> Self {
        Self {
            follow_end: true,
            ..Self::default()
        }
    }

    pub fn configure(&mut self, total: usize, height: usize, allow_overscroll: bool) {
        self.total = total;
        self.height = height.max(1);
        self.tail_overscroll = if allow_overscroll && total > self.height {
            (self.height / 3).clamp(1, MAX_TAIL_OVERSCROLL)
        } else {
            0
        };
        if self.follow_end {
            self.offset = self.normal_max();
        } else {
            self.offset = self.offset.min(self.max_offset());
        }
    }

    pub fn offset(&self) -> usize {
        if self.follow_end {
            self.normal_max()
        } else {
            self.offset.min(self.max_offset())
        }
    }

    pub fn source_scrollbar_offset(&self) -> usize {
        self.offset().min(self.normal_max())
    }

    pub fn scroll(&mut self, delta: isize) {
        let at = self.offset();
        self.follow_end = false;
        self.offset = if delta < 0 {
            at.saturating_sub(delta.unsigned_abs())
        } else {
            at.saturating_add(delta as usize).min(self.max_offset())
        };
    }

    pub fn page(&mut self, down: bool) {
        let step = self.height.saturating_sub(2).max(1);
        self.scroll(if down { step as isize } else { -(step as isize) });
    }

    pub fn home(&mut self) {
        self.follow_end = false;
        self.offset = 0;
    }

    pub fn end(&mut self) {
        self.follow_end = true;
        self.offset = self.normal_max();
    }

    pub fn jump_to(&mut self, row: usize, scrolloff: usize) {
        self.follow_end = false;
        let row = row.min(self.total.saturating_sub(1));
        let margin = scrolloff.min(self.height.saturating_sub(1) / 2);
        let at = self.offset();
        let bottom_guard = at
            .saturating_add(self.height)
            .saturating_sub(margin.saturating_add(1));
        if row < at.saturating_add(margin) {
            self.offset = row.saturating_sub(margin);
        } else if row > bottom_guard {
            self.offset = row
                .saturating_add(margin)
                .saturating_add(1)
                .saturating_sub(self.height);
        }
        self.offset = self.offset.min(self.max_offset());
    }

    #[cfg(test)]
    pub fn is_following_end(&self) -> bool {
        self.follow_end
    }

    #[cfg(test)]
    pub fn tail_overscroll(&self) -> usize {
        self.tail_overscroll
    }

    fn normal_max(&self) -> usize {
        self.total.saturating_sub(self.height)
    }

    fn max_offset(&self) -> usize {
        self.normal_max().saturating_add(self.tail_overscroll)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_view_gets_bounded_tail_overscroll_but_normal_lists_do_not() {
        let mut file = Viewport::default();
        file.configure(100, 20, true);
        assert_eq!(file.tail_overscroll(), 6);
        file.end();
        file.scroll(6);
        assert_eq!(file.offset(), 86);

        let mut list = Viewport::default();
        list.configure(100, 20, false);
        list.end();
        list.scroll(6);
        assert_eq!(list.offset(), 80);
    }

    #[test]
    fn jump_uses_scrolloff_when_room_exists() {
        let mut view = Viewport::default();
        view.configure(100, 20, true);
        view.jump_to(50, SCROLLOFF_ROWS);
        assert!(view.offset() <= 47);
        assert!(50 < view.offset() + 20 - SCROLLOFF_ROWS);
        view.jump_to(0, SCROLLOFF_ROWS);
        assert_eq!(view.offset(), 0);
    }

    #[test]
    fn follow_end_tracks_resize_without_usize_sentinel() {
        let mut view = Viewport::following_end();
        assert!(view.is_following_end());
        view.configure(100, 20, false);
        assert_eq!(view.offset(), 80);
        view.configure(100, 10, false);
        assert_eq!(view.offset(), 90);
    }
}
