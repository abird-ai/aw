//! One canonical AI-native schema registry. Adapters map these transport-free
//! definitions into AW ToolSpec, dotlink MCP Tool, or other model protocols.
//! Host-specific byte ceilings and read-wave hints are explicit inputs.
use super::access::Action;
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Ls,
    Find,
    Grep,
    Read,
    Write,
    Edit,
    Patch,
    Copy,
    Move,
    Remove,
    Bread,
    Bwrite,
    Bedit,
}
impl Tool {
    pub const ALL: [Self; 13] = [
        Self::Ls,
        Self::Find,
        Self::Grep,
        Self::Read,
        Self::Write,
        Self::Edit,
        Self::Patch,
        Self::Copy,
        Self::Move,
        Self::Remove,
        Self::Bread,
        Self::Bwrite,
        Self::Bedit,
    ];
    /// Canonical V1 native Graph surface. `bread` is deliberately direct-only
    /// until retained Graph results can preserve content-aware binary/typed
    /// output without argument-dependent eligibility.
    pub const GRAPH_NATIVE: [Self; 12] = [
        Self::Ls,
        Self::Find,
        Self::Grep,
        Self::Read,
        Self::Write,
        Self::Edit,
        Self::Patch,
        Self::Copy,
        Self::Move,
        Self::Remove,
        Self::Bwrite,
        Self::Bedit,
    ];

    pub const fn graph_eligible(self) -> bool {
        !matches!(self, Self::Bread)
    }
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "ls" => Self::Ls,
            "find" => Self::Find,
            "grep" => Self::Grep,
            "read" => Self::Read,
            "write" => Self::Write,
            "edit" => Self::Edit,
            "patch" => Self::Patch,
            "cp" => Self::Copy,
            "mv" => Self::Move,
            "rm" => Self::Remove,
            "bread" => Self::Bread,
            "bwrite" => Self::Bwrite,
            "bedit" => Self::Bedit,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Ls => "ls",
            Self::Find => "find",
            Self::Grep => "grep",
            Self::Read => "read",
            Self::Write => "write",
            Self::Edit => "edit",
            Self::Patch => "patch",
            Self::Copy => "cp",
            Self::Move => "mv",
            Self::Remove => "rm",
            Self::Bread => "bread",
            Self::Bwrite => "bwrite",
            Self::Bedit => "bedit",
        }
    }
    pub fn action(self) -> Action {
        match self {
            Self::Ls | Self::Find | Self::Grep | Self::Read | Self::Bread => Action::Read,
            Self::Write | Self::Bwrite => Action::Write,
            Self::Edit | Self::Patch | Self::Bedit => Action::Edit,
            Self::Copy => Action::Copy,
            Self::Move => Action::Move,
            Self::Remove => Action::Remove,
        }
    }
}
/// Shortest valid canonical patch: `*** Begin Patch\n*** Add File: x\n*** End Patch`.
/// A lower advertised maxLength would make the enabled patch tool unusable.
pub const MIN_PATCH_BYTES: usize = 45;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SchemaCaps {
    pub text_write_bytes: usize,
    pub patch_bytes: usize,
    pub binary_data_bytes: usize,
    pub binary_edit_ranges: usize,
    /// General/complete-media byte ceiling for bread.
    pub binary_read_bytes: usize,
    /// Base64/hex byte-window ceiling; encoded output expands in transport.
    pub binary_encoded_read_bytes: usize,
    pub read_line_limit: usize,
    pub parallel_reads: bool,
    pub typed_media: bool,
}
impl Default for SchemaCaps {
    fn default() -> Self {
        Self {
            text_write_bytes: 48 * 1024,
            patch_bytes: 48 * 1024,
            binary_data_bytes: 48 * 1024,
            binary_edit_ranges: 32,
            binary_read_bytes: 65_536,
            binary_encoded_read_bytes: 24 * 1024,
            read_line_limit: 65_536,
            parallel_reads: false,
            typed_media: false,
        }
    }
}
impl SchemaCaps {
    /// Validate shared engine safety bounds plus irreducible semantic minima.
    /// Product adapters own their defaults and may choose any viable tighter
    /// ceiling inside these bounds.
    pub fn validate(self) -> Result<Self, &'static str> {
        if self.text_write_bytes == 0
            || self.text_write_bytes > crate::MAX_MUTATION_PLAN_BYTES
            || self.patch_bytes < MIN_PATCH_BYTES
            || self.patch_bytes > crate::MAX_MUTATION_PLAN_BYTES
            || self.binary_data_bytes == 0
            || self.binary_data_bytes > crate::MAX_MUTATION_PLAN_BYTES
            || self.binary_edit_ranges == 0
            || self.binary_edit_ranges > crate::MAX_BINARY_EDIT_RANGES
            || self.binary_read_bytes == 0
            || self.binary_read_bytes > crate::MAX_READ_WINDOW_BYTES
            || self.binary_encoded_read_bytes == 0
            || self.binary_encoded_read_bytes > self.binary_read_bytes
            || self.read_line_limit == 0
            || self.read_line_limit > 65_536
        {
            return Err("schema caps exceed shared engine safety/semantic bounds");
        }
        Ok(self)
    }
}

