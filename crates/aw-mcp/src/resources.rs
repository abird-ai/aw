//! Explicit, bounded MCP data discovery/read and finite notification observation.
//! MCP content is untrusted data: it never becomes an Agent instruction or a tool grant.
use super::{connect, label, Connection, Options, CONNECT_TIMEOUT};
use aw_agent_core::{Error, Result};
use rmcp::{
    model::{
        ContentBlock, GetPromptRequestParams, GetPromptResponse, PaginatedRequestParams, Prompt,
        ProtocolVersion, ReadResourceRequestParams, ReadResourceResponse, Resource,
        ResourceContents, ResourceTemplate, ServerNotification, SubscriptionFilter,
    },
    service::{NotificationContext, RoleClient},
    ClientHandler,
};
use serde::Serialize;
use std::{
    collections::BTreeSet,
    future::{self, Future},
    sync::atomic::{AtomicU64, Ordering},
    sync::Arc,
    time::Duration,
};
use tokio::sync::broadcast;

const MAX_ENTRIES: usize = 128;
const MAX_PAGES: usize = 8;
const MAX_DESCRIPTOR: usize = 16 * 1024;
const MAX_CONTENT: usize = 128 * 1024;
const MAX_MESSAGES: usize = 32;

#[derive(Clone, Debug, clap::Subcommand)]
pub enum DataAction {
    /// List bounded MCP prompt templates; does not fetch or admit any prompt.
    Prompts,
    /// List advertised resource URIs; does not read their contents.
    Resources,
    /// List advertised URI templates; does not expand or read them.
    Templates,
    /// Explicitly retrieve one advertised prompt, with validated KEY=VALUE arguments.
    Prompt {
        server: String,
        name: String,
        #[arg(value_name = "KEY=VALUE")]
        args: Vec<String>,
    },
    /// Explicitly read one currently advertised text resource by exact URI.
    Read { server: String, uri: String },
    /// Expand a currently advertised template, then explicitly read its text result.
    ReadTemplate {
        server: String,
        name: String,
        #[arg(value_name = "KEY=VALUE")]
        args: Vec<String>,
    },
    /// Observe negotiated changes for a bounded interval; never fetch or run anything.
    Watch {
        server: String,
        /// Explicit resource URI to subscribe to, if the server supports it.
        #[arg(long)]
        resource: Option<String>,
        /// Maximum watch duration, from 1 through 30 seconds.
        #[arg(long, default_value_t = 3, value_parser = clap::value_parser!(u64).range(1..=30))]
        seconds: u64,
    },
}

