use super::{performance::PerformanceStats, status::StatusContext};
use aw::config::{StatusItem, StatusLineConfig, StatusRegion, UiConfig};
use aw_ui_extension::{
    Context, Contribution, Line as ExtensionLine, Manifest, Performance, Registry, Slot, Span,
    Tone, UiExtension,
};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span as TuiSpan},
};
use std::sync::Arc;
use unicode_width::UnicodeWidthStr;

const STATUS_ID: &str = "builtin.status-line";
const PERFORMANCE_ID: &str = "builtin.performance";
const FACTUAL_NOTICES_ID:&str="builtin.factual-notices";

#[derive(Clone)]
struct StatusLineExtension {
    lines: Vec<StatusLineConfig>,
}

impl UiExtension for StatusLineExtension {
    fn manifest(&self) -> Manifest {
        Manifest {
            id: STATUS_ID.into(),
            protocol_version: 2,
            slots: vec![
                Slot::AboveComposer,
                Slot::BelowComposer,
                Slot::FullscreenStatus,
                Slot::LeftStatus,
                Slot::RightStatus,
                Slot::BelowStatus,
            ],
            settings: Vec::new(),
        }
    }

    fn project(&self, slot: Slot, context: &Context) -> Vec<Contribution> {
        let Some(region) = region_for_slot(slot) else {
            return Vec::new();
        };
        let mut lines = Vec::new();
        for config in self.lines.iter().filter(|line| line.region == region) {
            let mut spans = status_spans(&config.items, context);
            let right = status_spans(&config.right_items, context);
            if !spans.is_empty() && !right.is_empty() {
                spans.push(Span::text(" · ", Tone::Muted));
            }
            spans.extend(right);
            if !spans.is_empty() {
                lines.push(ExtensionLine { spans });
            }
        }
        if lines.is_empty() {
            return Vec::new();
        }
        vec![Contribution {
            extension: STATUS_ID.into(),
            slot,
            order: 10,
            lines,
        }]
    }
}

fn region_for_slot(slot: Slot) -> Option<StatusRegion> {
    match slot {
        Slot::AboveComposer => Some(StatusRegion::AboveComposer),
        Slot::BelowComposer => Some(StatusRegion::BelowComposer),
        Slot::FullscreenStatus => Some(StatusRegion::FullscreenStatus),
        Slot::LeftStatus => Some(StatusRegion::LeftStatus),
        Slot::RightStatus => Some(StatusRegion::RightStatus),
        Slot::BelowStatus => Some(StatusRegion::BelowStatus),
        Slot::AfterTurn | Slot::AfterStep | Slot::AfterFinalOutput => None,
    }
}

#[derive(Clone, Copy)]
struct PerformanceExtension {
    show: bool,
    show_ttft: bool,
    show_waterfall: bool,
}

impl UiExtension for PerformanceExtension {
    fn manifest(&self) -> Manifest {
        Manifest {
            id: PERFORMANCE_ID.into(),
            protocol_version: 1,
            slots: vec![Slot::AfterTurn],
            settings: Vec::new(),
        }
    }

    fn project(&self, slot: Slot, context: &Context) -> Vec<Contribution> {
        if !self.show {
            return Vec::new();
        }
        let Some(stats) = context.performance.as_ref() else {
            return Vec::new();
        };
        let mut parts = Vec::new();
        if let Some(tps) = stats.output_tps {
            parts.push(format!("{tps:.0}t/s"));
        }
        if let Some(input) = stats.input_tokens {
            parts.push(format!("↑{}", fmt_tokens(input)));
        }
        if let Some(output) = stats.output_tokens {
            parts.push(format!("↓{}", fmt_tokens(output)));
        }
        if self.show_ttft {
            if let Some(first) = stats.first_token_secs {
                parts.push(format!("TTFT {first:.2}s"));
            }
        }
        parts.push(format!("{:.1}s", stats.elapsed_secs));

        let mut lines = vec![ExtensionLine::plain(parts.join("  "), Tone::Performance)];
        if self.show_waterfall {
            if let Some(waterfall) = waterfall(stats, 36) {
                lines.push(ExtensionLine::plain(waterfall, Tone::Muted));
            }
        }
        vec![Contribution {
            extension: PERFORMANCE_ID.into(),
            slot,
            order: 20,
            lines,
        }]
    }
}

