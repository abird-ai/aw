//! Host-only, bounded resource-read enrichment. The core read happens first;
//! an optional facet can never replace its authority or hide its snapshot.
use crate::source_metadata::ApprovalFile;
use aw::{Error, PrincipalId, Result};
use aw_agent_core::{
    view::{MAX_VIEW_NAMES, MAX_VIEW_NAME_BYTES, MAX_VIEW_RESULT_BYTES},
    View, ViewFailure, ViewRegistry,
};
use aw_store::{SessionStore, SourceMetadataApproval, SourceMetadataRef, SourceMetadataView};
use serde_json::{json, Value};
use std::{collections::BTreeSet, sync::OnceLock};

pub(crate) const SOURCE_METADATA: &str = "aw.source_metadata";

// 'sources' is the pre-existing Sandbox core selection, not a View.
pub(crate) fn include(params: &Value, allow_sources: bool) -> Result<Vec<String>> {
    let Some(value) = params.get("include") else {
        return Ok(Vec::new());
    };
    let names = value
        .as_array()
        .ok_or(Error::Protocol("include must be an array"))?;
    if names.len() > MAX_VIEW_NAMES {
        return Err(Error::Limit("too many included Views"));
    }
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for value in names {
        let name = value
            .as_str()
            .filter(|name| {
                !name.is_empty()
                    && name.len() <= MAX_VIEW_NAME_BYTES
                    && name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
            })
            .ok_or(Error::Protocol("invalid included View name"))?;
        if !seen.insert(name) {
            return Err(Error::Protocol("duplicate included View"));
        }
        if !(allow_sources && name == "sources") {
            out.push(name.to_owned());
        }
    }
    Ok(out)
}

struct ApprovedSources(std::result::Result<Vec<SourceMetadataRef>, ViewFailure>);
struct SourceIdentityView;
impl View for SourceIdentityView {
    type Resource = ApprovedSources;
    type Query = ();
    type Output = Vec<SourceMetadataRef>;

    fn read(
        &self,
        resource: &ApprovedSources,
        _: (),
    ) -> std::result::Result<Vec<SourceMetadataRef>, ViewFailure> {
        resource.0.clone()
    }
}

fn registry() -> Result<&'static ViewRegistry> {
    // Fixed trusted host installation. Nothing is loaded or activated from
    // include names; registered definitions are not independent authority.
    static VIEWS: OnceLock<ViewRegistry> = OnceLock::new();
    if let Some(views) = VIEWS.get() {
        return Ok(views);
    }
    let views = ViewRegistry::new();
    views.register_wire("aw", "source_metadata", 1, SourceIdentityView, Some(0))?;
    Ok(VIEWS.get_or_init(|| views))
}

fn failure(error: &Error) -> ViewFailure {
    match error {
        Error::Denied(_) => ViewFailure::Denied,
        _ => ViewFailure::Unavailable,
    }
}

// Keep core's existing 4 KiB envelope reserve. Included names and constant
// errors are bounded by 16 * (97+64) bytes, safely inside that reserve.
const WIRE_LIMIT: usize = aw::headless::MAX_FRAME_BYTES - 1024;
fn fits(value: &Value) -> bool {
    serde_json::to_vec(value).is_ok_and(|bytes| bytes.len() <= WIRE_LIMIT)
}

pub(crate) fn append<R: Send + Sync + 'static>(
    mut core: Value,
    names: &[String],
    resource: &R,
) -> Result<Value> {
    core = super::bounded_source_metadata(core)?;
    if names.is_empty() {
        return Ok(core);
    }
    let reader = registry().and_then(ViewRegistry::reader);
    let extensions = core
        .as_object_mut()
        .ok_or(Error::Storage("View core snapshot missing"))?;
    // Reserve all per-View error entries before admitting any result.
    extensions.insert(
        "extensions".into(),
        Value::Object(
            names
                .iter()
                .map(|name| (name.clone(), json!({"status":"error","code":"unavailable"})))
                .collect(),
        ),
    );
    for name in names {
        let outcome = match &reader {
            Ok(reader) => reader.read_wire(name, resource, Value::Null, MAX_VIEW_RESULT_BYTES),
            Err(_) => ViewFailure::Unavailable.into(),
        };
        core["extensions"][name] = match serde_json::to_value(outcome) {
            Ok(value) => value,
            Err(_) => json!({"status":"error","code":"execution_failed"}),
        };
        if !fits(&core) {
            core["extensions"][name] = json!({"status":"error","code":"unavailable"});
        }
    }
    Ok(core)
}