#[derive(Clone, Debug, Serialize)]
pub struct DataArgument {
    pub name: String,
    pub required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct DataRow {
    pub server: String,
    pub kind: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Vec<DataArgument>>,
}
impl DataRow {
    fn new(server: &str, kind: &str, name: &str) -> Self {
        Self {
            server: server.into(),
            kind: kind.into(),
            name: name.into(),
            uri: None,
            description: None,
            text: None,
            role: None,
            arguments: None,
        }
    }
}

/// Revisions are connection-scoped, never derived from untrusted server claims.
#[derive(Default)]
pub(super) struct Revisions {
    pub tools: AtomicU64,
    pub prompts: AtomicU64,
    pub resources: AtomicU64,
}
impl Revisions {
    fn invalidate_all(&self) {
        // Once a change stream closes, it can no longer guarantee that a
        // registered capability is current. Fail closed, without reconnect.
        self.tools.fetch_add(1, Ordering::AcqRel);
        self.prompts.fetch_add(1, Ordering::AcqRel);
        self.resources.fetch_add(1, Ordering::AcqRel);
    }
    fn invalidate(&self, what: &Changed) {
        let slot = match what {
            Changed::Tools => &self.tools,
            Changed::Prompts => &self.prompts,
            Changed::Resources | Changed::Resource(_) => &self.resources,
        };
        slot.fetch_add(1, Ordering::AcqRel);
    }
}
#[derive(Clone, Debug)]
pub(crate) enum Changed {
    Prompts,
    Resources,
    Tools,
    Resource(String),
}
impl Changed {
    fn row(self, server: &str) -> DataRow {
        let (name, uri) = match self {
            Self::Prompts => ("prompts/list_changed", None),
            Self::Resources => ("resources/list_changed", None),
            Self::Tools => ("tools/list_changed", None),
            Self::Resource(uri) => ("resources/updated", Some(uri)),
        };
        let mut row = DataRow::new(server, "changed", name);
        row.uri = uri;
        row
    }
}

pub(crate) struct NotificationHandler {
    config: rmcp::model::ClientConfig,
    changed: broadcast::Sender<Changed>,
    revisions: Arc<Revisions>,
}
impl NotificationHandler {
    pub(crate) fn new(
        config: rmcp::model::ClientConfig,
        changed: broadcast::Sender<Changed>,
        revisions: Arc<Revisions>,
    ) -> Self {
        Self {
            config,
            changed,
            revisions,
        }
    }
}
impl ClientHandler for NotificationHandler {
    fn get_info(&self) -> rmcp::model::ClientConfig {
        self.config.clone()
    }
    fn on_resource_updated(
        &self,
        params: rmcp::model::ResourceUpdatedNotificationParam,
        _: NotificationContext<RoleClient>,
    ) -> impl Future<Output = ()> + Send + '_ {
        if valid_uri(&params.uri).is_ok() {
            let notification = Changed::Resource(params.uri);
            self.revisions.invalidate(&notification);
            let _ = self.changed.send(notification);
        }
        future::ready(())
    }
    fn on_resource_list_changed(
        &self,
        _: NotificationContext<RoleClient>,
    ) -> impl Future<Output = ()> + Send + '_ {
        let notification = Changed::Resources;
        self.revisions.invalidate(&notification);
        let _ = self.changed.send(notification);
        future::ready(())
    }
    fn on_prompt_list_changed(
        &self,
        _: NotificationContext<RoleClient>,
    ) -> impl Future<Output = ()> + Send + '_ {
        let notification = Changed::Prompts;
        self.revisions.invalidate(&notification);
        let _ = self.changed.send(notification);
        future::ready(())
    }
    fn on_tool_list_changed(
        &self,
        _: NotificationContext<RoleClient>,
    ) -> impl Future<Output = ()> + Send + '_ {
        let notification = Changed::Tools;
        self.revisions.invalidate(&notification);
        let _ = self.changed.send(notification);
        future::ready(())
    }
}

pub(super) fn valid_uri(uri: &str) -> Result<()> {
    if uri.is_empty() || uri.len() > 2048 || uri.chars().any(char::is_control) {
        return Err(Error::Denied("MCP resource URI invalid or too long"));
    }
    let raw_lower = uri.to_ascii_lowercase();
    if ["%2e", "%2f", "%5c", "%00"]
        .iter()
        .any(|part| raw_lower.contains(part))
    {
        return Err(Error::Denied("MCP URI encoding/path escape denied"));
    }
    let raw_path = uri.split(['?', '#']).next().unwrap_or(uri);
    if raw_path.split('/').any(|part| part == "." || part == "..") || uri.contains('\\') {
        return Err(Error::Denied("MCP URI literal path traversal denied"));
    }
    let parsed = url::Url::parse(uri).map_err(|_| Error::Denied("MCP URI must be absolute"))?;
    if parsed.scheme().len() > 32 || parsed.username() != "" || parsed.password().is_some() {
        return Err(Error::Denied("MCP URI credentials are forbidden"));
    }
    // Refuse both literal and percent-encoded traversal/separators in URI paths.
    for segment in parsed.path().split('/') {
        let lower = segment.to_ascii_lowercase();
        if matches!(lower.as_str(), "." | "..")
            || lower.contains("%2e")
            || lower.contains("%2f")
            || lower.contains("%5c")
            || lower.contains("%00")
            || lower.contains('\\')
        {
            return Err(Error::Denied("MCP URI path escape denied"));
        }
    }
    Ok(())
}

fn valid_text(text: &str, max: usize) -> Result<()> {
    if text.len() > max || text.contains('\0') {
        return Err(Error::Limit("MCP untrusted text exceeds content budget"));
    }
    Ok(())
}

fn descriptor<T: Serialize>(value: &T) -> Result<()> {
    let len = serde_json::to_vec(value)
        .map_err(|_| Error::Protocol("invalid MCP descriptor"))?
        .len();
    if len > MAX_DESCRIPTOR {
        return Err(Error::Limit("MCP descriptor exceeds 16 KiB"));
    }
    Ok(())
}