/// Opt-in, strictly data-only v2 event projection. This is NOT a new Tool,
/// guardian policy, provider listener or display chrome; it only enriches
/// the already existing Activity tab when user config explicitly enables it.
struct FactualNoticeExtension;
impl UiExtension for FactualNoticeExtension {
    fn manifest(&self)->Manifest {
        Manifest {id:FACTUAL_NOTICES_ID.into(),protocol_version:2,
            slots:vec![Slot::AfterStep,Slot::AfterTurn,Slot::AfterFinalOutput],settings:Vec::new()}
    }
    fn project(&self,_:Slot,_:&Context)->Vec<Contribution>{Vec::new()}
    fn event(&self,event:&aw_ui_extension::status_events::EventContext)->Vec<Contribution> {
        use aw_ui_extension::status_events::EventKind;
        let label=match event.kind {
            EventKind::StepEnd=>"Model step settled",
            EventKind::TurnEnd=>"Turn settled",
            EventKind::FinalOutput=>"Final answer available",
        };
        vec![Contribution {extension:FACTUAL_NOTICES_ID.into(),slot:event.kind.slot(),order:50,
            lines:vec![ExtensionLine::plain(format!("{label} · turn {}",event.turn_id),Tone::Muted)]}]
    }
}

pub fn context(
    status: &StatusContext,
    performance: Option<&PerformanceStats>,
) -> Context {
    Context {
        model: status.model.clone(),
        profile: status.profile.clone(),
        provider: status.provider.clone(),
        effort: status.effort.clone(),
        directory: status.directory.clone(),
        agent: status.agent.clone(),
        branch: status.branch.clone(),
        tools: status.tools.clone(),
        session: status.session.clone(),
        state: status.state.clone(),
        session_tps: status.session_tps,
        performance: performance.map(|stats| Performance {
            elapsed_secs: stats.elapsed.as_secs_f64(),
            first_token_secs: stats.first_token.map(|value| value.as_secs_f64()),
            input_tokens: stats.input_tokens,
            output_tokens: stats.output_tokens,
            output_tps: stats.output_tps(),
        }),
    }
}

pub fn project(
    slot: Slot,
    ui: &UiConfig,
    status: &StatusContext,
    performance: Option<&PerformanceStats>,
) -> Vec<Line<'static>> {
    let registry = registry(ui);
    let context = context(status, performance);
    registry
        .project(slot, &context)
        .unwrap_or_default()
        .into_iter()
        .map(to_tui_line)
        .collect()
}

/// Width-aware status projection for every configured row.
///
/// Left/right groups are part of the persisted typed row model, so defaults and
/// customized layouts use one renderer rather than a magic default-only path.
pub fn project_status(
    slot: Slot,
    ui: &UiConfig,
    status: &StatusContext,
    _performance: Option<&PerformanceStats>,
    width: u16,
) -> Vec<Line<'static>> {
    let Some(region) = region_for_slot(slot) else {
        return Vec::new();
    };
    let width = usize::from(width);
    if width == 0 {
        return Vec::new();
    }
    let context = context(status, None);
    ui.status_lines
        .iter()
        .filter(|line| line.region == region)
        .filter_map(|line| {
            let left = status_tui_spans(&line.items, &context);
            let right = status_tui_spans(&line.right_items, &context);
            (!left.is_empty() || !right.is_empty()).then(|| balanced_line(left, right, width))
        })
        .collect()
}

fn spans_width(spans: &[TuiSpan<'_>]) -> usize {
    spans
        .iter()
        .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
        .sum()
}

fn balanced_line(
    mut left: Vec<TuiSpan<'static>>,
    right: Vec<TuiSpan<'static>>,
    width: usize,
) -> Line<'static> {
    let left_width = spans_width(&left);
    let right_width = spans_width(&right);
    if left.is_empty() {
        if right_width < width {
            left.push(TuiSpan::raw(" ".repeat(width - right_width)));
        }
        left.extend(right);
        return Line::from(left);
    }
    if left_width.saturating_add(right_width).saturating_add(1) <= width {
        left.push(TuiSpan::raw(" ".repeat(width - left_width - right_width)));
        left.extend(right);
        return Line::from(left);
    }
    Line::from(left)
}