pub struct Schema {
    pub name: &'static str,
    pub description: String,
    pub parameters: Value,
    pub action: Action,
}
pub fn spec(tool: Tool, caps: SchemaCaps) -> Schema {
    let (description,mut parameters):(&str,Value)=match tool {
        Tool::Ls=>("Inspect one directory; compact kind and basename rows, opt-in metadata.",
            json!({"type":"object","properties":{
                "path":{"type":"string"},
                "sort":{"type":"string","enum":["path","size","mtime","ctime","btime"]},
                "meta":{"type":"array","maxItems":5,"items":{"type":"string",
                    "enum":["size","mtime","ctime","btime","mode"]}},
                "hidden":{"type":"boolean"},"gitignore":{"type":"boolean"},
                "offset":{"type":"integer","minimum":0},
                "limit":{"type":"integer","minimum":1,"maximum":4096}
            },"additionalProperties":false})),
        Tool::Find=>("Find paths by bounded glob, regex, kind, depth, timestamps. Compact output.",
            json!({"type":"object","properties":{
                "path":{"type":"string"},"glob":{"type":"string"},
                "regex":{"type":"string"},"depth":{"type":"integer","minimum":1,"maximum":128},
                "kind":{"type":"string","enum":["any","file","dir","symlink","other"]},
                "hidden":{"type":"boolean"},"gitignore":{"type":"boolean"},
                "mtime":{"$ref":"#/$defs/range"},
                "ctime":{"$ref":"#/$defs/range"},
                "btime":{"$ref":"#/$defs/range"},
                "sort":{"type":"string","enum":["path","size","mtime","ctime","btime"]},
                "meta":{"type":"array","maxItems":5,"items":{"type":"string",
                    "enum":["size","mtime","ctime","btime","mode"]}},
                "offset":{"type":"integer","minimum":0},
                "limit":{"type":"integer","minimum":1,"maximum":200}
            },"additionalProperties":false,
            "$defs":{"range":{"type":"object","properties":{
                "after":{"type":"string","format":"date-time"},
                "before":{"type":"string","format":"date-time"}
            },"additionalProperties":false}}})),
        Tool::Grep=>("Search bounded UTF-8 contents; mode=content/files/count and pathname glob.",
            json!({"type":"object","properties":{
                "pattern":{"type":"string"},"path":{"type":"string"},
                "glob":{"type":"string"},"depth":{"type":"integer","minimum":1,"maximum":128},
                "literal":{"type":"boolean"},"ignore_case":{"type":"boolean"},
                "mode":{"type":"string","enum":["content","files","count"]},
                "context":{"type":"integer","minimum":0,"maximum":8},
                "hidden":{"type":"boolean"},"gitignore":{"type":"boolean"},
                "offset":{"type":"integer","minimum":0},
                "limit":{"type":"integer","minimum":1,"maximum":200}
            },"required":["pattern"],"additionalProperties":false})),
        Tool::Read=>("Read a UTF-8 file from a logical line and optional byte cursor; supports exact continuation within long lines.",
            json!({"type":"object","properties":{
                "path":{"type":"string"},
                "line":{"type":"integer","minimum":1},
                "lines":{"type":"integer","minimum":1,"maximum":caps.read_line_limit},
                "byte":{"type":"integer","minimum":0},
                "numbered":{"type":"boolean"}
            },"required":["path"],"additionalProperties":false})),
        Tool::Write=>("Create or atomically replace one UTF-8 file, making missing parents. Returns ok.",
            json!({"type":"object","properties":{
                "path":{"type":"string"},
                "content":{"type":"string","maxLength":caps.text_write_bytes}
            },"required":["path","content"],"additionalProperties":false})),
        Tool::Edit=>("Apply independent exact replacements in one file, matched against one original snapshot.",
            json!({"type":"object","properties":{
                "path":{"type":"string"},
                "edits":{"type":"array","minItems":1,"maxItems":512,
                    "items":{"type":"array","minItems":2,"maxItems":3,
                        "prefixItems":[
                            {"type":"string","minLength":1},
                            {"type":"string"},{"type":"boolean"}
                        ]}}
            },"required":["path","edits"],"additionalProperties":false})),
        Tool::Patch=>("Apply bounded Codex Update/Add text patches; strict preflight and atomic per-file publication.",
            json!({"type":"object","properties":{
                "patch":{"type":"string","maxLength":caps.patch_bytes}
            },"required":["patch"],"additionalProperties":false})),
        Tool::Copy=>("Copy one exact file, symlink or complete directory tree to a new destination. Never overwrite.",
            json!({"type":"object","properties":{
                "from":{"type":"string"},"to":{"type":"string"}
            },"required":["from","to"],"additionalProperties":false})),
        Tool::Move=>("Rename one exact path to a new destination on the same volume. Never overwrite or copy+delete.",
            json!({"type":"object","properties":{
                "from":{"type":"string"},"to":{"type":"string"}
            },"required":["from","to"],"additionalProperties":false})),
        Tool::Remove=>("Remove exact paths; non-empty directory requires recursive=true. No glob expansion.",
            json!({"type":"object","properties":{
                "paths":{"type":"array","minItems":1,"maxItems":64,
                    "items":{"type":"string"}},"recursive":{"type":"boolean"}
            },"required":["paths"],"additionalProperties":false})),
        Tool::Bread=>("Read a binary byte window as base64/hex, or complete media in mcp format.",
            json!({"type":"object","properties":{
                "path":{"type":"string"},"offset":{"type":"integer","minimum":0},
                "limit":{"type":"integer","minimum":1,"maximum":caps.binary_read_bytes},
                "format":{"type":"string","enum":["mcp","base64","hex"]}
            },"required":["path"],"additionalProperties":false,
            "allOf":[{"if":{"required":["format"],"properties":{
                "format":{"enum":["base64","hex"]}
            }},"then":{"properties":{"limit":{"maximum":caps.binary_encoded_read_bytes}}}}]})),
        Tool::Bwrite=>("Create or atomically replace one file with base64/hex data. Returns ok.",
            json!({"type":"object","properties":{
                "path":{"type":"string"},
                "data":{"type":"string","maxLength":caps.binary_data_bytes},
                "encoding":{"type":"string","enum":["base64","hex"]}
            },"required":["path","data"],"additionalProperties":false})),
        Tool::Bedit=>("Apply bounded original-relative binary ranges with optional SHA-256 precondition.",
            json!({"type":"object","properties":{
                "path":{"type":"string"},"sha256":{"type":"string"},
                "edits":{"type":"array","minItems":1,"maxItems":caps.binary_edit_ranges,
                    "items":{"type":"object","properties":{
                        "offset":{"type":"integer","minimum":0},
                        "length":{"type":"integer","minimum":0},
                        "data":{"type":"string","maxLength":caps.binary_data_bytes},
                        "encoding":{"type":"string","enum":["base64","hex"]}
                    },"required":["offset"],"additionalProperties":false}}
            },"required":["path","edits"],"additionalProperties":false})),
    };
    // Parallel scheduling is AW Agent-specific, not part of the canonical
    // filesystem semantics. Dotlink does not need to advertise it.
    if caps.parallel_reads && tool.action() == Action::Read {
        parameters["properties"]["parallel"] = json!({"type":"boolean"});
    }
    let description = if tool == Tool::Bread && !caps.typed_media {
        "Read base64/hex windows; mcp returns encoded complete media when native typed output is unavailable.".to_owned()
    } else {
        description.to_owned()
    };
    Schema {
        name: tool.name(),
        description,
        parameters,
        action: tool.action(),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn graph_native_registry_excludes_only_direct_binary_read() {
        assert_eq!(Tool::GRAPH_NATIVE.len(), Tool::ALL.len() - 1);
        assert!(!Tool::Bread.graph_eligible());
        assert!(Tool::GRAPH_NATIVE.iter().all(|tool| tool.graph_eligible()));
        assert!(!Tool::GRAPH_NATIVE.contains(&Tool::Bread));
    }

    #[test]
    fn schemas_are_unique_small_and_have_no_legacy_fields() {
        let mut names = std::collections::BTreeSet::new();
        let mut bytes = 0usize;
        for tool in Tool::ALL {
            let schema = spec(tool, SchemaCaps::default());
            assert!(names.insert(schema.name));
            let wire = serde_json::to_vec(&schema.parameters).unwrap();
            bytes += wire.len() + schema.description.len() + schema.name.len();
            if tool == Tool::Edit {
                let props = &schema.parameters["properties"];
                assert!(props.get("path").is_some());
                assert!(props.get("edits").is_some());
                for legacy in ["old", "new", "all"] {
                    assert!(props.get(legacy).is_none());
                }
            }
            if tool == Tool::Ls {
                assert!(schema.parameters["properties"].get("depth").is_none());
            }
        }
        assert_eq!(names.len(), 13);
        assert!(bytes < 12 * 1024, "schema budget {bytes}");
    }
    #[test]
    fn schema_caps_use_shared_safety_maxima_not_one_hosts_defaults() {
        let caps = SchemaCaps {
            text_write_bytes: 1024 * 1024,
            patch_bytes: 1024 * 1024,
            binary_data_bytes: 1024 * 1024,
            binary_edit_ranges: 4,
            binary_read_bytes: 8 * 1024 * 1024,
            binary_encoded_read_bytes: 128 * 1024,
            read_line_limit: 128,
            ..Default::default()
        };
        assert_eq!(caps.validate().unwrap(), caps);
        let zero = SchemaCaps {
            read_line_limit: 0,
            ..Default::default()
        };
        assert!(zero.validate().is_err());
        let too_wide = SchemaCaps {
            binary_read_bytes: crate::MAX_READ_WINDOW_BYTES + 1,
            ..Default::default()
        };
        assert!(too_wide.validate().is_err());
        let encoded_too_wide = SchemaCaps {
            binary_encoded_read_bytes: SchemaCaps::default().binary_read_bytes + 1,
            ..Default::default()
        };
        assert!(encoded_too_wide.validate().is_err());
    }

    #[test]
    fn bread_schema_advertises_smaller_encoded_window_without_narrowing_mcp() {
        let caps = SchemaCaps::default();
        let schema = spec(Tool::Bread, caps).parameters;
        assert_eq!(
            schema["properties"]["limit"]["maximum"],
            caps.binary_read_bytes
        );
        assert_eq!(
            schema["allOf"][0]["then"]["properties"]["limit"]["maximum"],
            caps.binary_encoded_read_bytes
        );
    }

    #[test]
    fn patch_cap_cannot_advertise_an_impossible_schema() {
        let invalid = SchemaCaps {
            patch_bytes: MIN_PATCH_BYTES - 1,
            ..SchemaCaps::default()
        };
        assert!(invalid.validate().is_err());
        assert!(SchemaCaps {
            patch_bytes: MIN_PATCH_BYTES,
            ..SchemaCaps::default()
        }
        .validate()
        .is_ok());
    }

    #[test]
    fn adapter_additions_do_not_change_canonical_semantics() {
        let basic = spec(Tool::Find, SchemaCaps::default());
        assert!(basic.parameters["properties"].get("parallel").is_none());
        let caps = SchemaCaps {
            parallel_reads: true,
            ..Default::default()
        };
        assert!(spec(Tool::Find, caps).parameters["properties"]
            .get("parallel")
            .is_some());
        assert!(spec(Tool::Copy, caps).parameters["properties"]
            .get("parallel")
            .is_none());
        assert_eq!(spec(Tool::Copy, caps).action, Action::Copy);
    }
}