fn cursor(
    next: Option<String>,
    page: usize,
    seen: &mut BTreeSet<String>,
) -> Result<Option<String>> {
    match next {
        None => Ok(None),
        Some(value)
            if page + 1 < MAX_PAGES
                && !value.is_empty()
                && value.len() <= 512
                && !value.chars().any(char::is_control)
                && seen.insert(value.clone()) =>
        {
            Ok(Some(value))
        }
        _ => Err(Error::Protocol("invalid/cyclic MCP page cursor")),
    }
}

fn arguments(input: &[String]) -> Result<std::collections::BTreeMap<String, String>> {
    if input.len() > 16 {
        return Err(Error::Limit("at most 16 MCP arguments"));
    }
    let mut result = std::collections::BTreeMap::new();
    for item in input {
        let (key, value) = item
            .split_once('=')
            .ok_or(Error::Denied("MCP argument must be KEY=VALUE"))?;
        if !label(key, 64)
            || value.len() > 2048
            || value.chars().any(char::is_control)
            || result.insert(key.into(), value.into()).is_some()
        {
            return Err(Error::Denied("invalid/duplicate MCP argument"));
        }
    }
    Ok(result)
}

fn prompt_argument_views(prompt: &Prompt) -> Result<Option<Vec<DataArgument>>> {
    let Some(items) = &prompt.arguments else {
        return Ok(None);
    };
    if items.len() > 16 {
        return Err(Error::Limit("MCP prompt has too many arguments"));
    }
    let mut names = BTreeSet::new();
    let mut fields = Vec::new();
    for item in items {
        if !label(&item.name, 64) || !names.insert(item.name.as_str()) {
            return Err(Error::Denied("MCP prompt argument descriptor invalid"));
        }
        fields.push(DataArgument {
            name: item.name.clone(),
            required: item.required == Some(true),
            description: item.description.clone(),
        });
    }
    Ok(Some(fields))
}

fn validate_prompt_args(
    prompt: &Prompt,
    values: &std::collections::BTreeMap<String, String>,
) -> Result<()> {
    let available = prompt.arguments.as_deref().unwrap_or(&[]);
    if available.len() > 16 {
        return Err(Error::Limit("MCP prompt has too many arguments"));
    }
    let mut names = BTreeSet::new();
    for arg in available {
        if !label(&arg.name, 64) || !names.insert(arg.name.as_str()) {
            return Err(Error::Denied("MCP prompt argument descriptor invalid"));
        }
        if arg.required == Some(true) && !values.contains_key(&arg.name) {
            return Err(Error::Denied("required MCP prompt argument missing"));
        }
    }
    if values.keys().any(|key| !names.contains(key.as_str())) {
        return Err(Error::Denied("MCP prompt argument not advertised"));
    }
    Ok(())
}

// Simple RFC6570 level-1 expansion only. Unsupported operators are denied, not
// interpreted with a partial or permissive parser.
fn expand_template(
    template: &str,
    values: &std::collections::BTreeMap<String, String>,
) -> Result<String> {
    valid_text(template, 2048)?;
    let mut output = String::new();
    let mut rest = template;
    let mut used = BTreeSet::new();
    while let Some(start) = rest.find('{') {
        output.push_str(&rest[..start]);
        let part = &rest[start + 1..];
        let end = part
            .find('}')
            .ok_or(Error::Denied("unterminated MCP URI template"))?;
        let name = &part[..end];
        if !label(name, 64) || !used.insert(name) {
            return Err(Error::Denied(
                "unsupported/duplicate MCP URI template variable",
            ));
        }
        let value = values
            .get(name)
            .ok_or(Error::Denied("MCP URI template variable missing"))?;
        let encoded = url::form_urlencoded::byte_serialize(value.as_bytes())
            .collect::<String>()
            .replace('+', "%20");
        output.push_str(&encoded);
        rest = &part[end + 1..];
    }
    output.push_str(rest);
    if output.contains('}') || used.len() != values.len() {
        return Err(Error::Denied("unknown MCP URI template variable"));
    }
    valid_uri(&output)?;
    Ok(output)
}

