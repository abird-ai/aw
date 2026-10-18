use super::{agent_resources::LoadedResourceSummary, extensions, markdown, performance::PerformanceStats, status::StatusContext};
use aw_ui_extension::Slot;
use ratatui::{style::Style, text::{Line, Span}};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderMode {
    Full,
    Hidden,
    #[cfg(feature = "workspace")]
    Embedded,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AgentHeader {
    pub resources: LoadedResourceSummary,
}

impl AgentHeader {
    pub fn from_resources(resources: &LoadedResourceSummary) -> Self {
        Self { resources: resources.clone() }
    }
}

pub struct LiveProjection<'a> {
    pub answer: &'a str,
    pub tools: &'a [aw_agent_core::ToolRecord],
}

pub struct TranscriptProjection<'a> {
    pub header: &'a AgentHeader,
    pub header_mode: HeaderMode,
    pub turns: &'a [aw::conversation::Turn],
    pub pending: Option<&'a str>,
    pub live: Option<&'a LiveProjection<'a>>,
    pub ui: &'a aw::config::UiConfig,
    pub width: usize,
    pub padding_x: u8,
    pub last_performance: Option<&'a PerformanceStats>,
    pub highlight_code: bool,
}

fn filled_line(text: String, style: Style, width: usize) -> Line<'static> {
    let mut line = Line::styled(text, style);
    if width > 0 {
        let used = line.width();
        if used < width { line.spans.push(Span::styled(" ".repeat(width-used), style)); }
    }
    line
}

pub(crate) fn filled_block(mut body: Vec<String>, style: Style, width: usize) -> Vec<Line<'static>> {
    if body.is_empty() { body.push(String::new()); }
    let mut out=Vec::with_capacity(body.len()+2);
    out.push(filled_line(String::new(),style,width));
    out.extend(body.into_iter().map(|line|filled_line(line,style,width)));
    out.push(filled_line(String::new(),style,width));
    out
}

pub fn header_lines(header: &AgentHeader, mode: HeaderMode, ui: &aw::config::UiConfig) -> Vec<Line<'static>> {
    let p=super::theme::palette_for(ui); let mut out=Vec::new();
    if mode == HeaderMode::Hidden {
        return out;
    }
    if mode == HeaderMode::Full {
        out.push(Line::from(vec![Span::styled(format!("aw v{}", env!("CARGO_PKG_VERSION")), p.user_marker_style())]));
        out.push(Line::styled("Esc close · Ctrl+C clear/exit · F1 palette · / commands", Style::default().fg(p.muted)));
        out.push(Line::styled("aw can explain its own features and loaded resources.", Style::default().fg(p.muted)));
        out.push(Line::default());
    }
    const MAX_SECTION_ITEMS: usize = 8;
    let context = header.resources.context();
    let skills = header.resources.skills();
    // A minimal embedded Agent shouldn't start with two empty status cards.
    // Never suppress loaded Context/Skills or the explicit Full header.
    if mode != HeaderMode::Full && context.is_empty() && skills.is_empty() {
        return out;
    }
    if mode == HeaderMode::Full || !context.is_empty() {
        out.push(Line::styled("[Context]", p.markdown_heading_style()));
    if context.is_empty() { out.push(Line::styled("  none loaded", Style::default().fg(p.muted))); }
    else if context.len() <= MAX_SECTION_ITEMS { out.extend(context.iter().map(|v| Line::styled(format!("  {}", aw::presentation::safe_text(&v.label())), p.text_style()))); }
    else {
        out.push(Line::styled(format!("  {}", aw::presentation::safe_text(&context[0].label())), p.text_style()));
        let tail = MAX_SECTION_ITEMS.saturating_sub(1);
        let omitted = context.len().saturating_sub(1 + tail);
        out.push(Line::styled(format!("  … {omitted} earlier loaded context resource{}", if omitted == 1 { "" } else { "s" }), Style::default().fg(p.muted)));
        out.extend(context[context.len()-tail..].iter().map(|v| Line::styled(format!("  {}", aw::presentation::safe_text(&v.label())), p.text_style())));
    }
        out.push(Line::default());
    }
    if mode == HeaderMode::Full || !skills.is_empty() {
        out.push(Line::styled("[Skills]", p.markdown_heading_style()));
    if skills.is_empty() { out.push(Line::styled("  none loaded", Style::default().fg(p.muted))); }
    else {
        out.extend(skills.iter().take(MAX_SECTION_ITEMS).map(|v| Line::styled(format!("  {}", aw::presentation::safe_text(&v.label())), p.text_style())));
        if skills.len() > MAX_SECTION_ITEMS { out.push(Line::styled(format!("  … {} more loaded skills", skills.len()-MAX_SECTION_ITEMS), Style::default().fg(p.muted))); }
    }
        out.push(Line::default());
    }
    out
}