fn registry(ui: &UiConfig) -> Registry {
    let mut extensions=vec![
        Arc::new(StatusLineExtension{lines:ui.status_lines.clone()}) as Arc<dyn UiExtension>,
        Arc::new(PerformanceExtension{
            show:ui.show_performance,show_ttft:ui.show_ttft,
            show_waterfall:ui.show_waterfall,
        }) as Arc<dyn UiExtension>,
    ];
    if ui.show_event_notices {
        extensions.push(Arc::new(FactualNoticeExtension) as Arc<dyn UiExtension>);
    }
    Registry::new(extensions).expect("built-in UI extension registry is valid")
}
/// Explicitly opt-in v2 event subscriber of the original, existing UI
/// registry. The host never installs it for the default configuration.
#[cfg(any(feature = "workspace", test))]
pub fn event_registry(ui:&UiConfig)->Registry { registry(ui) }

fn status_spans(items: &[StatusItem], context: &Context) -> Vec<Span> {
    let mut spans = Vec::new();
    for item in items {
        let Some(mut span) = status_span(*item, context) else {
            continue;
        };
        if !spans.is_empty() {
            spans.push(Span::text(" · ", Tone::Muted));
        }
        if matches!(item, StatusItem::Model) {
            span.bold = true;
        }
        spans.push(span);
    }
    spans
}

fn status_tui_spans(items: &[StatusItem], context: &Context) -> Vec<TuiSpan<'static>> {
    status_spans(items, context)
        .into_iter()
        .map(to_tui_span)
        .collect()
}

fn status_span(item: StatusItem, context: &Context) -> Option<Span> {
    match item {
        StatusItem::Model if !context.model.is_empty() => {
            Some(Span::text(context.model.clone(), Tone::Model))
        }
        StatusItem::Profile if !context.profile.is_empty() => {
            Some(Span::text(context.profile.clone(), Tone::Profile))
        }
        StatusItem::Provider if !context.provider.is_empty() => {
            Some(Span::text(format!("[{}]", context.provider), Tone::Provider))
        }
        StatusItem::Effort if !context.effort.is_empty() => {
            Some(Span::text(context.effort.clone(), Tone::Effort))
        }
        StatusItem::SessionTps => context
            .session_tps
            .map(|tps| Span::text(format!("{tps:.0} avg t/s"), Tone::Performance)),
        StatusItem::Directory if !context.directory.is_empty() => Some(Span::text(
            compact_path(&context.directory),
            Tone::Directory,
        )),
        StatusItem::Agent if !context.agent.is_empty() => Some(Span::text(
            format!("agent {}", context.agent),
            Tone::Agent,
        )),
        StatusItem::Branch if !context.branch.is_empty() => {
            Some(Span::text(context.branch.clone(), Tone::Branch))
        }
        StatusItem::Tools if !context.tools.is_empty() => Some(Span::text(
            format!("tools {}", context.tools),
            if context.tools == "off" {
                Tone::Muted
            } else {
                Tone::Tools
            },
        )),
        StatusItem::Session if !context.session.is_empty() => {
            Some(Span::text(context.session.clone(), Tone::Session))
        }
        StatusItem::State if !context.state.is_empty() => {
            Some(Span::text(context.state.clone(), Tone::State))
        }
        _ => None,
    }
}

fn compact_path(path: &str) -> String {
    let Some(home) = std::env::var_os("HOME") else {
        return path.to_owned();
    };
    let path_value = std::path::Path::new(path);
    let home = std::path::PathBuf::from(home);
    match path_value.strip_prefix(&home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.to_owned(),
    }
}

fn fmt_tokens(value: u64) -> String {
    if value < 1_000 {
        value.to_string()
    } else if value < 10_000 {
        format!("{:.1}k", value as f64 / 1_000.0)
    } else if value < 1_000_000 {
        format!("{}k", value / 1_000)
    } else {
        format!("{:.1}M", value as f64 / 1_000_000.0)
    }
}

fn waterfall(stats: &Performance, width: usize) -> Option<String> {
    let first = stats.first_token_secs?;
    if stats.elapsed_secs <= 0.0 || width < 4 {
        return None;
    }
    let ratio = (first / stats.elapsed_secs).clamp(0.0, 1.0);
    let wait = (ratio * width as f64).round() as usize;
    Some(format!("{}{}", "·".repeat(wait), "━".repeat(width.saturating_sub(wait))))
}

fn to_tui_span(span: Span) -> TuiSpan<'static> {
    let mut style = tone_style(span.tone);
    if span.bold {
        style = style.add_modifier(Modifier::BOLD);
    }
    if span.italic {
        style = style.add_modifier(Modifier::ITALIC);
    }
    // Status metadata may contain user-selected Session titles or paths.
    // They are never terminal control sequences, multiline redraw commands,
    // or bidi control even if a third-party extension passed them as data.
    let safe=aw::presentation::safe_text(&span.text)
        .replace(['\n', '\t'], " ");
    TuiSpan::styled(safe.chars().take(512).collect::<String>(), style)
}