async fn prompts(conn: &Connection) -> Result<Vec<Prompt>> {
    tokio::time::timeout(CONNECT_TIMEOUT, async {
        let peer = conn.peer.lock().await;
        let initial_revision = conn.revisions.prompts.load(Ordering::Acquire);
        let mut rows = Vec::new();
        let mut seen = BTreeSet::new();
        let mut names = BTreeSet::new();
        let mut next = None;
        for page in 0..MAX_PAGES {
            let reply = peer
                .list_prompts(Some(PaginatedRequestParams::default().with_cursor(next)))
                .await
                .map_err(|_| Error::Transport("MCP prompts/list failed"))?;
            for prompt in reply.prompts {
                descriptor(&prompt)?;
                prompt_argument_views(&prompt)?;
                if !label(&prompt.name, 64)
                    || !names.insert(prompt.name.clone())
                    || rows.len() >= MAX_ENTRIES
                {
                    return Err(Error::Denied("invalid/duplicate/excess MCP prompt"));
                }
                rows.push(prompt);
            }
            next = cursor(reply.next_cursor, page, &mut seen)?;
            if next.is_none() {
                if conn.revisions.prompts.load(Ordering::Acquire) != initial_revision {
                    return Err(Error::Denied("MCP prompts changed while listing"));
                }
                return Ok(rows);
            }
        }
        Err(Error::Limit("too many MCP prompts pages"))
    })
    .await
    .map_err(|_| Error::Timeout)?
}
pub(super) async fn resources(conn: &Connection) -> Result<Vec<Resource>> {
    tokio::time::timeout(CONNECT_TIMEOUT, async {
        let peer = conn.peer.lock().await;
        let initial_revision = conn.revisions.resources.load(Ordering::Acquire);
        let mut rows = Vec::new();
        let mut seen = BTreeSet::new();
        let mut uris = BTreeSet::new();
        let mut next = None;
        for page in 0..MAX_PAGES {
            let reply = peer
                .list_resources(Some(PaginatedRequestParams::default().with_cursor(next)))
                .await
                .map_err(|_| Error::Transport("MCP resources/list failed"))?;
            for resource in reply.resources {
                descriptor(&resource)?;
                valid_uri(&resource.uri)?;
                if !label(&resource.name, 64)
                    || !uris.insert(resource.uri.clone())
                    || rows.len() >= MAX_ENTRIES
                {
                    return Err(Error::Denied("invalid/duplicate/excess MCP resource"));
                }
                rows.push(resource);
            }
            next = cursor(reply.next_cursor, page, &mut seen)?;
            if next.is_none() {
                if conn.revisions.resources.load(Ordering::Acquire) != initial_revision {
                    return Err(Error::Denied("MCP resources changed while listing"));
                }
                return Ok(rows);
            }
        }
        Err(Error::Limit("too many MCP resource pages"))
    })
    .await
    .map_err(|_| Error::Timeout)?
}
async fn templates(conn: &Connection) -> Result<Vec<ResourceTemplate>> {
    tokio::time::timeout(CONNECT_TIMEOUT, async {
        let peer = conn.peer.lock().await;
        let initial_revision = conn.revisions.resources.load(Ordering::Acquire);
        let mut rows = Vec::new();
        let mut seen = BTreeSet::new();
        let mut names = BTreeSet::new();
        let mut next = None;
        for page in 0..MAX_PAGES {
            let reply = peer
                .list_resource_templates(Some(PaginatedRequestParams::default().with_cursor(next)))
                .await
                .map_err(|_| Error::Transport("MCP resources/templates/list failed"))?;
            for template in reply.resource_templates {
                descriptor(&template)?;
                if !label(&template.name, 64)
                    || !names.insert(template.name.clone())
                    || rows.len() >= MAX_ENTRIES
                {
                    return Err(Error::Denied(
                        "invalid/duplicate/excess MCP resource template",
                    ));
                }
                rows.push(template);
            }
            next = cursor(reply.next_cursor, page, &mut seen)?;
            if next.is_none() {
                if conn.revisions.resources.load(Ordering::Acquire) != initial_revision {
                    return Err(Error::Denied("MCP templates changed while listing"));
                }
                return Ok(rows);
            }
        }
        Err(Error::Limit("too many MCP resource template pages"))
    })
    .await
    .map_err(|_| Error::Timeout)?
}