pub fn prompt_lines(
    prompt: &str,
    ui: &aw::config::UiConfig,
    width: usize,
) -> Vec<Line<'static>> {
    let palette = super::theme::palette_for(ui);
    let style = palette.user_prompt_style();
    let safe = aw::presentation::safe_text(prompt);
    let width = width.max(1);
    let mut body=Vec::new();
    for source in safe.split('\n') { body.extend(wrap_cells(source,width)); }
    filled_block(body,style,width)
}

pub fn turn_lines(
    turn: &aw::conversation::Turn,
    stats: &PerformanceStats,
    ui: &aw::config::UiConfig,
    width: usize,
    highlight_code: bool,
) -> Vec<Line<'static>> {
    let p=super::theme::palette_for(ui); let mut lines=Vec::new(); let thinking=stats.first_token.unwrap_or(stats.elapsed);
    let render_markdown = |text: &str| {
        if highlight_code {
            markdown::render(text, p, ui.theme, width)
        } else {
            markdown::render_plain(text, p, ui.theme, width)
        }
    };
    if !thinking.is_zero(){lines.push(Line::styled(if ui.show_thinking{format!("Thinking {:.1}s",thinking.as_secs_f64())}else{"Thinking · hidden".into()},p.thinking_style()));lines.push(Line::default());}
    let record_for=|id:&str|turn.tool_records.iter().find(|r|r.call.id==id); let trace=turn.context_messages.iter().skip(1).collect::<Vec<_>>(); let fallback=aw::presentation::settled_answer_fallback(turn); let mut assistant=false;
    for m in trace { match m.role {
        aw_agent_core::Role::Assistant=>{if !m.content.trim().is_empty(){assistant=true;lines.extend(render_markdown(&m.content));} for call in &m.tool_calls{let rec=record_for(&call.id);let style=p.tool_style(aw::presentation::tool_tone(rec));lines.push(Line::default());if ui.show_tool_calls{lines.extend(filled_block(vec![rec.map_or_else(
    || format!("{} · unknown", aw::presentation::safe_text(&format!("{}  {}",call.name,call.arguments))),
    |record| format!("{} · {}", aw::presentation::tool_title(record), factual_tool_status(record)),
)],style,width));}else{let status=rec.map(factual_tool_status).unwrap_or("unknown");lines.extend(filled_block(vec![format!("tool {} · {status} · hidden",call.name)],style,width));}}}
        aw_agent_core::Role::Tool if ui.show_tool_calls=>{let rec=m.tool_call_id.as_deref().and_then(record_for);let style=p.tool_style(aw::presentation::tool_tone(rec));let safe=aw::presentation::safe_text(&m.content);let all=safe.lines().collect::<Vec<_>>();const MAX:usize=12;let mut block=Vec::new();if all.len()>MAX{block.push(format!("… ({} earlier lines omitted)",all.len()-MAX));}block.extend(all.iter().skip(all.len().saturating_sub(MAX)).map(|line|(*line).to_owned()));lines.extend(filled_block(block,style,width));}
        _=>{}
    }}
    if let Some(f)=fallback{if assistant{lines.push(Line::default());}lines.extend(render_markdown(&f));assistant=true;}
    if turn.outcome!=aw::conversation::Outcome::Completed{if assistant{lines.push(Line::default());}let d=turn.error.as_deref().unwrap_or("model turn did not complete with visible output");lines.push(Line::styled(format!("! {}",aw::presentation::safe_text(d)),p.error_style()));}else if !assistant{lines.push(Line::styled("! completed turn contained no visible assistant output",p.error_style()));}
    let perf=extensions::project(Slot::AfterTurn,ui,&StatusContext::default(),Some(stats));if !perf.is_empty(){lines.push(Line::default());lines.extend(perf);}lines.push(Line::default());lines
}

