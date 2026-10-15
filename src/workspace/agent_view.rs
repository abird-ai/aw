use super::owner::Snapshot;
use crate::tui::performance::PerformanceStats;
use ratatui::text::Line;

pub(super) fn settled_lines(
    turns: &[aw::conversation::Turn],
    resources: &crate::tui::agent_resources::LoadedResourceSummary,
    ui: &aw::config::UiConfig,
    performance: Option<&PerformanceStats>,
    width: u16,
    highlight_code: bool,
) -> Vec<Line<'static>> {
    let header = crate::tui::agent_chrome::AgentHeader::from_resources(resources);
    crate::tui::agent_chrome::transcript_lines(crate::tui::agent_chrome::TranscriptProjection {
        header: &header,
        header_mode: crate::tui::agent_chrome::HeaderMode::Embedded,
        turns,
        pending: None,
        live: None,
        ui,
        width: usize::from(width),
        padding_x: ui.workspace_output_padding,
        last_performance: performance,
        highlight_code,
    })
}

pub(super) fn lines(
    snapshot: &Snapshot,
    ui: &aw::config::UiConfig,
    width: u16,
    highlight_code: bool,
) -> Vec<Line<'static>> {
    let inner_width = usize::from(width)
        .saturating_sub(usize::from(ui.workspace_output_padding).saturating_mul(2))
        .max(1);
    let mut lines = settled_lines(
        &snapshot.turns,
        &snapshot.loaded_resources,
        ui,
        snapshot.performance.as_ref(),
        width,
        highlight_code,
    );

    if let Some(answer) = snapshot.auxiliary_output.as_deref() {
        if !lines.is_empty() {
            lines.push(Line::default());
        }
        let mut auxiliary =
            crate::tui::agent_chrome::auxiliary_lines("BTW", answer, ui, inner_width);
        crate::tui::agent_chrome::inset_lines(&mut auxiliary, ui.workspace_output_padding);
        lines.extend(auxiliary);
    }

    if !snapshot.pending_prompt.is_empty() {
        if !lines.is_empty() {
            lines.push(Line::default());
        }
        let mut live_lines =
            crate::tui::agent_chrome::prompt_lines(&snapshot.pending_prompt, ui, inner_width);
        live_lines.push(Line::default());
        let live = crate::tui::agent_chrome::LiveProjection {
            answer: &snapshot.live,
            tools: &snapshot.live_tools,
        };
        live_lines.extend(crate::tui::agent_chrome::live_lines(
            &live,
            ui,
            inner_width,
        ));
        crate::tui::agent_chrome::inset_lines(&mut live_lines, ui.workspace_output_padding);
        lines.extend(live_lines);
    }
    lines
}