pub(super) async fn read(conn: &Connection, server: &str, uri: &str) -> Result<Vec<DataRow>> {
    valid_uri(uri)?;
    let response = tokio::time::timeout(CONNECT_TIMEOUT, async {
        conn.peer
            .lock()
            .await
            .read_resource_once(ReadResourceRequestParams::new(uri))
            .await
            .map_err(|_| Error::Transport("MCP resources/read failed"))
    })
    .await
    .map_err(|_| Error::Timeout)??;
    let ReadResourceResponse::Complete(result) = response else {
        return Err(Error::Denied(
            "MCP resource needs interactive authorization",
        ));
    };
    if serde_json::to_vec(&result)
        .map_err(|_| Error::Protocol("invalid MCP resource response"))?
        .len()
        > MAX_CONTENT
    {
        return Err(Error::Limit("MCP resource response exceeds 128 KiB"));
    }
    if result.contents.len() > MAX_MESSAGES {
        return Err(Error::Limit("too many MCP resource parts"));
    }
    let mut total = 0usize;
    let mut rows = Vec::new();
    for part in result.contents {
        let ResourceContents::TextResourceContents {
            uri: returned,
            text,
            mime_type,
            ..
        } = part
        else {
            return Err(Error::Denied("binary MCP resource is unsupported"));
        };
        if returned != uri {
            return Err(Error::Denied("MCP resource response URI mismatch"));
        }
        if mime_type.as_deref().is_some_and(|mime| {
            !mime.to_ascii_lowercase().starts_with("text/") && mime != "application/json"
        }) {
            return Err(Error::Denied("unsupported MCP resource media type"));
        }
        valid_text(&text, MAX_CONTENT)?;
        total = total.saturating_add(text.len());
        if total > MAX_CONTENT {
            return Err(Error::Limit("MCP resource response too large"));
        }
        let mut row = DataRow::new(server, "resource-text", uri);
        row.uri = Some(uri.into());
        row.text = Some(text);
        rows.push(row);
    }
    Ok(rows)
}
async fn get_prompt(
    conn: &Connection,
    server: &str,
    prompt: &Prompt,
    values: &std::collections::BTreeMap<String, String>,
) -> Result<Vec<DataRow>> {
    validate_prompt_args(prompt, values)?;
    let args = values
        .iter()
        .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
        .collect();
    let response = tokio::time::timeout(CONNECT_TIMEOUT, async {
        conn.peer
            .lock()
            .await
            .get_prompt_once(GetPromptRequestParams::new(&prompt.name).with_arguments(args))
            .await
            .map_err(|_| Error::Transport("MCP prompts/get failed"))
    })
    .await
    .map_err(|_| Error::Timeout)??;
    let GetPromptResponse::Complete(result) = response else {
        return Err(Error::Denied(
            "MCP prompt requires interactive authorization",
        ));
    };
    if serde_json::to_vec(&result)
        .map_err(|_| Error::Protocol("invalid MCP prompt response"))?
        .len()
        > MAX_CONTENT
    {
        return Err(Error::Limit("MCP prompt response exceeds 128 KiB"));
    }
    if result.messages.len() > MAX_MESSAGES {
        return Err(Error::Limit("too many MCP prompt messages"));
    }
    let mut rows = Vec::new();
    let mut total = 0usize;
    for item in result.messages {
        let ContentBlock::Text(content) = item.content else {
            return Err(Error::Denied("non-text MCP prompt content unsupported"));
        };
        valid_text(&content.text, MAX_CONTENT)?;
        total = total.saturating_add(content.text.len());
        if total > MAX_CONTENT {
            return Err(Error::Limit("MCP prompt content too large"));
        }
        let mut row = DataRow::new(server, "prompt-text", &prompt.name);
        row.role = Some(format!("{:?}", item.role).to_lowercase());
        row.text = Some(content.text);
        rows.push(row);
    }
    Ok(rows)
}

pub(super) fn capability(conn: &Connection, what: &str) -> Result<()> {
    let info = conn
        .peer
        .try_lock()
        .map_err(|_| Error::Protocol("MCP connection busy"))?;
    let caps = info
        .peer_info()
        .ok_or(Error::Protocol("MCP capability negotiation missing"))?;
    if (what == "prompt" && caps.capabilities.prompts.is_none())
        || (what == "resource" && caps.capabilities.resources.is_none())
    {
        return Err(Error::Denied(
            "MCP server did not advertise selected capability",
        ));
    }
    Ok(())
}