fn to_tui_line(line: ExtensionLine) -> Line<'static> {
    Line::from(line.spans.into_iter().map(to_tui_span).collect::<Vec<_>>())
}

fn tone_style(tone: Tone) -> Style {
    let color = match tone {
        Tone::Normal => Color::Gray,
        Tone::Muted => Color::DarkGray,
        Tone::Accent => Color::LightCyan,
        Tone::Model => Color::LightYellow,
        Tone::Profile | Tone::Provider => Color::LightBlue,
        Tone::Effort => Color::LightMagenta,
        Tone::Directory => Color::Green,
        Tone::Agent | Tone::Branch => Color::Cyan,
        Tone::Tools => Color::Gray,
        Tone::Session => Color::Magenta,
        Tone::State => Color::LightMagenta,
        Tone::Performance => Color::Gray,
        Tone::Good => Color::LightGreen,
        Tone::Warning => Color::LightYellow,
        Tone::Error => Color::LightRed,
    };
    Style::default().fg(color)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_extension_owns_multiple_ordered_rows_and_slots() {
        let ui = UiConfig {
            status_lines: vec![
                StatusLineConfig {
                    region: StatusRegion::AboveComposer,
                    items: vec![StatusItem::State],
                    right_items: Vec::new(),
                },
                StatusLineConfig {
                    region: StatusRegion::BelowComposer,
                    items: vec![StatusItem::Branch, StatusItem::Model],
                    right_items: Vec::new(),
                },
                StatusLineConfig {
                    region: StatusRegion::BelowComposer,
                    items: vec![StatusItem::Profile],
                    right_items: Vec::new(),
                },
                StatusLineConfig {
                    region: StatusRegion::FullscreenStatus,
                    items: vec![StatusItem::Model, StatusItem::Branch],
                    right_items: Vec::new(),
                },
            ],
            ..UiConfig::default()
        };
        let status = StatusContext {
            model: "m".into(),
            profile: "p".into(),
            branch: "main".into(),
            state: "ready".into(),
            ..StatusContext::default()
        };

        let above = project(Slot::AboveComposer, &ui, &status, None);
        assert_eq!(above.len(), 1);
        assert_eq!(
            above[0]
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>(),
            "ready"
        );

        let below = project(Slot::BelowComposer, &ui, &status, None);
        assert_eq!(below.len(), 2);
        assert_eq!(
            below[0]
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>(),
            "main · m"
        );
        assert_eq!(below[1].spans[0].content.as_ref(), "p");

        let fullscreen = project(Slot::FullscreenStatus, &ui, &status, None);
        assert_eq!(fullscreen.len(), 1);
        assert_eq!(
            fullscreen[0]
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>(),
            "m · main"
        );
    }

    #[test]
    fn width_aware_status_uses_same_typed_left_right_rows_for_custom_layouts() {
        let ui = UiConfig {
            status_lines: vec![StatusLineConfig {
                region: StatusRegion::BelowComposer,
                items: vec![StatusItem::Branch, StatusItem::SessionTps],
                right_items: vec![
                    StatusItem::Model,
                    StatusItem::Provider,
                    StatusItem::Effort,
                ],
            }],
            ..UiConfig::default()
        };
        let status = StatusContext {
            model: "gpt".into(),
            provider: "openai".into(),
            effort: "high".into(),
            branch: "main".into(),
            session_tps: Some(42.4),
            ..StatusContext::default()
        };
        let lines = project_status(Slot::BelowComposer, &ui, &status, None, 60);
        assert_eq!(lines.len(), 1);
        let text = lines[0]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert!(text.starts_with("main · 42 avg t/s"));
        assert!(text.ends_with("gpt · [openai] · high"));
        assert_eq!(lines[0].width(), 60);
    }

    #[test]
    fn performance_is_owned_by_after_turn_extension() {
        let ui = UiConfig::default();
        let stats = PerformanceStats {
            elapsed: std::time::Duration::from_secs(2),
            first_token: Some(std::time::Duration::from_millis(500)),
            input_tokens: Some(2_000),
            output_tokens: Some(100),
            throughput_eligible: true,
        };
        let lines = project(
            Slot::AfterTurn,
            &ui,
            &StatusContext::default(),
            Some(&stats),
        );
        let text = lines[0].spans[0].content.as_ref();
        assert!(text.contains("67t/s"));
        assert!(text.contains("↑2.0k"));
    }
}