pub fn progress_indicator(elapsed: std::time::Duration) -> (&'static str, &'static str, &'static str) {
    const FRAMES: [(&str, &str, &str); 4] = [
        ("▪", "▫", "▫"),
        ("▫", "▪", "▫"),
        ("▫", "▫", "▪"),
        ("▫", "▪", "▫"),
    ];
    FRAMES[((elapsed.as_millis() / 260) as usize) % FRAMES.len()]
}

fn elapsed_label(elapsed: std::time::Duration) -> String {
    let seconds = elapsed.as_secs();
    if seconds < 60 {
        format!("{seconds}s")
    } else {
        format!("{}m {:02}s", seconds / 60, seconds % 60)
    }
}

/// Pi-inspired activity embedded in the composer's top rule rather than
/// consuming a separate transcript/status row. The tiny three-box animation
/// is intentionally shared with background Session activity.
pub fn activity_border_line(
    width: u16,
    elapsed: std::time::Duration,
    border_color: ratatui::style::Color,
    theme: aw::config::ThemeMode,
) -> Line<'static> {
    let width = usize::from(width);
    if width == 0 {
        return Line::default();
    }
    let palette = super::theme::palette(theme);
    let active = Style::default().fg(palette.accent);
    let inactive = Style::default().fg(palette.faint);
    let border = Style::default().fg(border_color);
    let muted = Style::default().fg(palette.muted);
    let (a, b, c) = progress_indicator(elapsed);
    let elapsed = elapsed_label(elapsed);

    if width < 7 {
        return Line::styled("─".repeat(width), border);
    }

    let mut spans = vec![
        Span::styled("── ", border),
        Span::styled(a, if a == "▪" { active } else { inactive }),
        Span::styled(b, if b == "▪" { active } else { inactive }),
        Span::styled(c, if c == "▪" { active } else { inactive }),
    ];
    let mut used = 6usize;

    if width >= 18 {
        spans.push(Span::styled(" working", muted));
        used += 8;
    }

    let right_width = elapsed.len().saturating_add(1);
    if width >= used.saturating_add(right_width).saturating_add(2) {
        let fill = width.saturating_sub(used).saturating_sub(right_width);
        spans.push(Span::styled("─".repeat(fill), border));
        spans.push(Span::styled(format!(" {elapsed}"), muted));
    } else {
        spans.push(Span::styled("─".repeat(width.saturating_sub(used)), border));
    }
    Line::from(spans)
}

#[cfg(feature = "workspace")]
pub fn auxiliary_lines(
    label: &str,
    text: &str,
    ui: &aw::config::UiConfig,
    width: usize,
) -> Vec<Line<'static>> {
    let palette = super::theme::palette_for(ui);
    let mut lines = vec![Line::from(vec![
        Span::styled(
            aw::presentation::safe_text(label),
            palette.markdown_heading_style(),
        ),
        Span::styled(
            " · out-of-band; not added to Agent history",
            Style::default().fg(palette.muted),
        ),
    ])];
    lines.push(Line::default());
    lines.extend(markdown::render(
        &aw::presentation::safe_text(text),
        palette,
        ui.theme,
        width.max(1),
    ));
    lines
}

/// Text is the authoritative outcome, even in monochrome/16-color terminals.
/// A misleading "succeeded" tool status with an error output is a failure.
fn factual_tool_status(record: &aw_agent_core::ToolRecord) -> &'static str {
    if record.output.as_ref().is_some_and(|output| output.is_error) {
        return "failed";
    }
    match record.status {
        aw_agent_core::ToolStatus::Running => "running",
        aw_agent_core::ToolStatus::Succeeded => "succeeded",
        aw_agent_core::ToolStatus::Failed => "failed",
        aw_agent_core::ToolStatus::Denied => "denied",
        aw_agent_core::ToolStatus::Interrupted => "interrupted",
    }
}