/// Active-Agent change feed for negotiated modern MCP. Nothing is started
/// without an explicit host selection and an actual approved tool/resource.
/// No automatic refresh, replacement grant, provider call or effect replay.
pub(super) async fn subscribe_authorized(
    conn: &Arc<Connection>,
    tools_granted: bool,
    uris: &BTreeSet<String>,
) -> Result<()> {
    if !tools_granted && uris.is_empty() {
        return Ok(());
    }
    let (version, caps) = {
        let peer = conn.peer.lock().await;
        let info = peer
            .peer_info()
            .ok_or(Error::Protocol("MCP capabilities missing"))?;
        (info.protocol_version.clone(), info.capabilities.clone())
    };
    if version != ProtocolVersion::V_2026_07_28 {
        // Pre-2026 peers deliver notifications to ClientHandler. Explicitly
        // subscribe to each approved resource URI when the server offers it;
        // a descriptor alone cannot authorize a new resource or effect.
        if !uris.is_empty() && caps.resources.as_ref().and_then(|r| r.subscribe) == Some(true) {
            let peer = conn.peer.lock().await;
            for uri in uris {
                #[allow(deprecated)]
                tokio::time::timeout(
                    CONNECT_TIMEOUT,
                    peer.subscribe(rmcp::model::SubscribeRequestParams::new(uri)),
                )
                .await
                .map_err(|_| Error::Timeout)?
                .map_err(|_| Error::Transport("MCP resource subscription rejected"))?;
            }
        }
        return Ok(());
    }
    let mut filter = SubscriptionFilter::default();
    filter.tools_list_changed = tools_granted.then_some(true);
    filter.resources_list_changed = (!uris.is_empty()).then_some(true);
    filter.resource_subscriptions = (!uris.is_empty()).then(|| uris.iter().cloned().collect());
    let filter = filter.supported_by(&caps);
    if filter.tools_list_changed != Some(true)
        && filter.resources_list_changed != Some(true)
        && filter.resource_subscriptions.is_none()
    {
        return Ok(());
    }
    let peer = conn.peer.lock().await;
    let mut subscription = tokio::time::timeout(CONNECT_TIMEOUT, peer.listen(filter))
        .await
        .map_err(|_| Error::Timeout)?
        .map_err(|_| Error::Transport("MCP change subscription rejected"))?;
    drop(peer);
    let revisions = Arc::clone(&conn.revisions);
    let task = tokio::spawn(async move {
        loop {
            match subscription.next().await {
                Ok(Some(notification)) => {
                    if let Some(changed) = notification_event(notification) {
                        revisions.invalidate(&changed);
                    }
                }
                Ok(None) | Err(_) => {
                    revisions.invalidate_all();
                    break;
                }
            }
        }
    });
    *conn.subscription_abort.lock().await = Some(task.abort_handle());
    Ok(())
}

async fn watch(
    conn: &Connection,
    server: &str,
    uri: Option<&str>,
    seconds: u64,
) -> Result<Vec<DataRow>> {
    let (version, caps) = {
        let peer = conn.peer.lock().await;
        let info = peer
            .peer_info()
            .ok_or(Error::Protocol("MCP capability negotiation missing"))?;
        (info.protocol_version.clone(), info.capabilities.clone())
    };
    if let Some(uri) = uri {
        valid_uri(uri)?;
        if !resources(conn).await?.iter().any(|r| r.uri == uri) {
            return Err(Error::Denied(
                "MCP resource subscription URI not advertised",
            ));
        }
        if caps.resources.as_ref().and_then(|r| r.subscribe) != Some(true) {
            return Err(Error::Denied(
                "MCP server does not support resource subscriptions",
            ));
        }
    }
    let mut rx = conn.notifications.subscribe();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(seconds);
    let mut events = Vec::new();
    if version == ProtocolVersion::V_2026_07_28 {
        let mut filter = SubscriptionFilter::default();
        filter.tools_list_changed = Some(true);
        filter.prompts_list_changed = Some(true);
        filter.resources_list_changed = Some(true);
        filter.resource_subscriptions = uri.map(|s| vec![s.to_owned()]);
        let peer = conn.peer.lock().await;
        let mut subscription =
            tokio::time::timeout(CONNECT_TIMEOUT, peer.listen(filter.supported_by(&caps)))
                .await
                .map_err(|_| Error::Timeout)?
                .map_err(|_| Error::Transport("MCP subscription listen rejected"))?;
        drop(peer);
        loop {
            let notification = match tokio::time::timeout_at(deadline, subscription.next()).await {
                Ok(Ok(Some(note))) => note,
                Ok(Ok(None)) => break,
                Ok(Err(_)) => return Err(Error::Transport("MCP subscription failed")),
                Err(_) => break,
            };
            if let Some(event) = notification_event(notification) {
                conn.revisions.invalidate(&event);
                push_event(&mut events, server, event)?;
            }
        }
        drop(subscription);
    } else {
        #[allow(deprecated)]
        if let Some(uri) = uri {
            let peer = conn.peer.lock().await;
            tokio::time::timeout(
                CONNECT_TIMEOUT,
                peer.subscribe(rmcp::model::SubscribeRequestParams::new(uri)),
            )
            .await
            .map_err(|_| Error::Timeout)?
            .map_err(|_| Error::Transport("MCP resource subscribe failed"))?;
        }
        loop {
            match tokio::time::timeout_at(deadline, rx.recv()).await {
                Ok(Ok(event)) => push_event(&mut events, server, event)?,
                Ok(Err(broadcast::error::RecvError::Lagged(_))) => {
                    return Err(Error::Limit("MCP notification overflow"))
                }
                Ok(Err(_)) | Err(_) => break,
            }
        }
        #[allow(deprecated)]
        if let Some(uri) = uri {
            let peer = conn.peer.lock().await;
            tokio::time::timeout(
                CONNECT_TIMEOUT,
                peer.unsubscribe(rmcp::model::UnsubscribeRequestParams::new(uri)),
            )
            .await
            .map_err(|_| Error::Timeout)?
            .map_err(|_| Error::Transport("MCP resource unsubscribe failed"))?;
        }
    }
    Ok(events)
}

