//! Transport-neutral MCP projection helpers.
//!
//! This crate owns only MCP model shaping: bounded result metadata, receipt
//! timestamps, text-only serialized fitting, and advisory progress projection.
//! It starts no transport, performs no I/O beyond caller-supplied async send
//! futures, reads no wall clock, and depends on no Agent runtime.

#![forbid(unsafe_code)]

use aw_tools::{budget::SerializedResultBytes, receipt_policy::ReceiptProfile, OperationId};
use rmcp::model::{
    CallToolResult, ContentBlock, MetaObject, ProgressNotificationParam, ProgressToken,
};
use serde_json::Value;
use std::{fmt, future::Future, sync::Arc, time::Duration};
use tokio::time::{sleep_until, Instant};

pub const MAX_MCP_RESULT_BLOCKS: usize = 32;
pub const MAX_MCP_BLOCK_METADATA_BYTES: usize = 16 * 1024;
pub const MAX_MCP_RESULT_META_BYTES: usize = 16 * 1024;
pub const RECEIPT_META_KEY: &str = "io.abird/receipt";
const FIT_TRUNCATION_SUFFIX: &str = "\n… truncated to fit the serialized MCP result budget";

pub const DEFAULT_PROGRESS_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);
pub const DEFAULT_PROGRESS_SEND_BUDGET: Duration = Duration::from_millis(100);
const MAX_SEMANTIC_PROGRESS_MESSAGE_BYTES: usize = 64;
const MAX_EXACT_F64_INTEGER: u64 = 1u64 << 53;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Config(&'static str),
    Limit(&'static str),
    Protocol(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(message) | Self::Limit(message) | Self::Protocol(message) => {
                f.write_str(message)
            }
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = core::result::Result<T, Error>;

/// Host-tunable heartbeat timings. Both values must be positive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProgressHeartbeatConfig {
    interval: Duration,
    send_budget: Duration,
}

impl ProgressHeartbeatConfig {
    pub fn new(interval: Duration, send_budget: Duration) -> Result<Self> {
        if interval.is_zero() || send_budget.is_zero() {
            return Err(Error::Config(
                "MCP progress heartbeat interval and send budget must be positive",
            ));
        }
        Ok(Self {
            interval,
            send_budget,
        })
    }

    pub const fn interval(self) -> Duration {
        self.interval
    }

    pub const fn send_budget(self) -> Duration {
        self.send_budget
    }
}

impl Default for ProgressHeartbeatConfig {
    fn default() -> Self {
        Self {
            interval: DEFAULT_PROGRESS_HEARTBEAT_INTERVAL,
            send_budget: DEFAULT_PROGRESS_SEND_BUDGET,
        }
    }
}

/// Advisory heartbeat delivery counts for one wait.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProgressHeartbeatSummary {
    pub attempted: u64,
    pub delivered: u64,
}

/// Optional product/host observer. The shared adapter emits only counts.
pub trait ProgressHeartbeatObserver: Send + Sync {
    fn completed(&self, summary: ProgressHeartbeatSummary);
}

/// MCP heartbeat context for one request that actually carried a progress token.
pub struct ProgressHeartbeat {
    token: ProgressToken,
    config: ProgressHeartbeatConfig,
    observer: Option<Arc<dyn ProgressHeartbeatObserver>>,
}

impl ProgressHeartbeat {
    pub fn new(
        token: ProgressToken,
        config: ProgressHeartbeatConfig,
        observer: Option<Arc<dyn ProgressHeartbeatObserver>>,
    ) -> Self {
        Self {
            token,
            config,
            observer,
        }
    }

    pub fn from_token(
        token: Option<ProgressToken>,
        config: ProgressHeartbeatConfig,
        observer: Option<Arc<dyn ProgressHeartbeatObserver>>,
    ) -> Option<Self> {
        token.map(|token| Self::new(token, config, observer))
    }
}

struct ProgressObserverGuard {
    observer: Option<Arc<dyn ProgressHeartbeatObserver>>,
    summary: ProgressHeartbeatSummary,
}

impl ProgressObserverGuard {
    fn new(observer: Option<Arc<dyn ProgressHeartbeatObserver>>) -> Self {
        Self {
            observer,
            summary: ProgressHeartbeatSummary::default(),
        }
    }
}

impl Drop for ProgressObserverGuard {
    fn drop(&mut self) {
        if let Some(observer) = &self.observer {
            observer.completed(self.summary);
        }
    }
}

/// Adapt transport-independent semantic progress into one MCP notification.
pub fn semantic_progress_notification(
    token: ProgressToken,
    completed: u64,
    end: aw_tools::filesystem::progress::End,
    message: Option<&str>,
) -> Result<ProgressNotificationParam> {
    if completed > MAX_EXACT_F64_INTEGER {
        return Err(Error::Limit(
            "MCP progress counter exceeds exact numeric representation",
        ));
    }
    let total = match end {
        aw_tools::filesystem::progress::End::Known(total) => {
            if total == 0 || total > MAX_EXACT_F64_INTEGER || completed > total {
                return Err(Error::Protocol("invalid semantic MCP progress total"));
            }
            Some(total as f64)
        }
        aw_tools::filesystem::progress::End::Unknown => None,
    };
    let message = match message {
        Some(message)
            if message.len() <= MAX_SEMANTIC_PROGRESS_MESSAGE_BYTES
                && !message.chars().any(char::is_control) =>
        {
            Some(message.to_owned())
        }
        Some(_) => return Err(Error::Protocol("invalid semantic MCP progress message")),
        None => None,
    };
    let mut param = ProgressNotificationParam::new(token, completed as f64);
    param.total = total;
    param.message = message;
    Ok(param)
}

fn heartbeat_notification(token: ProgressToken, tick: u64) -> ProgressNotificationParam {
    ProgressNotificationParam::new(token, tick as f64)
}

async fn await_with_heartbeat_sender<F, S, SF>(
    token: Option<ProgressToken>,
    config: ProgressHeartbeatConfig,
    deadline: Option<Instant>,
    observer: Option<Arc<dyn ProgressHeartbeatObserver>>,
    wait: F,
    mut send: S,
) -> F::Output
where
    F: Future,
    S: FnMut(ProgressNotificationParam) -> SF,
    SF: Future<Output = bool>,
{
    let Some(token) = token else {
        return wait.await;
    };
    tokio::pin!(wait);
    let mut report = ProgressObserverGuard::new(observer);

    loop {
        let tick_at = Instant::now() + config.interval;
        tokio::select! {
            biased;
            output = &mut wait => return output,
            () = sleep_until(tick_at) => {}
        }

        if report.summary.attempted >= MAX_EXACT_F64_INTEGER {
            continue;
        }
        report.summary.attempted = report.summary.attempted.saturating_add(1);
        let param = heartbeat_notification(token.clone(), report.summary.attempted);

        let now = Instant::now();
        let send_bound = now + config.send_budget;
        let send_deadline = deadline.map_or(send_bound, |deadline| deadline.min(send_bound));
        if send_deadline <= now {
            continue;
        }

        let delivery = send(param);
        tokio::pin!(delivery);
        tokio::select! {
            biased;
            output = &mut wait => return output,
            delivered = &mut delivery => {
                if delivered {
                    report.summary.delivered = report.summary.delivered.saturating_add(1);
                }
            }
            () = sleep_until(send_deadline) => {}
        }
    }
}

/// Await one operation while emitting bare unknown-total MCP heartbeat ticks.
pub async fn await_with_progress_heartbeat<F, S, SF>(
    heartbeat: Option<ProgressHeartbeat>,
    deadline: Option<Instant>,
    wait: F,
    send: S,
) -> F::Output
where
    F: Future,
    S: FnMut(ProgressNotificationParam) -> SF,
    SF: Future<Output = bool>,
{
    let Some(heartbeat) = heartbeat else {
        return wait.await;
    };
    await_with_heartbeat_sender(
        Some(heartbeat.token),
        heartbeat.config,
        deadline,
        heartbeat.observer,
        wait,
        send,
    )
    .await
}

/// Send one real semantic progress update with the same bounded delivery rule.
pub async fn notify_semantic_progress_bounded<S, SF>(
    token: ProgressToken,
    completed: u64,
    end: aw_tools::filesystem::progress::End,
    message: Option<&str>,
    send_budget: Duration,
    deadline: Option<Instant>,
    mut send: S,
) -> Result<bool>
where
    S: FnMut(ProgressNotificationParam) -> SF,
    SF: Future<Output = bool>,
{
    if send_budget.is_zero() {
        return Err(Error::Config(
            "MCP semantic progress send budget must be positive",
        ));
    }
    let param = semantic_progress_notification(token, completed, end, message)?;
    let now = Instant::now();
    let bound = now + send_budget;
    let send_deadline = deadline.map_or(bound, |deadline| deadline.min(bound));
    if send_deadline <= now {
        return Ok(false);
    }
    tokio::select! {
        biased;
        () = sleep_until(send_deadline) => Ok(false),
        delivered = send(param) => Ok(delivered),
    }
}

pub fn serialized_result_len(result: &CallToolResult) -> Result<usize> {
    serde_json::to_vec(result)
        .map(|bytes| bytes.len())
        .map_err(|_| Error::Protocol("invalid MCP tool result"))
}

fn validate_result_meta(meta: Option<&MetaObject>) -> Result<()> {
    let Some(meta) = meta else {
        return Ok(());
    };
    let bytes = serde_json::to_vec(meta)
        .map_err(|_| Error::Protocol("invalid MCP result metadata"))?
        .len();
    if bytes > MAX_MCP_RESULT_META_BYTES {
        return Err(Error::Limit("MCP result metadata exceeds 16 KiB"));
    }
    Ok(())
}

fn content_block_payload_len(block: &ContentBlock) -> Result<usize> {
    let primary =
        match block {
            ContentBlock::Text(text) => text.text.len(),
            ContentBlock::Image(image) => image.data.len().saturating_add(image.mime_type.len()),
            ContentBlock::Audio(audio) => audio.data.len().saturating_add(audio.mime_type.len()),
            ContentBlock::Resource(resource) => serde_json::to_vec(resource)
                .map(|bytes| bytes.len())
                .map_err(|_| Error::Protocol("invalid embedded MCP resource result"))?,
            ContentBlock::ResourceLink(resource) => serde_json::to_vec(resource)
                .map(|bytes| bytes.len())
                .map_err(|_| Error::Protocol("invalid MCP resource-link result"))?,
            _ => return Err(Error::Protocol("unsupported MCP tool result content block")),
        };
    let encoded = serde_json::to_vec(block)
        .map_err(|_| Error::Protocol("invalid MCP tool result content block"))?
        .len();
    if encoded > primary.saturating_add(MAX_MCP_BLOCK_METADATA_BYTES) {
        return Err(Error::Limit("MCP content-block metadata exceeds 16 KiB"));
    }
    Ok(primary)
}

pub fn validate_complete_result(result: &CallToolResult) -> Result<()> {
    if result.content.len() > MAX_MCP_RESULT_BLOCKS {
        return Err(Error::Limit("too many MCP tool result content blocks"));
    }
    validate_result_meta(result.meta.as_ref())?;
    for block in &result.content {
        let _ = content_block_payload_len(block)?;
    }
    if let Some(value) = &result.structured_content {
        serde_json::to_vec(value).map_err(|_| Error::Protocol("invalid structured MCP result"))?;
    }
    Ok(())
}

pub fn stored_receipt_timestamp(result: &CallToolResult) -> Option<u64> {
    result
        .meta
        .as_ref()
        .and_then(|meta| meta.0.get(RECEIPT_META_KEY))
        .and_then(Value::as_object)
        .and_then(|receipt| receipt.get("ts"))
        .and_then(Value::as_u64)
}

pub fn has_visible_receipt_timestamp(result: &CallToolResult, ts: u64) -> bool {
    let marker = format!("ts={ts}");
    result.content.iter().any(|block| {
        block
            .as_text()
            .is_some_and(|text| text.text.lines().any(|line| line.trim() == marker))
    })
}

pub fn set_stored_receipt_timestamp(result: &mut CallToolResult, ts: u64) -> Result<()> {
    let mut meta = result.meta.clone().unwrap_or_default();
    let receipt = meta
        .0
        .entry(RECEIPT_META_KEY.to_owned())
        .or_insert_with(|| serde_json::json!({}));
    let object = receipt.as_object_mut().ok_or(Error::Protocol(
        "reserved receipt metadata must be an object",
    ))?;
    object.insert("ts".to_owned(), Value::from(ts));
    validate_result_meta(Some(&meta))?;
    result.meta = Some(meta);
    Ok(())
}

fn total_text_bytes(result: &CallToolResult) -> usize {
    result
        .content
        .iter()
        .filter_map(ContentBlock::as_text)
        .map(|text| text.text.len())
        .sum()
}

fn fitted_text_candidate(
    original: &CallToolResult,
    keep_bytes: usize,
    suffix: &str,
) -> CallToolResult {
    let total = total_text_bytes(original);
    if keep_bytes >= total {
        return original.clone();
    }

    let mut fitted = original.clone();
    let mut remaining = keep_bytes;
    let mut truncated = false;
    for block in &mut fitted.content {
        let ContentBlock::Text(text) = block else {
            continue;
        };
        if truncated {
            text.text.clear();
            continue;
        }
        if remaining >= text.text.len() {
            remaining -= text.text.len();
            continue;
        }
        let mut end = remaining.min(text.text.len());
        while end > 0 && !text.text.is_char_boundary(end) {
            end -= 1;
        }
        text.text.truncate(end);
        text.text.push_str(suffix);
        remaining = 0;
        truncated = true;
    }
    fitted
}

/// Fit one complete MCP tool result without changing typed/structured facts.
pub fn fit_complete_result(
    result: CallToolResult,
    ceiling: SerializedResultBytes,
) -> Result<CallToolResult> {
    validate_complete_result(&result)?;
    let budget = ceiling.get();
    if serialized_result_len(&result)? <= budget {
        return Ok(result);
    }

    let total_text = total_text_bytes(&result);
    if total_text == 0 {
        return Err(Error::Limit(
            "immutable MCP result cannot fit serialized result ceiling",
        ));
    }

    let minimal = fitted_text_candidate(&result, 0, FIT_TRUNCATION_SUFFIX);
    if serialized_result_len(&minimal)? > budget {
        return Err(Error::Limit(
            "minimal factual MCP result cannot fit serialized result ceiling",
        ));
    }

    let mut low = 0usize;
    let mut high = total_text.saturating_sub(1);
    let mut best = minimal;
    while low <= high {
        let mid = low + (high - low) / 2;
        let candidate = fitted_text_candidate(&result, mid, FIT_TRUNCATION_SUFFIX);
        if serialized_result_len(&candidate)? <= budget {
            best = candidate;
            low = mid.saturating_add(1);
        } else if mid == 0 {
            break;
        } else {
            high = mid - 1;
        }
    }
    Ok(best)
}

#[derive(Clone, Copy, Debug)]
enum ReceiptMarkerPlacement {
    NewBlock,
    ExistingText { index: usize, original_len: usize },
}

fn append_receipt_marker(
    result: &mut CallToolResult,
    marker: &str,
) -> Result<ReceiptMarkerPlacement> {
    if result.content.len() < MAX_MCP_RESULT_BLOCKS {
        result.content.push(ContentBlock::text(marker.to_owned()));
        return Ok(ReceiptMarkerPlacement::NewBlock);
    }

    let Some((index, text)) =
        result
            .content
            .iter_mut()
            .enumerate()
            .rev()
            .find_map(|(index, block)| match block {
                ContentBlock::Text(text) => Some((index, text)),
                _ => None,
            })
    else {
        return Err(Error::Limit(
            "receipt timestamp cannot fit the MCP content-block limit",
        ));
    };

    let original_len = text.text.len();
    if !text.text.is_empty() && !text.text.ends_with('\n') {
        text.text.push('\n');
    }
    text.text.push_str(marker);
    Ok(ReceiptMarkerPlacement::ExistingText {
        index,
        original_len,
    })
}

fn rollback_receipt_marker(result: &mut CallToolResult, placement: ReceiptMarkerPlacement) {
    match placement {
        ReceiptMarkerPlacement::NewBlock => {
            let _ = result.content.pop();
        }
        ReceiptMarkerPlacement::ExistingText {
            index,
            original_len,
        } => {
            if let Some(ContentBlock::Text(text)) = result.content.get_mut(index) {
                text.text.truncate(original_len);
            }
        }
    }
}

/// Apply shared receipt policy and fit one complete result.
///
/// Stored receipt time is immutable replay state. The body is first fitted
/// against the full envelope; an extra visible sidecar is reserved only when
/// the fitted body no longer carries the exact timestamp line.
pub fn decorate_and_fit_complete_result(
    receipt_profile: ReceiptProfile,
    operation: OperationId,
    mut result: CallToolResult,
    host_timestamp: Option<u64>,
    ceiling: SerializedResultBytes,
) -> Result<CallToolResult> {
    validate_complete_result(&result)?;
    if !receipt_profile.timestamps(operation) {
        return fit_complete_result(result, ceiling);
    }

    let ts = stored_receipt_timestamp(&result)
        .or(host_timestamp)
        .ok_or(Error::Config("timestamped receipt requires host timestamp"))?;
    set_stored_receipt_timestamp(&mut result, ts)?;

    let marker = format!("ts={ts}");
    let mut fitted = fit_complete_result(result, ceiling)?;
    if has_visible_receipt_timestamp(&fitted, ts) {
        return Ok(fitted);
    }

    let fitted_size = serialized_result_len(&fitted)?;
    let placement = append_receipt_marker(&mut fitted, &marker)?;
    let candidate_size = serialized_result_len(&fitted)?;
    if candidate_size <= ceiling.get() {
        return Ok(fitted);
    }

    rollback_receipt_marker(&mut fitted, placement);
    let reserve = candidate_size.saturating_sub(fitted_size);
    let body_budget = ceiling
        .get()
        .checked_sub(reserve)
        .and_then(SerializedResultBytes::new)
        .ok_or(Error::Limit(
            "receipt timestamp cannot fit serialized result ceiling",
        ))?;
    let mut body = fit_complete_result(fitted, body_budget)?;
    let _ = append_receipt_marker(&mut body, &marker)?;
    if serialized_result_len(&body)? > ceiling.get() {
        return Err(Error::Limit(
            "receipt timestamp cannot fit serialized result ceiling",
        ));
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn test_progress_token(value: &str) -> ProgressToken {
        ProgressToken(rmcp::model::NumberOrString::String(value.into()))
    }

    #[derive(Default)]
    struct RecordingProgressObserver(std::sync::Mutex<Vec<ProgressHeartbeatSummary>>);

    impl ProgressHeartbeatObserver for RecordingProgressObserver {
        fn completed(&self, summary: ProgressHeartbeatSummary) {
            self.0.lock().unwrap().push(summary);
        }
    }

    #[test]
    fn complete_result_fitter_only_shrinks_text_and_preserves_typed_facts() {
        let image = ContentBlock::image("aW1hZ2U=", "image/png");
        let structured = json!({"kind":"page","offset":7});
        let mut meta = MetaObject::new();
        meta.0.insert("keep".into(), json!("yes"));

        let mut original =
            CallToolResult::success(vec![ContentBlock::text("雪".repeat(2048)), image.clone()]);
        original.structured_content = Some(structured.clone());
        original.meta = Some(meta.clone());

        let minimal = fitted_text_candidate(&original, 0, FIT_TRUNCATION_SUFFIX);
        let budget = serialized_result_len(&minimal).unwrap() + 96;
        let fitted = fit_complete_result(
            original.clone(),
            SerializedResultBytes::new(budget).unwrap(),
        )
        .unwrap();

        assert!(serialized_result_len(&fitted).unwrap() <= budget);
        assert_eq!(fitted.content[1], image);
        assert_eq!(fitted.structured_content, Some(structured));
        assert_eq!(fitted.meta, Some(meta));
        assert_eq!(fitted.is_error, original.is_error);
        assert_eq!(fitted.result_type, original.result_type);
        assert!(fitted.content[0]
            .as_text()
            .unwrap()
            .text
            .ends_with(FIT_TRUNCATION_SUFFIX));
    }

    #[test]
    fn immutable_typed_or_structured_result_fails_instead_of_changing_identity() {
        let media =
            CallToolResult::success(vec![ContentBlock::image("x".repeat(2048), "image/png")]);
        let media_size = serialized_result_len(&media).unwrap();
        assert!(
            fit_complete_result(media, SerializedResultBytes::new(media_size - 1).unwrap())
                .is_err()
        );

        let mut structured = CallToolResult::success(Vec::new());
        structured.structured_content = Some(json!({"payload":"x".repeat(2048)}));
        let size = serialized_result_len(&structured).unwrap();
        assert!(
            fit_complete_result(structured, SerializedResultBytes::new(size - 1).unwrap()).is_err()
        );
    }

    #[test]
    fn receipt_metadata_merge_is_transactional_and_preserves_reserved_fields() {
        let mut result = CallToolResult::success(vec![ContentBlock::text("ok")]);
        let mut meta = MetaObject::new();
        meta.0
            .insert(RECEIPT_META_KEY.into(), json!({"kind":"job"}));
        result.meta = Some(meta);
        set_stored_receipt_timestamp(&mut result, 77).unwrap();
        let receipt = result.meta.as_ref().unwrap().0[RECEIPT_META_KEY]
            .as_object()
            .unwrap();
        assert_eq!(receipt["kind"], "job");
        assert_eq!(receipt["ts"], 77);

        let mut invalid = CallToolResult::success(vec![ContentBlock::text("ok")]);
        let mut meta = MetaObject::new();
        meta.0.insert(RECEIPT_META_KEY.into(), json!("invalid"));
        invalid.meta = Some(meta);
        let before = invalid.meta.clone();
        assert!(set_stored_receipt_timestamp(&mut invalid, 1).is_err());
        assert_eq!(invalid.meta, before);
    }

    #[test]
    fn visible_receipt_that_survives_full_budget_fit_needs_no_sidecar_reserve() {
        let profile = ReceiptProfile {
            unix_ts: true,
            ..Default::default()
        };
        let mut result = CallToolResult::success(vec![
            ContentBlock::text("ts=77\nkept"),
            ContentBlock::text("x".repeat(4096)),
        ]);
        set_stored_receipt_timestamp(&mut result, 77).unwrap();

        let minimal = fitted_text_candidate(&result, 16, FIT_TRUNCATION_SUFFIX);
        let budget = serialized_result_len(&minimal).unwrap() + 64;
        let expected =
            fit_complete_result(result.clone(), SerializedResultBytes::new(budget).unwrap())
                .unwrap();
        assert!(has_visible_receipt_timestamp(&expected, 77));

        let decorated = decorate_and_fit_complete_result(
            profile,
            OperationId::JobsGet,
            result,
            Some(999),
            SerializedResultBytes::new(budget).unwrap(),
        )
        .unwrap();
        assert_eq!(decorated, expected);
        assert_eq!(decorated.content.len(), 2);
        assert!(has_visible_receipt_timestamp(&decorated, 77));
        assert!(!has_visible_receipt_timestamp(&decorated, 999));
    }

    #[test]
    fn receipt_sidecar_is_atomic_under_tight_result_budgets() {
        let profile = ReceiptProfile {
            unix_ts: true,
            ..Default::default()
        };
        let long = CallToolResult::success(vec![ContentBlock::text("x".repeat(4096))]);
        let tight = decorate_and_fit_complete_result(
            profile,
            OperationId::JobsGet,
            long,
            Some(1_794_737_000),
            SerializedResultBytes::new(512).unwrap(),
        )
        .unwrap();
        assert!(serialized_result_len(&tight).unwrap() <= 512);
        assert_eq!(stored_receipt_timestamp(&tight), Some(1_794_737_000));
        assert!(has_visible_receipt_timestamp(&tight, 1_794_737_000));
    }

    #[test]
    fn receipt_visibility_respects_the_content_block_ceiling() {
        let profile = ReceiptProfile {
            unix_ts: true,
            ..Default::default()
        };
        let ceiling = SerializedResultBytes::new(64 * 1024).unwrap();

        let text_blocks = CallToolResult::success(
            (0..MAX_MCP_RESULT_BLOCKS)
                .map(|_| ContentBlock::text("x"))
                .collect(),
        );
        let text_blocks = decorate_and_fit_complete_result(
            profile,
            OperationId::JobsGet,
            text_blocks,
            Some(88),
            ceiling,
        )
        .unwrap();
        assert_eq!(text_blocks.content.len(), MAX_MCP_RESULT_BLOCKS);
        assert!(has_visible_receipt_timestamp(&text_blocks, 88));

        let typed_blocks = CallToolResult::success(
            (0..MAX_MCP_RESULT_BLOCKS)
                .map(|_| ContentBlock::image("eA==", "image/png"))
                .collect(),
        );
        assert!(matches!(
            decorate_and_fit_complete_result(
                profile,
                OperationId::JobsGet,
                typed_blocks,
                Some(89),
                ceiling,
            ),
            Err(Error::Limit(_))
        ));
    }

    #[test]
    fn progress_heartbeat_config_and_semantic_projection_are_strict() {
        assert!(ProgressHeartbeatConfig::new(Duration::ZERO, Duration::from_millis(1)).is_err());
        assert!(ProgressHeartbeatConfig::new(Duration::from_millis(1), Duration::ZERO).is_err());

        let token = test_progress_token("semantic");
        let known = semantic_progress_notification(
            token.clone(),
            4,
            aw_tools::filesystem::progress::End::Known(10),
            Some("scan"),
        )
        .unwrap();
        assert_eq!(known.progress_token, token);
        assert_eq!(known.progress, 4.0);
        assert_eq!(known.total, Some(10.0));
        assert_eq!(known.message.as_deref(), Some("scan"));
        assert!(semantic_progress_notification(
            test_progress_token("bad-total"),
            11,
            aw_tools::filesystem::progress::End::Known(10),
            None,
        )
        .is_err());
    }

    #[tokio::test]
    async fn heartbeat_without_token_is_exact_wait_and_never_polls_sender() {
        let polled = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let send_polled = polled.clone();
        let output = await_with_progress_heartbeat(None, None, async { 42_u32 }, move |_| {
            send_polled.store(true, std::sync::atomic::Ordering::Relaxed);
            std::future::ready(true)
        })
        .await;
        assert_eq!(output, 42);
        assert!(!polled.load(std::sync::atomic::Ordering::Relaxed));
    }

    #[tokio::test]
    async fn stalled_heartbeat_delivery_never_delays_underlying_wait() {
        let config =
            ProgressHeartbeatConfig::new(Duration::from_millis(1), Duration::from_secs(1)).unwrap();
        let observer = Arc::new(RecordingProgressObserver::default());
        let heartbeat = ProgressHeartbeat::new(
            test_progress_token("stalled"),
            config,
            Some(observer.clone()),
        );

        let output = tokio::time::timeout(
            Duration::from_millis(100),
            await_with_progress_heartbeat(
                Some(heartbeat),
                None,
                async {
                    tokio::time::sleep(Duration::from_millis(12)).await;
                    7_u8
                },
                |_| std::future::pending::<bool>(),
            ),
        )
        .await
        .expect("stalled progress send delayed the underlying wait");
        assert_eq!(output, 7);
        assert_eq!(
            observer.0.lock().unwrap().as_slice(),
            &[ProgressHeartbeatSummary {
                attempted: 1,
                delivered: 0,
            }]
        );
    }

    #[tokio::test]
    async fn heartbeat_ticks_are_monotonic_and_stop_with_wait() {
        let config =
            ProgressHeartbeatConfig::new(Duration::from_millis(2), Duration::from_millis(20))
                .unwrap();
        let observer = Arc::new(RecordingProgressObserver::default());
        let heartbeat =
            ProgressHeartbeat::new(test_progress_token("beat"), config, Some(observer.clone()));
        let sent = Arc::new(std::sync::Mutex::new(
            Vec::<ProgressNotificationParam>::new(),
        ));
        let sent_for_send = sent.clone();
        let output = await_with_progress_heartbeat(
            Some(heartbeat),
            None,
            async {
                tokio::time::sleep(Duration::from_millis(14)).await;
                9_u8
            },
            move |param| {
                sent_for_send.lock().unwrap().push(param);
                std::future::ready(true)
            },
        )
        .await;
        assert_eq!(output, 9);
        let params = sent.lock().unwrap().clone();
        assert!(params.len() >= 2);
        for (index, param) in params.iter().enumerate() {
            assert_eq!(param.progress, (index + 1) as f64);
            assert_eq!(param.total, None);
            assert_eq!(param.message, None);
        }
        let summary = observer.0.lock().unwrap()[0];
        assert_eq!(summary.attempted, params.len() as u64);
        assert_eq!(summary.delivered, params.len() as u64);
        let count = params.len();
        tokio::time::sleep(Duration::from_millis(8)).await;
        assert_eq!(sent.lock().unwrap().len(), count);
    }

    #[tokio::test]
    async fn heartbeat_send_budget_is_independent_and_expired_deadline_skips_sender() {
        let config =
            ProgressHeartbeatConfig::new(Duration::from_millis(1), Duration::from_millis(3))
                .unwrap();
        let observer = Arc::new(RecordingProgressObserver::default());
        let heartbeat = ProgressHeartbeat::new(
            test_progress_token("bounded"),
            config,
            Some(observer.clone()),
        );
        let output = tokio::time::timeout(
            Duration::from_millis(100),
            await_with_progress_heartbeat(
                Some(heartbeat),
                None,
                async {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    5_u8
                },
                |_| std::future::pending::<bool>(),
            ),
        )
        .await
        .unwrap();
        assert_eq!(output, 5);
        assert!(observer.0.lock().unwrap()[0].attempted >= 2);

        let polled = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let send_polled = polled.clone();
        let heartbeat = ProgressHeartbeat::new(
            test_progress_token("expired"),
            ProgressHeartbeatConfig::new(Duration::from_millis(1), Duration::from_secs(1)).unwrap(),
            None,
        );
        let output = await_with_progress_heartbeat(
            Some(heartbeat),
            Some(Instant::now() - Duration::from_millis(1)),
            async {
                tokio::time::sleep(Duration::from_millis(5)).await;
                6_u8
            },
            move |_| {
                send_polled.store(true, std::sync::atomic::Ordering::Relaxed);
                std::future::ready(true)
            },
        )
        .await;
        assert_eq!(output, 6);
        assert!(!polled.load(std::sync::atomic::Ordering::Relaxed));
    }

    #[tokio::test]
    async fn cancelled_wrapper_reports_attempt_summary_and_semantic_send_is_bounded() {
        let observer = Arc::new(RecordingProgressObserver::default());
        let heartbeat = ProgressHeartbeat::new(
            test_progress_token("cancelled"),
            ProgressHeartbeatConfig::new(Duration::from_millis(1), Duration::from_secs(1)).unwrap(),
            Some(observer.clone()),
        );
        assert!(tokio::time::timeout(
            Duration::from_millis(10),
            await_with_progress_heartbeat(
                Some(heartbeat),
                None,
                std::future::pending::<()>(),
                |_| std::future::pending::<bool>(),
            ),
        )
        .await
        .is_err());
        let summary = observer.0.lock().unwrap()[0];
        assert_eq!(summary.attempted, 1);
        assert_eq!(summary.delivered, 0);

        let delivered = notify_semantic_progress_bounded(
            test_progress_token("semantic"),
            3,
            aw_tools::filesystem::progress::End::Known(8),
            Some("commit"),
            Duration::from_millis(20),
            None,
            |_| std::future::ready(true),
        )
        .await
        .unwrap();
        assert!(delivered);
        let timed_out = notify_semantic_progress_bounded(
            test_progress_token("slow"),
            4,
            aw_tools::filesystem::progress::End::Unknown,
            None,
            Duration::from_millis(2),
            None,
            |_| std::future::pending::<bool>(),
        )
        .await
        .unwrap();
        assert!(!timed_out);
    }
}
