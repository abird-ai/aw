//! Model-facing DL-200 Event tools.
//!
//! This adapter owns only schemas/argument validation and one host-injected
//! routing slot. The router slot is composition state, never Event authority;
//! every invocation still arrives through the exact ToolPolicy + Actor Envelope
//! binding of the registered tool and the host router revalidates the concrete
//! Agent/source at the authoritative Store.

use crate::{Error, Result};
use aw_agent_core::{Tool, ToolContext, ToolEffect, ToolFuture, ToolOutput, ToolSpec};
use serde_json::{json, Value};
use std::sync::{Arc, RwLock};

pub const EVENTS_PUBLISH: &str = "events_publish";
pub const EVENTS_SUBSCRIBE: &str = "events_subscribe";
pub const EVENT_TOOL_NAMES: &[&str] = &[EVENTS_PUBLISH, EVENTS_SUBSCRIBE];

pub trait EventRouter: Send + Sync {
    fn publish(&self, ctx: &ToolContext, event: &str, data: Option<&Value>) -> Result<Value>;
    fn subscribe(&self, ctx: &ToolContext, subject: &str, enabled: bool) -> Result<Value>;
}

/// Stable host-composition slot. Installing/replacing the router grants
/// nothing; model visibility still requires the exact registered Tool grant.
pub struct EventRouterSlot {
    inner: RwLock<Option<Arc<dyn EventRouter>>>,
}

impl Default for EventRouterSlot {
    fn default() -> Self {
        Self {
            inner: RwLock::new(None),
        }
    }
}

impl std::fmt::Debug for EventRouterSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let installed = self
            .inner
            .read()
            .map(|guard| guard.is_some())
            .unwrap_or(true);
        f.debug_struct("EventRouterSlot")
            .field("installed", &installed)
            .finish()
    }
}

impl EventRouterSlot {
    pub fn replace(&self, router: Arc<dyn EventRouter>) -> Result<()> {
        let mut guard = self
            .inner
            .write()
            .map_err(|_| Error::Protocol("Event router slot poisoned"))?;
        *guard = Some(router);
        Ok(())
    }

    pub fn clear(&self) -> Result<()> {
        let mut guard = self
            .inner
            .write()
            .map_err(|_| Error::Protocol("Event router slot poisoned"))?;
        *guard = None;
        Ok(())
    }

    pub fn is_installed(&self) -> Result<bool> {
        Ok(self
            .inner
            .read()
            .map_err(|_| Error::Protocol("Event router slot poisoned"))?
            .is_some())
    }

    fn current(&self) -> Result<Arc<dyn EventRouter>> {
        self.inner
            .read()
            .map_err(|_| Error::Protocol("Event router slot poisoned"))?
            .clone()
            .ok_or(Error::Denied("Event host router is unavailable"))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EventToolKind {
    Publish,
    Subscribe,
}

#[derive(Clone)]
struct EventTool {
    router: Arc<EventRouterSlot>,
    kind: EventToolKind,
}

impl EventTool {
    fn new(router: Arc<EventRouterSlot>, kind: EventToolKind) -> Self {
        Self { router, kind }
    }

    fn object<'a>(
        arguments: &'a Value,
        allowed: &[&str],
    ) -> Result<&'a serde_json::Map<String, Value>> {
        let object = arguments
            .as_object()
            .ok_or(Error::Denied("events_* arguments must be an object"))?;
        if object.keys().any(|key| !allowed.contains(&key.as_str())) {
            return Err(Error::Denied("unknown events_* argument"));
        }
        Ok(object)
    }

    fn event(arguments: &Value) -> Result<&str> {
        arguments
            .get("event")
            .and_then(Value::as_str)
            .filter(|event| {
                let Some(custom) = event.strip_prefix('#') else {
                    return false;
                };
                let bytes = custom.as_bytes();
                !bytes.is_empty()
                    && event.len() <= aw_tools::event_subject::MAX_EVENT_NAME_BYTES
                    && bytes[0].is_ascii_lowercase()
                    && bytes[1..].iter().all(|byte| {
                        byte.is_ascii_lowercase()
                            || byte.is_ascii_digit()
                            || matches!(*byte, b'_' | b'-')
                    })
            })
            .ok_or(Error::Denied(
                "events_publish requires a bounded canonical custom #event name",
            ))
    }