#[cfg(test)]
mod aw_p6_8c_opt_in_factual_notice_tests {
    use super::*;
    use aw_ui_extension::status_events::{EventContext,EventKind,EventRail,STATUS_PROTOCOL};
    #[test]
    fn aw_p6_8c_no_automatic_events_unless_explicitly_enabled_in_current_ui_config() {
        let status=StatusContext{session:"s_approved".into(),state:"ready".into(),..StatusContext::default()};
        let event=EventContext {protocol_version:STATUS_PROTOCOL,kind:EventKind::TurnEnd,
            session:status.session.clone(),turn_id:2,sequence:1,
            context:context(&status,None)};
        let default=UiConfig::default();
        assert!(!default.show_event_notices);
        assert!(event_registry(&default).event_lines(&event).unwrap().is_empty());
        let mut enabled=default.clone();
        enabled.show_event_notices=true;
        let reg=event_registry(&enabled);
        let mut rail=EventRail::new("s_approved").unwrap();
        let notice=rail.publish(&reg,&event).unwrap().unwrap();
        assert_eq!(notice.kind,EventKind::TurnEnd);
        assert_eq!(notice.lines[0].spans[0].text,"Turn settled · turn 2");
        assert!(rail.publish(&reg,&event).is_err(),"duplicate UI event sequence is never accepted");
        assert_eq!(rail.notices().len(),1);
        // This never activates a Tool/agent; StatusLineExtension remains
        // presentation-only and the old AfterTurn performance slot remains.
        assert_eq!(enabled.status_lines,default.status_lines);
    }
}

#[cfg(test)]
mod aw_p6_8b_status_region_tests {
    use super::*;
    #[test]
    fn aw_p6_8b_native_regions_use_same_bounded_semantic_status_rows_without_new_defaults() {
        let ui = UiConfig {
            status_lines: vec![
            StatusLineConfig {
                region: StatusRegion::LeftStatus,
                items: vec![StatusItem::Agent],
                right_items: vec![],
            },
            StatusLineConfig {
                region: StatusRegion::RightStatus,
                items: vec![StatusItem::Branch],
                right_items: vec![],
            },
            StatusLineConfig {
                region: StatusRegion::BelowStatus,
                items: vec![StatusItem::State],
                right_items: vec![],
            },
            ],
            ..UiConfig::default()
        };
        let status = StatusContext {
            agent: "original".into(),
            branch: "main".into(),
            state: "ready".into(),
            ..StatusContext::default()
        };
        assert_eq!(
            project_status(Slot::LeftStatus, &ui, &status, None, 32).len(),
            1
        );
        assert_eq!(
            project_status(Slot::RightStatus, &ui, &status, None, 32).len(),
            1
        );
        assert_eq!(
            project_status(Slot::BelowStatus, &ui, &status, None, 32).len(),
            1
        );
        assert_eq!(
            project_status(Slot::AboveComposer, &ui, &status, None, 32).len(),
            0
        );
        assert_eq!(
            project_status(Slot::LeftStatus, &ui, &status, None, 0).len(),
            0
        );
        let left = project_status(Slot::LeftStatus, &ui, &status, None, 32);
        assert_eq!(left[0].spans[0].content, "agent original");
        assert_eq!(left[0].width(), 32);
        let right = project_status(Slot::RightStatus, &ui, &status, None, 32);
        assert_eq!(right[0].spans[0].content, "main");
        assert_eq!(right[0].width(), 32);
    }
    #[test]
    fn aw_p6_8b_status_sanitizes_terminal_controls_and_multiline_user_titles() {
        let ui=UiConfig{status_lines:vec![StatusLineConfig{
            region:StatusRegion::LeftStatus,items:vec![StatusItem::Agent],right_items:vec![],
        }],..UiConfig::default()};
        let ctx=StatusContext{agent:"\x1b[31mOwner\nnext\u{202e}".into(),..StatusContext::default()};
        let projected=project_status(Slot::LeftStatus,&ui,&ctx,None,32);
        let text=projected[0].spans.iter().map(|s|s.content.as_ref()).collect::<String>();
        assert!(!text.contains('\x1b') && !text.contains('\n') && !text.contains('\u{202e}'));
        assert!(text.contains("\\u{1b}"),"terminal control must be rendered as literal text");
    }

}