/// Current host decision; read/encode under its original approval lease.
pub(crate) struct SourceViewContext<'a> {
    pub store: &'a SessionStore,
    pub actor: &'a PrincipalId,
    pub project: &'a str,
    pub approval: Option<&'a ApprovalFile>,
}

impl SourceViewContext<'_> {
    pub(crate) fn render<T: PartialEq>(
        &self,
        snapshot: &T,
        core: Value,
        names: &[String],
        fetch: impl FnOnce(&SourceMetadataApproval) -> Result<SourceMetadataView<T>>,
    ) -> Result<Value> {
        let approval = self
            .approval
            .map(|file| file.read(self.store, self.actor, self.project))
            .transpose();
        let disclosed = match &approval {
            Ok(Some(approval)) => match fetch(approval.policy()) {
                Ok(view) if &view.snapshot == snapshot => {
                    if view.disclosed.is_empty() {
                        Err(ViewFailure::Denied)
                    } else {
                        Ok(view.disclosed)
                    }
                }
                Ok(_) => Err(ViewFailure::Unavailable), // snapshot raced
                Err(error) => Err(failure(&error)),
            },
            Ok(None) => Err(ViewFailure::Denied),
            Err(error) => Err(failure(error)),
        };
        // Existing opt-in inline identity output remains compatible. An
        // explicit View uses only the namespaced result, not two copies.
        let mut core = core;
        if !names.iter().any(|name| name == SOURCE_METADATA) {
            if let Ok(disclosed) = &disclosed {
                let mut enriched = core.clone();
                if super::enrich_source_metadata(&mut enriched, disclosed).is_ok()
                    && super::bounded_source_metadata(enriched.clone()).is_ok()
                {
                    core = enriched;
                }
            }
        }
        append(core, names, &ApprovedSources(disclosed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn include_errors_and_output_budget_preserve_authoritative_core() {
        let names = include(
            &json!({"include":["sources","missing.view",SOURCE_METADATA]}),
            true,
        )
        .unwrap();
        assert_eq!(names, ["missing.view", SOURCE_METADATA]);
        assert!(include(&json!({"include":["same","same"]}), false).is_err());
        assert!(include(&json!({"include":[42]}), false).is_err());
        let core = json!({"ref":"fixture","rev":1,"sources":[]});
        let output = append(
            core.clone(),
            &names,
            &ApprovedSources(Err(ViewFailure::Denied)),
        )
        .unwrap();
        assert_eq!(
            output["extensions"]["missing.view"],
            json!({"status":"error","code":"unknown"})
        );
        assert_eq!(
            output["extensions"][SOURCE_METADATA],
            json!({"status":"error","code":"denied"})
        );
        let mut actual = output;
        actual.as_object_mut().unwrap().remove("extensions");
        assert_eq!(actual, core);

        // Even a core near the original page limit has room for all bounded
        // error entries; enrichment must not invalidate a readable snapshot.
        let core = json!({"body":"x".repeat(aw::headless::MAX_FRAME_BYTES - 4200)});
        let names: Vec<_> = (0..MAX_VIEW_NAMES)
            .map(|i| format!("missing.{}{}", "x".repeat(84), i))
            .collect();
        let result = append(core.clone(), &names, &()).unwrap();
        assert!(fits(&result));
        assert_eq!(result["body"], core["body"]);
        assert_eq!(
            result["extensions"].as_object().unwrap().len(),
            MAX_VIEW_NAMES
        );
    }
}
