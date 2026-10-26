//! Ordered, bounded multi-file exact edits. Plan every file under one
//! cooperative mutation guard, stage all changes, then commit per file.
use crate::{platform_fs, tool_failure};
use aw_agent_core::*;
use aw_platform::scoped_fs::Root;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{collections::BTreeMap, sync::Arc};

const MAX_EDIT_OPS: usize = 64;
const MAX_EDIT_FILES: usize = 16;
const MAX_EDIT_BYTES: usize = 48 * 1024;
const MAX_FILE_BYTES: usize = 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EditArgs {
    path: Option<String>,
    edits: Vec<EditOp>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EditOp {
    path: Option<String>,
    old_text: String,
    new_text: String,
    #[serde(default)]
    replace_all: bool,
}
fn parse(value: &Value) -> Result<EditArgs> {
    serde_json::from_value(value.clone())
        .map_err(|_| Error::Denied("edit requires a bounded edits array"))
}
pub(crate) struct Edit {
    root: Arc<Root>,
}
impl Edit {
    pub(crate) fn new(root: Arc<Root>) -> Self {
        Self { root }
    }
}
struct Plan {
    original: Vec<u8>,
    text: String,
    edits: usize,
    predecessor_texts: Vec<String>,
}
fn line_delta(before: &str, after: &str) -> isize {
    let count = |text: &str| {
        if text.is_empty() {
            0
        } else {
            text.bytes().filter(|b| *b == b'\n').count() + usize::from(!text.ends_with('\n'))
        }
    };
    count(after) as isize - count(before) as isize
}
impl Tool for Edit {
    fn spec(&self) -> ToolSpec {
        ToolSpec{name:"edit".into(),description:"Apply an ordered edits[] array across up to 16 existing UTF-8 files (64 edits). Each entry has path?, old_text, new_text, replace_all?; a top-level path is inherited. An exact old_text must occur uniquely unless replace_all is enabled. A prior edit's replacement cannot serve as a later edit's search anchor. Every file is planned, checked and staged before any commit. Publication is atomic per file, not a cross-file transaction; receipts report changed files and line deltas. Strip numbered read prefixes before editing.".into(),parameters:json!({"type":"object","properties":{
            "path":{"type":"string"},
            "edits":{"type":"array","minItems":1,"maxItems":MAX_EDIT_OPS,"items":{"type":"object","properties":{
                "path":{"type":"string"},"old_text":{"type":"string","minLength":1},"new_text":{"type":"string"},"replace_all":{"type":"boolean"}},"required":["old_text","new_text"],"additionalProperties":false}}},
                "required":["edits"],"additionalProperties":false}),effect:ToolEffect::Write}
    }
    fn validate(&self, value: &Value) -> Result<()> {
        let args = parse(value)?;
        if args.edits.is_empty() || args.edits.len() > MAX_EDIT_OPS {
            return Err(Error::Limit("edit requires 1..=64 edits"));
        }
        let mut paths = std::collections::BTreeSet::new();
        let mut windows_spelling = std::collections::BTreeMap::new();
        let mut bytes = 0usize;
        for edit in &args.edits {
            let path = edit
                .path
                .as_ref()
                .or(args.path.as_ref())
                .ok_or(Error::Denied("edit path is missing"))?;
            platform_fs(aw_platform::scoped_fs::validate_path(path))?;
            paths.insert(path);
            if cfg!(windows) {
                let key = path.to_lowercase();
                if let Some(previous) = windows_spelling.insert(key, path) {
                    if previous != path {
                        return Err(Error::Denied(
                            "edit paths differ only by Windows filename casing",
                        ));
                    }
                }
            }
            crate::text_class::require_text(edit.old_text.as_bytes())?;
            crate::text_class::require_text(edit.new_text.as_bytes())?;
            if edit.old_text.is_empty() {
                return Err(Error::Denied("edit old_text must be nonempty"));
            }
            bytes = bytes
                .saturating_add(edit.old_text.len())
                .saturating_add(edit.new_text.len());
            if bytes > MAX_EDIT_BYTES {
                return Err(Error::Limit("combined edits exceed 48 KiB"));
            }
            if paths.len() > MAX_EDIT_FILES {
                return Err(Error::Limit("edit exceeds 16 distinct files"));
            }
        }
        Ok(())
    }
    fn execute<'a>(&'a self, ctx: ToolContext, value: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            self.validate(value)?;
            let args = parse(value)?;
            let root = self.root.clone();
            let answer=tokio::task::spawn_blocking(move||->Result<ToolOutput>{
                let paths=args.edits.iter().map(|edit|edit.path.as_ref().or(args.path.as_ref())
                    .expect("validated path").clone()).collect::<Vec<_>>();
                let _guard=platform_fs(root.lock_mutations_with_cancel(&paths,&||ctx.cancellation.is_cancelled()))?;
                let mut plans=BTreeMap::<String,Plan>::new();
                let total=args.edits.len();
                for (index,edit) in args.edits.iter().enumerate(){
                    if ctx.cancellation.is_cancelled(){return Err(Error::Cancelled);}
                    let path=paths[index].clone();
                    if !plans.contains_key(&path){
                        let original=platform_fs(root.read_bytes(&path,MAX_FILE_BYTES))?;
                        let text=crate::text_class::require_text(&original)?.to_owned();
                        plans.insert(path.clone(),Plan{
                            original,text,edits:0,predecessor_texts:Vec::new(),
                        });
                    }
                    let plan=plans.get_mut(&path).expect("inserted plan");
                    if plan.predecessor_texts.iter().any(|new|new.contains(&edit.old_text)){
                        return Ok(ToolOutput::error(format!("edit {}/{total} ({path}): old_text occurs within an earlier replacement",index+1)));
                    }
                    let hits=plan.text.match_indices(&edit.old_text).count();
                    if hits==0{
                        return Ok(ToolOutput::error(format!("edit {}/{total} ({path}): old_text not found",index+1)));
                    }
                    if hits>1 && !edit.replace_all {
                        let positions=plan.text.match_indices(&edit.old_text).take(10)
                            .map(|(at,_)|1+plan.text[..at].bytes().filter(|b|*b==b'\n').count())
                            .collect::<Vec<_>>();
                        return Ok(ToolOutput::error(format!("edit {}/{total} ({path}): {hits} matches at lines {positions:?}; use unique text or replace_all",index+1)));
                    }
                    if edit.replace_all{
                        plan.text=plan.text.replace(&edit.old_text,&edit.new_text);
                    }else{
                        plan.text=plan.text.replacen(&edit.old_text,&edit.new_text,1);
                    }
                    if plan.text.len()>MAX_FILE_BYTES{
                        return Err(Error::Limit("edited file would exceed 1 MiB"));
                    }
                    plan.edits+=1;
                    plan.predecessor_texts.push(edit.new_text.clone());
                }
                let mut staged=Vec::new();
                let mut receipt=Vec::new();
                let mut edits_applied=0usize;
                for (path,plan) in &plans {
                    edits_applied+=plan.edits;
                    if plan.original==plan.text.as_bytes(){continue;}
                    let before=std::str::from_utf8(&plan.original).expect("verified UTF-8");
                    receipt.push(json!({"path":path,"edits":plan.edits,
                        "line_delta":line_delta(before,&plan.text)}));
                    staged.push(platform_fs(root.stage_update(path,&plan.original,
                        plan.text.as_bytes(),&||ctx.cancellation.is_cancelled()))?);
                }
                if ctx.cancellation.is_cancelled(){return Err(Error::Cancelled);}
                let mut committed=0usize;
                for file in staged{
                    if ctx.cancellation.is_cancelled(){
                        return Err(Error::Protocol("multi-file edit interrupted during commit; inspect workspace"));
                    }
                    if let Err(error)=platform_fs(file.commit()){
                        return if committed==0 {Err(error)} else {
                            Err(Error::Protocol("multi-file edit partially committed; inspect workspace"))
                        };
                    }
                    committed+=1;
                }
                Ok(ToolOutput::text(json!({"files":receipt,"edits_applied":edits_applied,
                    "changed_files":committed}).to_string()))
            }).await.map_err(|_|Error::Protocol("edit worker failed; inspect workspace"))?;
            tool_failure(answer, true)
        })
    }
}
