use aw_workspace::layout::Id;
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::Span,
    widgets::Paragraph,
    Frame,
};
use unicode_width::UnicodeWidthStr;

pub(super) fn draw(
    frame: &mut Frame<'_>,
    area: Rect,
    tabs: &[(Id, String, Color)],
    active: Option<Id>,
    palette: crate::tui::theme::Palette,
) -> (Vec<(Rect, Id)>, u16) {
    if area.width == 0 || area.height == 0 || tabs.is_empty() {
        return (Vec::new(), 0);
    }

    // Anchor the viewport to the selected tab, including after keyboard cycling.
    // Overflow arrows select the adjacent hidden tab through the ordinary tab
    // hit target, so paint and input need no second tab order or scroll state.
    const GAP: usize = 1;
    let available = usize::from(area.width);
    let widths = tabs
        .iter()
        .map(|(_, label, _)| UnicodeWidthStr::width(label.as_str()).saturating_add(2))
        .collect::<Vec<_>>();
    let selected = active
        .and_then(|id| tabs.iter().position(|(tab, _, _)| *tab == id))
        .unwrap_or(0);
    let mut start = selected;
    let mut end = selected + 1;
    let mut natural = widths[selected];
    // Below three cells, reserve all space for the active label. Keyboard
    // cycling remains available when two arrows and content cannot coexist.
    let show_overflow = available >= 3;
    let needed = |start: usize, end: usize, natural: usize| {
        natural
            .saturating_add(GAP.saturating_mul(end - start - 1))
            .saturating_add(usize::from(show_overflow && start > 0))
            .saturating_add(usize::from(show_overflow && end < tabs.len()))
    };
    loop {
        let mut grew = false;
        if start > 0 {
            let next = natural.saturating_add(widths[start - 1]);
            if needed(start - 1, end, next) <= available {
                start -= 1;
                natural = next;
                grew = true;
            }
        }
        if end < tabs.len() {
            let next = natural.saturating_add(widths[end]);
            if needed(start, end + 1, next) <= available {
                end += 1;
                natural = next;
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }

    let left = show_overflow && start > 0;
    let right = show_overflow && end < tabs.len();
    let quiet = Style::default().fg(palette.muted);
    let mut hits = Vec::new();
    let mut x = area.x;
    if left {
        let rect = Rect::new(x, area.y, 1, 1);
        frame.render_widget(Paragraph::new(Span::styled("‹", quiet)), rect);
        hits.push((rect, tabs[start - 1].0));
        x = x.saturating_add(1);
    }
    let content_right = area.right().saturating_sub(u16::from(right));
    for index in start..end {
        let (tab, label, _) = &tabs[index];
        let width = widths[index].min(usize::from(content_right.saturating_sub(x))) as u16;
        if width == 0 {
            break;
        }
        let rect = Rect::new(x, area.y, width, 1);
        let padding = usize::from(width >= 3);
        let shown = truncate_cells(label, usize::from(width).saturating_sub(padding * 2));
        let text = if padding == 0 {
            shown
        } else {
            format!(" {shown} ")
        };
        let style = if active == Some(*tab) {
            palette.tab_active_style()
        } else {
            quiet
        };
        frame.render_widget(Paragraph::new(Span::styled(text, style)).style(style), rect);
        hits.push((rect, *tab));
        x = x.saturating_add(width);
        if index + 1 < end {
            x = x.saturating_add(GAP as u16);
        }
    }
    if right {
        let rect = Rect::new(content_right, area.y, 1, 1);
        frame.render_widget(Paragraph::new(Span::styled("›", quiet)), rect);
        hits.push((rect, tabs[end].0));
    }
    (hits, 1)
}

fn truncate_cells(text: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0usize;
    for grapheme in unicode_segmentation::UnicodeSegmentation::graphemes(text, true) {
        let width = UnicodeWidthStr::width(grapheme);
        if used.saturating_add(width) > max {
            break;
        }
        out.push_str(grapheme);
        used = used.saturating_add(width);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, buffer::Buffer, Terminal};

    fn tabs() -> Vec<(Id, String, Color)> {
        (1..=7)
            .map(|id| (id, format!("Tab {id}"), Color::Green))
            .collect()
    }

    fn paint(
        width: u16,
        tabs: &[(Id, String, Color)],
        active: Option<Id>,
    ) -> (Buffer, Vec<(Rect, Id)>, u16) {
        let mut terminal = Terminal::new(TestBackend::new(width.max(1), 4)).unwrap();
        let mut hits = Vec::new();
        let mut rows = 0;
        terminal
            .draw(|frame| {
                (hits, rows) = draw(
                    frame,
                    Rect::new(0, 0, width, 4),
                    tabs,
                    active,
                    crate::tui::theme::palette(aw::config::ThemeMode::Dark),
                );
            })
            .unwrap();
        (terminal.backend().buffer().clone(), hits, rows)
    }

    #[test]
    fn painted_label_cells_are_inside_returned_hit_rectangles() {
        let tabs = tabs();
        for active in 1..=7 {
            let (buffer, hits, rows) = paint(24, &tabs, Some(active));
            assert_eq!(rows, 1);
            for (rect, _) in &hits {
                assert_eq!(rect.y, 0);
                assert_eq!(rect.height, 1);
                assert!(rect.width > 0 && rect.right() <= 24);
            }
            for x in 0..24 {
                if buffer[(x, 0)].symbol() != " " {
                    assert_eq!(
                        hits.iter()
                            .filter(|(rect, _)| x >= rect.x && x < rect.right())
                            .count(),
                        1,
                        "painted cell {x} must have exactly one matching hit"
                    );
                }
            }
            for y in 1..4 {
                assert!((0..24).all(|x| buffer[(x, y)].symbol() == " "));
            }
        }
    }

    #[test]
    fn overflow_keeps_middle_and_last_active_tabs_visible_on_one_row() {
        let palette = crate::tui::theme::palette(aw::config::ThemeMode::Dark);
        for active in [4, 7] {
            let (buffer, hits, rows) = paint(17, &tabs(), Some(active));
            assert_eq!(rows, 1);
            let rect = hits
                .iter()
                .find(|(rect, id)| *id == active && rect.width > 1)
                .unwrap()
                .0;
            let painted = (rect.x..rect.right())
                .map(|x| buffer[(x, 0)].symbol())
                .collect::<String>();
            assert!(painted.contains(&format!("Tab {active}")));
            assert_eq!(buffer[(rect.x, 0)].bg, palette.user_prompt_background);
        }
    }

    #[test]
    fn overflow_affordances_select_adjacent_hidden_tabs() {
        let tabs = tabs();
        let (buffer, hits, _) = paint(9, &tabs, Some(4));
        assert_eq!(buffer[(0, 0)].symbol(), "‹");
        assert_eq!(hits.first(), Some(&(Rect::new(0, 0, 1, 1), 3)));
        assert_eq!(buffer[(8, 0)].symbol(), "›");
        assert_eq!(hits.last(), Some(&(Rect::new(8, 0, 1, 1), 5)));
        // Ordinary selection of either arrow's exact ID moves that tab into view.
        for next in [3, 5] {
            let (_, hits, _) = paint(9, &tabs, Some(next));
            assert!(hits.iter().any(|(rect, id)| *id == next && rect.width == 7));
        }
    }

    #[test]
    fn clipping_preserves_complete_unicode_graphemes() {
        let label = "界cafe\u{301}👩‍💻tail";
        assert_eq!(truncate_cells(label, 1), "");
        assert_eq!(truncate_cells(label, 6), "界cafe\u{301}");
        assert_eq!(truncate_cells(label, 7), "界cafe\u{301}");
        assert_eq!(truncate_cells(label, 8), "界cafe\u{301}👩‍💻");
        let tabs = vec![(1, label.into(), Color::Red)];
        let (buffer, hits, rows) = paint(10, &tabs, Some(1));
        assert_eq!(rows, 1);
        assert_eq!(hits, vec![(Rect::new(0, 0, 10, 1), 1)]);
        assert_eq!(buffer[(6, 0)].symbol(), "e\u{301}");
        assert_eq!(buffer[(7, 0)].symbol(), "👩‍💻");
    }

    #[test]
    fn empty_and_tiny_areas_are_bounded_and_preserve_the_active_hit() {
        let mut tabs = tabs();
        tabs[3].1 = "界".repeat(40_000);
        for width in 0..=4 {
            let (_, hits, rows) = paint(width, &tabs, Some(4));
            assert_eq!(rows, u16::from(width > 0));
            assert!(hits
                .iter()
                .all(|(rect, _)| rect.height == 1 && rect.right() <= width));
            assert_eq!(
                hits.iter().filter(|(_, id)| *id == 4).count(),
                usize::from(width > 0)
            );
        }
        let (_, hits, rows) = paint(20, &[], None);
        assert!(hits.is_empty());
        assert_eq!(rows, 0);
    }

    #[test]
    fn fitting_tabs_keep_stable_order_and_quiet_inactive_labels() {
        let palette = crate::tui::theme::palette(aw::config::ThemeMode::Dark);
        let (buffer, hits, rows) = paint(60, &tabs(), Some(4));
        assert_eq!(rows, 1);
        assert_eq!(
            hits.iter().map(|(_, id)| *id).collect::<Vec<_>>(),
            (1..=7).collect::<Vec<_>>()
        );
        assert_eq!(buffer[(1, 0)].fg, palette.muted);
    }
}