    fn data(arguments: &Value) -> Result<Option<&Value>> {
        match arguments.get("data") {
            None | Some(Value::Null) => Ok(None),
            Some(value) if value.is_object() => Ok(Some(value)),
            Some(_) => Err(Error::Denied("events_publish data must be a JSON object")),
        }
    }

    fn subject(arguments: &Value) -> Result<&str> {
        arguments
            .get("subject")
            .and_then(Value::as_str)
            .filter(|subject| {
                !subject.is_empty()
                    && subject.len() <= aw_tools::event_subject::MAX_EVENT_SUBJECT_BYTES
                    && subject.is_ascii()
                    && !subject.chars().any(char::is_control)
            })
            .ok_or(Error::Denied(
                "events_subscribe requires a bounded Event subject pattern",
            ))
    }

    fn enabled(arguments: &Value) -> Result<bool> {
        match arguments.get("enabled") {
            None => Ok(true),
            Some(value) => value
                .as_bool()
                .ok_or(Error::Denied("events_subscribe enabled must be boolean")),
        }
    }
}

pub fn model_tools(router: Arc<EventRouterSlot>) -> [Arc<dyn Tool>; 2] {
    [
        Arc::new(EventTool::new(router.clone(), EventToolKind::Publish)),
        Arc::new(EventTool::new(router, EventToolKind::Subscribe)),
    ]
}

impl Tool for EventTool {
    fn spec(&self) -> ToolSpec {
        match self.kind {
            EventToolKind::Publish => ToolSpec {
                name: EVENTS_PUBLISH.into(),
                description:
                    "Publish one bounded custom #event from the current Agent. Source/actor/Turn are host-derived."
                        .into(),
                parameters: json!({
                    "type":"object",
                    "properties":{
                        "event":{"type":"string","description":"Custom event token beginning with #"},
                        "data":{"type":"object","description":"Optional small JSON metadata"}
                    },
                    "required":["event"],
                    "additionalProperties":false
                }),
                effect: ToolEffect::Control,
            },
            EventToolKind::Subscribe => ToolSpec {
                name: EVENTS_SUBSCRIBE.into(),
                description:
                    "Enable or disable one durable Event subscription for the current Agent."
                        .into(),
                parameters: json!({
                    "type":"object",
                    "properties":{
                        "subject":{"type":"string","description":"Current-Agent or current-Project Event pattern"},
                        "enabled":{"type":"boolean","default":true}
                    },
                    "required":["subject"],
                    "additionalProperties":false
                }),
                effect: ToolEffect::Control,
            },
        }
    }

    fn is_available(&self) -> bool {
        self.router.is_installed().unwrap_or(false)
    }

    fn validate(&self, arguments: &Value) -> Result<()> {
        match self.kind {
            EventToolKind::Publish => {
                Self::object(arguments, &["event", "data"])?;
                let _ = Self::event(arguments)?;
                let _ = Self::data(arguments)?;
            }
            EventToolKind::Subscribe => {
                Self::object(arguments, &["subject", "enabled"])?;
                let _ = Self::subject(arguments)?;
                let _ = Self::enabled(arguments)?;
            }
        }
        Ok(())
    }

    fn execute<'a>(&'a self, ctx: ToolContext, arguments: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            self.validate(arguments)?;
            let router = self.router.current()?;
            let value = match self.kind {
                EventToolKind::Publish => {
                    router.publish(&ctx, Self::event(arguments)?, Self::data(arguments)?)?
                }
                EventToolKind::Subscribe => {
                    router.subscribe(&ctx, Self::subject(arguments)?, Self::enabled(arguments)?)?
                }
            };
            let encoded = serde_json::to_string(&value)
                .map_err(|_| Error::Protocol("cannot encode Event tool result"))?;
            Ok(ToolOutput::text(encoded))
        })
    }
}