fn notification_event(notification: ServerNotification) -> Option<Changed> {
    match notification {
        ServerNotification::PromptListChangedNotification(_) => Some(Changed::Prompts),
        ServerNotification::ResourceListChangedNotification(_) => Some(Changed::Resources),
        ServerNotification::ToolListChangedNotification(_) => Some(Changed::Tools),
        ServerNotification::ResourceUpdatedNotification(event) => valid_uri(&event.params.uri)
            .is_ok()
            .then_some(Changed::Resource(event.params.uri)),
        _ => None,
    }
}
fn push_event(rows: &mut Vec<DataRow>, server: &str, changed: Changed) -> Result<()> {
    if rows.len() >= MAX_ENTRIES {
        return Err(Error::Limit("too many MCP changes"));
    }
    rows.push(changed.row(server));
    Ok(())
}

/// Only explicit CLI actions read MCP data. No data is admitted as system
/// instructions or context, and no server notification launches any tool.
pub async fn inspect_data(options: &Options, action: &DataAction) -> Result<Vec<DataRow>> {
    let (servers, grants) = options.parsed()?;
    if !grants.is_empty() || !options.resource_allow.is_empty() {
        return Err(Error::Denied("MCP data inspection does not grant tools"));
    }
    if servers.is_empty() {
        return Err(Error::Denied("MCP data requires an explicit named server"));
    }
    let requested = match action {
        DataAction::Prompt { server, .. }
        | DataAction::Read { server, .. }
        | DataAction::ReadTemplate { server, .. }
        | DataAction::Watch { server, .. } => Some(server.as_str()),
        _ => None,
    };
    if let Some(id) = requested {
        if !servers.iter().any(|server| server.id() == id) {
            return Err(Error::Denied("MCP data action server not selected"));
        }
    }
    let mut rows = Vec::new();
    for spec in servers {
        if requested.is_some_and(|id| spec.id() != id) {
            continue;
        }
        let conn = connect(&spec).await?;
        let server = spec.id();
        let selected = match action {
            DataAction::Prompts => {
                capability(&conn, "prompt")?;
                prompts(&conn)
                    .await?
                    .into_iter()
                    .map(|p| {
                        let mut row = DataRow::new(server, "prompt", &p.name);
                        row.arguments = prompt_argument_views(&p)?;
                        row.description = p.description;
                        Ok(row)
                    })
                    .collect::<Result<Vec<_>>>()?
            }
            DataAction::Resources => {
                capability(&conn, "resource")?;
                resources(&conn)
                    .await?
                    .into_iter()
                    .map(|r| {
                        let mut row = DataRow::new(server, "resource", &r.name);
                        row.uri = Some(r.uri);
                        row.description = r.description;
                        row
                    })
                    .collect()
            }
            DataAction::Templates => {
                capability(&conn, "resource")?;
                templates(&conn)
                    .await?
                    .into_iter()
                    .map(|t| {
                        let mut row = DataRow::new(server, "template", &t.name);
                        row.uri = Some(t.uri_template);
                        row.description = t.description;
                        row
                    })
                    .collect()
            }
            DataAction::Prompt { name, args, .. } => {
                capability(&conn, "prompt")?;
                let found = prompts(&conn)
                    .await?
                    .into_iter()
                    .find(|p| p.name == *name)
                    .ok_or(Error::Denied("MCP prompt not advertised"))?;
                get_prompt(&conn, server, &found, &arguments(args)?).await?
            }
            DataAction::Read { uri, .. } => {
                capability(&conn, "resource")?;
                valid_uri(uri)?;
                if !resources(&conn).await?.iter().any(|r| &r.uri == uri) {
                    return Err(Error::Denied("MCP resource URI not advertised"));
                }
                read(&conn, server, uri).await?
            }
            DataAction::ReadTemplate { name, args, .. } => {
                capability(&conn, "resource")?;
                let found = templates(&conn)
                    .await?
                    .into_iter()
                    .find(|t| t.name == *name)
                    .ok_or(Error::Denied("MCP template not advertised"))?;
                let expanded = expand_template(&found.uri_template, &arguments(args)?)?;
                read(&conn, server, &expanded).await?
            }
            DataAction::Watch {
                resource, seconds, ..
            } => watch(&conn, server, resource.as_deref(), *seconds).await?,
        };
        if rows.len().saturating_add(selected.len()) > MAX_ENTRIES {
            return Err(Error::Limit("MCP discovery aggregate exceeds 128 rows"));
        }
        rows.extend(selected);
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_prompt_arguments_and_uri_template() {
        let prompt = Prompt::new(
            "review",
            Some("Review text"),
            Some(vec![
                rmcp::model::PromptArgument::new("path").with_required(true)
            ]),
        );
        assert!(
            validate_prompt_args(&prompt, &arguments(&["path=src/lib.rs".into()]).unwrap()).is_ok()
        );
        assert!(validate_prompt_args(&prompt, &arguments(&[]).unwrap()).is_err());
        assert!(validate_prompt_args(&prompt, &arguments(&["extra=x".into()]).unwrap()).is_err());
        assert!(arguments(&["a=1".into(), "a=2".into()]).is_err());
        assert!(arguments(&["bad".into()]).is_err());
        assert_eq!(
            expand_template(
                "test://host/{path}",
                &arguments(&["path=part name".into()]).unwrap()
            )
            .unwrap(),
            "test://host/part%20name"
        );
        assert!(expand_template(
            "test://host/{+path}",
            &arguments(&["path=a".into()]).unwrap()
        )
        .is_err());
        assert!(
            expand_template("test://host/{path}", &arguments(&["bad=a".into()]).unwrap()).is_err()
        );
        assert!(expand_template(
            "test://host/{path}",
            &arguments(&["path=..".into()]).unwrap()
        )
        .is_err());
    }
    #[test]
    fn notification_revisions_track_only_affected_capability() {
        let revisions = Revisions::default();
        revisions.invalidate(&Changed::Resources);
        revisions.invalidate(&Changed::Resource("test://host/a".into()));
        revisions.invalidate(&Changed::Tools);
        assert_eq!(revisions.resources.load(Ordering::Acquire), 2);
        assert_eq!(revisions.tools.load(Ordering::Acquire), 1);
        assert_eq!(revisions.prompts.load(Ordering::Acquire), 0);
        revisions.invalidate(&Changed::Prompts);
        assert_eq!(revisions.prompts.load(Ordering::Acquire), 1);
    }
    #[test]
    fn invalid_uri_and_cycle_denied() {
        for uri in [
            "",
            "relative",
            "file:///a/%2e%2e/secret",
            "test://user:pw@host/path",
            "test://host/file%2fetc",
            "test://host/a/../private",
            "test://host/a/./private",
        ] {
            assert!(valid_uri(uri).is_err(), "unexpected accepted: {uri}");
        }
        assert!(valid_uri("test://host/file.txt").is_ok());
        let mut seen = BTreeSet::new();
        assert!(cursor(Some("first".into()), 0, &mut seen).is_ok());
        assert!(cursor(Some("first".into()), 1, &mut seen).is_err());
        assert!(cursor(Some("n".into()), 7, &mut seen).is_err());
    }
}