pub fn live_lines(live: &LiveProjection<'_>, ui: &aw::config::UiConfig, width: usize) -> Vec<Line<'static>> {
    let p = super::theme::palette_for(ui);
    let mut lines = Vec::new();
    if !live.answer.is_empty() {
        lines.extend(
            aw::presentation::safe_text(live.answer)
                .lines()
                .map(|line| Line::styled(line.to_owned(), p.text_style())),
        );
    }
    for record in live.tools {
        let style = p.tool_style(aw::presentation::tool_tone(Some(record)));
        lines.push(Line::default());
        let mut block=Vec::new();
        if ui.show_tool_calls {
            block.push(format!("{} · {}", aw::presentation::tool_title(record), factual_tool_status(record)));
            if let Some(output)=&record.output { block.extend(aw::presentation::safe_text(&output.content).lines().take(12).map(str::to_owned)); }
        } else {
            block.push(format!("tool {} · {} · hidden", record.call.name, factual_tool_status(record)));
        }
        lines.extend(filled_block(block,style,width));
    }
    lines
}

pub fn transcript_lines(state: TranscriptProjection<'_>) -> Vec<Line<'static>> {
    let TranscriptProjection {
        header,
        header_mode,
        turns,
        pending,
        live,
        ui,
        width,
        padding_x,
        last_performance,
        highlight_code,
    } = state;
    let padding = usize::from(padding_x).min(width / 2);
    let content_width = width.saturating_sub(padding.saturating_mul(2)).max(1);
    let mut out = header_lines(header, header_mode, ui);
    let mut hist = ui.clone();
    hist.show_performance = false;
    hist.show_ttft = false;
    hist.show_waterfall = false;
    let zero = PerformanceStats::default();
    for (index, turn) in turns.iter().enumerate() {
        if !out.is_empty() {
            out.push(Line::default());
        }
        out.extend(prompt_lines(&turn.prompt, ui, content_width));
        out.push(Line::default());
        let stats = if index + 1 == turns.len() {
            last_performance.unwrap_or(&zero)
        } else {
            &zero
        };
        out.extend(turn_lines(
            turn,
            stats,
            &hist,
            content_width,
            highlight_code,
        ));
    }
    if let Some(prompt) = pending {
        if !out.is_empty() {
            out.push(Line::default());
        }
        out.extend(prompt_lines(prompt, ui, content_width));
        out.push(Line::default());
        if let Some(live) = live {
            out.extend(live_lines(live, ui, content_width));
        }
    }
    inset_lines(&mut out, padding as u8);
    out
}

pub fn inset_lines(lines: &mut [Line<'static>], padding_x: u8) {
    let padding = usize::from(padding_x);
    if padding == 0 {
        return;
    }
    let spaces = " ".repeat(padding);
    for line in lines {
        let style = line.style;
        line.spans
            .insert(0, Span::styled(spaces.clone(), style));
        line.spans.push(Span::styled(spaces.clone(), style));
    }
}

fn wrap_cells(text: &str, max: usize) -> Vec<String> {
    use unicode_segmentation::UnicodeSegmentation;
    use unicode_width::UnicodeWidthStr;

    let max = max.max(1);
    if text.is_empty() {
        return vec![String::new()];
    }

    let mut lines = Vec::new();
    let mut line = String::new();
    let mut used = 0usize;
    for grapheme in UnicodeSegmentation::graphemes(text, true) {
        let width = UnicodeWidthStr::width(grapheme).max(1);
        if !line.is_empty() && used.saturating_add(width) > max {
            lines.push(std::mem::take(&mut line));
            used = 0;
        }
        line.push_str(grapheme);
        used = used.saturating_add(width);
        // A single indivisible wide grapheme may exceed an extremely narrow
        // viewport. Keep it intact and let the terminal crop that one cell
        // rather than dropping prompt data or looping forever.
        if used >= max {
            lines.push(std::mem::take(&mut line));
            used = 0;
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

fn truncate_cells(text: &str, max: usize) -> String {
    use unicode_segmentation::UnicodeSegmentation;
    use unicode_width::UnicodeWidthStr;
    if max == 0 { return String::new(); }
    let mut out=String::new(); let mut used=0usize;
    for g in UnicodeSegmentation::graphemes(text,true){let w=UnicodeWidthStr::width(g);if used.saturating_add(w)>max{break;}out.push_str(g);used=used.saturating_add(w);}
    out
}

pub fn composer_placeholder(width: u16, busy: bool, editing: bool) -> String {
    let full = if busy {
        "Ask anything…  next turn · owner busy · / commands"
    } else if editing {
        "Ask anything…  / for commands"
    } else {
        "Ask anything…  i edit · / commands"
    };
    if width < 18 { "Ask anything…".into() }
    else if width < 42 { "Ask anything…  / commands".into() }
    else { truncate_cells(full, usize::from(width)) }
}

#[cfg(all(test, feature = "workspace"))]
mod tests {
    use super::*;
    #[cfg(feature = "workspace")]
    fn plain(lines:&[Line<'_>])->String{lines.iter().flat_map(|l|l.spans.iter()).map(|s|s.content.as_ref()).collect::<Vec<_>>().join("\n")}
    #[cfg(feature = "workspace")]
    #[test]
    fn full_and_embedded_headers_share_resources_but_only_full_has_outer_product_help(){
        let ui=aw::config::UiConfig::default();
        let mut resources=super::super::agent_resources::LoadedResourceSummary::with_system_prompt(super::super::agent_resources::SystemPromptOrigin::BuiltIn,128);
        resources.add_instruction_file("./AGENTS.md",42);
        let h=AgentHeader::from_resources(&resources);
        let full=plain(&header_lines(&h,HeaderMode::Full,&ui));let embedded=plain(&header_lines(&h,HeaderMode::Embedded,&ui));
        for token in ["[Context]", "system prompt · built-in · 128 B", "instructions · ./AGENTS.md · 42 B"] {
            assert!(full.contains(token));
            assert!(embedded.contains(token));
        }
        assert!(full.contains("[Skills]") && full.contains("none loaded"));
        assert!(!embedded.contains("[Skills]") && !embedded.contains("none loaded"));
        assert!(full.contains("F1 palette"));assert!(!full.contains("F1 help"));
        assert!(full.contains("aw v"));assert!(full.contains("/ commands"));assert!(!embedded.contains("aw v"));assert!(!embedded.contains("/ commands"));
    }

    #[test]
    fn embedded_header_is_quiet_when_empty_and_never_hides_loaded_resources() {
        let ui = aw::config::UiConfig::default();
        let none = AgentHeader::default();
        assert!(header_lines(&none, HeaderMode::Embedded, &ui).is_empty());
        assert!(header_lines(&none, HeaderMode::Hidden, &ui).is_empty());
        let explicit_full = plain(&header_lines(&none, HeaderMode::Full, &ui));
        assert!(explicit_full.contains("[Context]") && explicit_full.contains("[Skills]"));

        let mut resources = super::super::agent_resources::LoadedResourceSummary::default();
        resources.add_skill("review", "./SKILL.md", 100);
        let skills_only = plain(&header_lines(
            &AgentHeader::from_resources(&resources), HeaderMode::Embedded, &ui,
        ));
        assert!(!skills_only.contains("[Context]"));
        assert!(skills_only.contains("[Skills]"));
        assert!(skills_only.contains("review"));

        resources.add_instruction_file("./AGENTS.md", 42);
        let both = plain(&header_lines(
            &AgentHeader::from_resources(&resources), HeaderMode::Embedded, &ui,
        ));
        assert!(both.contains("[Context]") && both.contains("[Skills]"));
        assert!(both.contains("./AGENTS.md"));
    }

    #[test]
    fn native_tool_blocks_include_factual_status_words_in_all_color_modes() {
        use aw_agent_core::{ToolCall, ToolEffect, ToolOutput, ToolRecord, ToolStatus};
        let make = |name: &str, status, output| ToolRecord {
            call: ToolCall {
                id: name.to_owned(),
                name: name.to_owned(),
                arguments: serde_json::json!({}),
            },
            effect: ToolEffect::Process,
            execution_site: "synthetic".into(),
            status,
            output,
        };
        let records = vec![
            make("inflight", ToolStatus::Running, None),
            make("finished", ToolStatus::Succeeded, None),
            make("denied", ToolStatus::Denied, None),
            make("interrupted", ToolStatus::Interrupted, None),
            make("failure", ToolStatus::Failed, None),
            make("inconsistent", ToolStatus::Succeeded, Some(ToolOutput::error("not a success"))),
        ];
        for mode in [
            aw::config::ThemeMode::System,
            aw::config::ThemeMode::Light,
            aw::config::ThemeMode::Dark,
        ] {
            let ui = aw::config::UiConfig {
                theme: mode,
                ..aw::config::UiConfig::default()
            };
            let view = LiveProjection { answer: "", tools: &records };
            let full = plain(&live_lines(&view, &ui, 80));
            for label in [
                "inflight · running", "finished · succeeded", "denied · denied",
                "interrupted · interrupted", "failure · failed",
                "inconsistent · failed",
            ] {
                assert!(full.contains(label), "missing factual status: {label}");
            }
            let mut hidden = ui.clone();
            hidden.show_tool_calls = false;
            let summary = plain(&live_lines(&view, &hidden, 80));
            assert!(summary.contains("tool denied · denied · hidden"));
            assert!(summary.contains("tool inconsistent · failed · hidden"));
            assert!(!summary.contains("not a success"));
        }
    }

    #[test]
    fn user_prompt_is_a_full_width_muted_band_without_marker() {
        let ui = aw::config::UiConfig {
            theme: aw::config::ThemeMode::Dark,
            ..aw::config::UiConfig::default()
        };
        let palette = super::super::theme::palette_for(&ui);
        let lines = prompt_lines("hello", &ui, 20);
        assert_eq!(lines.len(), 3);
        assert!(lines.iter().all(|line| line.width() == 20));
        let text = lines[1]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert!(text.starts_with("hello"));
        assert!(!text.starts_with(' '), "padding=0 must not invent a horizontal inset");
        assert!(!text.contains('›'));
        assert!(lines.iter().all(|line| line.style.bg == Some(palette.user_prompt_background)));
        assert!(lines[0].spans.iter().all(|span| span.content.trim().is_empty()));
        assert!(lines[2].spans.iter().all(|span| span.content.trim().is_empty()));
    }

    #[test]
    fn user_prompt_band_wraps_without_dropping_prompt_text() {
        let ui = aw::config::UiConfig {
            theme: aw::config::ThemeMode::Dark,
            ..aw::config::UiConfig::default()
        };
        let lines = prompt_lines("abcdefghij", &ui, 6);
        assert_eq!(lines.len(), 4);
        let reconstructed = lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.trim().to_owned())
            .collect::<String>();
        assert_eq!(reconstructed, "abcdefghij");
        assert!(lines.iter().all(|line| line.width() == 6));
    }

    #[test]
    fn standalone_output_padding_is_symmetric_and_consumed_by_transcript_projection() {
        let ui = aw::config::UiConfig {
            output_padding: 2,
            ..aw::config::UiConfig::default()
        };
        let header = AgentHeader::default();
        let lines = transcript_lines(TranscriptProjection {
            header: &header,
            header_mode: HeaderMode::Hidden,
            turns: &[],
            pending: Some("hello"),
            live: None,
            ui: &ui,
            width: 20,
            padding_x: ui.output_padding,
            last_performance: None,
            highlight_code: false,
        });
        let prompt = lines
            .iter()
            .find(|line| {
                line.spans
                    .iter()
                    .any(|span| span.content.as_ref().contains("hello"))
            })
            .expect("prompt line");
        let visible = prompt
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert!(visible.starts_with("  hello"));
        assert!(visible.ends_with("  "));
        assert_eq!(prompt.width(), 20);
    }

    #[test]
    fn activity_border_uses_three_box_animation_and_right_aligned_elapsed_time() {
        let line = activity_border_line(
            40,
            std::time::Duration::from_secs(65),
            ratatui::style::Color::Gray,
            aw::config::ThemeMode::Dark,
        );
        let text = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert_eq!(line.width(), 40);
        assert!(text.contains('▪'));
        assert!(text.matches('▫').count() >= 2);
        assert!(text.contains("working"));
        assert!(text.ends_with("1m 05s"));
    }
}
