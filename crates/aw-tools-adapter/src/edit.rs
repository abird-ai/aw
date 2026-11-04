//! Ordered multi-file exact edits. Planning and receipt bounds come from the
//! transport-neutral shared toolset. AW still owns tool authority and IO.
use crate::{platform_fs, tool_failure};
use aw_agent_core::*;
use aw_platform::scoped_fs::Root;
use aw_tools::{
    mutation::{PlanBudget, MAX_EDIT_FILES, MAX_EDIT_OPS},
    text::{count_matches, replace_literal_in_place, replacement_len},
};
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

const MAX_FILE_BYTES: usize = 1024 * 1024;
const MAX_AMBIGUOUS_LINES: usize = 10;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EditArgs {
    path: Option<String>,
    #[serde(deserialize_with = "bounded_edits")]
    edits: Vec<EditOp>,
}
fn bounded_edits<'de, D: serde::Deserializer<'de>>(
    de: D,
) -> std::result::Result<Vec<EditOp>, D::Error> {
    aw_tools::mutation::bounded_array::<D, EditOp, MAX_EDIT_OPS>(de, "edit", "edits")
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EditOp {
    path: Option<String>,
    #[serde(rename = "old", alias = "old_text")]
    old_text: String,
    #[serde(rename = "new", alias = "new_text")]
    new_text: String,
    #[serde(default, rename = "all", alias = "replace_all")]
    replace_all: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExactEdit {
    path: String,
    old: String,
    new: String,
    #[serde(default)]
    all: bool,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum ExactTuple {
    Two((String, String)),
    Three((String, String, bool)),
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExactFileEdits {
    path: String,
    #[serde(deserialize_with = "bounded_exact_edits")]
    edits: Vec<ExactTuple>,
}
fn bounded_exact_edits<'de, D: serde::Deserializer<'de>>(
    de: D,
) -> std::result::Result<Vec<ExactTuple>, D::Error> {
    aw_tools::mutation::bounded_array::<D, ExactTuple, MAX_EDIT_OPS>(de, "edit", "edits")
}
fn is_canonical(value: &Value) -> bool {
    value
        .get("edits")
        .and_then(Value::as_array)
        .is_some_and(|items| items.first().is_some_and(Value::is_array))
}
fn canonical(value: &Value) -> Result<ExactFileEdits> {
    serde_json::from_value(value.clone())
        .map_err(|_| Error::Denied("edit requires path and edits=[[old,new,all?],...]"))
}
fn exact_error(error: aw_tools::filesystem::exact_edit::EditError) -> String {
    use aw_tools::filesystem::exact_edit::EditError;
    match error {
        EditError::Empty => "fail old must not be empty".into(),
        EditError::Missing(id) => format!("fail edit {id} old not found"),
        EditError::Ambiguous { edit, matches } => {
            format!("fail edit {edit} old matches {matches} times; set all=true or use patch")
        }
        EditError::Overlap { first, second } => format!("fail edits {first},{second} overlap"),
        EditError::TooMany => "fail edits must contain 1..512 tuples".into(),
        EditError::OutputLimit => "fail resulting edit exceeds file byte limit".into(),
    }
}
/// Canonical legacy singular edit; legacy edits[] is accepted at this adapter only.
fn parse(value: &Value) -> Result<EditArgs> {
    if is_canonical(value) {
        let c = canonical(value)?;
        return Ok(EditArgs {
            path: Some(c.path),
            edits: c
                .edits
                .into_iter()
                .map(|tuple| {
                    let (old, new, all) = match tuple {
                        ExactTuple::Two((old, new)) => (old, new, false),
                        ExactTuple::Three((old, new, all)) => (old, new, all),
                    };
                    EditOp {
                        path: None,
                        old_text: old,
                        new_text: new,
                        replace_all: all,
                    }
                })
                .collect(),
        });
    }
    if value.get("edits").is_some() {
        return serde_json::from_value(value.clone())
            .map_err(|_| Error::Denied("invalid legacy edit array"));
    }
    let exact: ExactEdit = serde_json::from_value(value.clone())
        .map_err(|_| Error::Denied("edit requires path, old, new and optional all"))?;
    Ok(EditArgs {
        path: Some(exact.path),
        edits: vec![EditOp {
            path: None,
            old_text: exact.old,
            new_text: exact.new,
            replace_all: exact.all,
        }],
    })
}
pub(crate) struct Edit {
    root: Arc<Root>,
    limits: crate::LocalToolLimits,
}
impl Edit {
    #[cfg(test)]
    pub(crate) fn new(root: Arc<Root>) -> Self {
        Self::with_limits(root, crate::LocalToolLimits::default())
    }
    pub(crate) fn with_limits(root: Arc<Root>, limits: crate::LocalToolLimits) -> Self {
        Self { root, limits }
    }
}
struct Plan {
    original: Vec<u8>,
    text: String,
    predecessors: Vec<String>,
}
fn prefixed_read_line(needle: &str) -> bool {
    let Some((before, _)) = needle.split_once('\t') else {
        return false;
    };
    !before.is_empty() && before.len() <= 10 && before.bytes().all(|b| b.is_ascii_digit())
}
impl Tool for Edit {
    fn spec(&self) -> ToolSpec {
        use aw_tools::filesystem::schema::Tool as C;
        crate::core_spec(C::Edit, self.limits.schema)
    }
    fn validate(&self, value: &Value) -> Result<()> {
        let args = parse(value)?;
        if args.edits.is_empty() || args.edits.len() > MAX_EDIT_OPS {
            return Err(Error::Limit("edit requires 1..=512 operations"));
        }
        let mut paths = BTreeSet::new();
        let mut bytes = 0usize;
        for op in &args.edits {
            let path = op
                .path
                .as_ref()
                .or(args.path.as_ref())
                .ok_or(Error::Denied("edit path is missing"))?;
            crate::model_path(path)?;
            paths.insert(path);
            if op.old_text.is_empty() {
                return Err(Error::Denied("old_text must be nonempty"));
            }
            crate::text_class::require_text(op.old_text.as_bytes())?;
            crate::text_class::require_text(op.new_text.as_bytes())?;
            bytes = bytes
                .saturating_add(op.old_text.len())
                .saturating_add(op.new_text.len());
            if bytes > self.limits.edit_text_bytes {
                return Err(Error::Limit(
                    "combined edit text exceeds configured tool ceiling",
                ));
            }
            if paths.len() > MAX_EDIT_FILES {
                return Err(Error::Limit("edit exceeds 64 distinct files"));
            }
        }
        Ok(())
    }
    fn execute<'a>(&'a self, ctx: ToolContext, value: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            self.validate(value)?;
            let _ = ctx.result_budget(self.limits.result_bytes)?;
            let modern = is_canonical(value);
            let legacy = value.get("edits").is_some() && !modern;
            let args = parse(value)?;
            let root = self.root.clone();
            let result=aw_tools::asynchronous::run_blocking(move||->Result<ToolOutput>{
                if ctx.cancellation.is_cancelled(){return Err(Error::Cancelled);}
                if modern{
                    let lexical=args.path.as_ref().expect("canonical path");
                    let path=crate::content_path(&root,lexical,false)?;
                    let ops=args.edits.iter().map(|op|
                        aw_tools::filesystem::exact_edit::Replacement{
                            old:&op.old_text,new:&op.new_text,all:op.replace_all
                        }
                    ).collect::<Vec<_>>();
                    return match aw_tools::filesystem::operations::edit_text(
                        root.as_ref(),&path,&ops,MAX_FILE_BYTES,MAX_FILE_BYTES,
                        &||ctx.cancellation.is_cancelled()
                    ){
                        Ok(outcome)=>Ok(ToolOutput::text(outcome.as_str())),
                        Err(aw_tools::filesystem::operations::Error::Edit(error))=>
                            Ok(ToolOutput::error(exact_error(error))),
                        Err(error)=>Err(crate::file_operation_error(error)),
                    };
                }
                let resolved=args.edits.iter().map(|op| {
                    let lexical=op.path.as_ref().or(args.path.as_ref())
                        .expect("validated edit path");
                    Ok((lexical.clone(), crate::content_path(&root,lexical,false)?))
                }).collect::<Result<Vec<_>>>()?;
                let mut aliases=BTreeMap::<Vec<u8>,String>::new();
                for (lexical,path) in &resolved {
                    let key=platform_fs(
                        aw_tools::filesystem::backend::SecuredFilesystem::mutation_path_key(
                            root.as_ref(),path
                        )
                    )?;
                    if aliases.insert(key,lexical.clone()).is_some_and(|old|old!=*lexical) {
                        return Err(Error::Denied("edit paths alias the same namespace entry"));
                    }
                }
                let paths=resolved.iter().map(|(_,path)|path.clone()).collect::<Vec<_>>();
                let _mutation=platform_fs(root.lock_mutations_with_cancel(
                    &paths,&||ctx.cancellation.is_cancelled()
                ))?;
                let mut plans=BTreeMap::<String,Plan>::new();
                let mut budget=PlanBudget::default();
                for (index,op) in args.edits.iter().enumerate() {
                    if ctx.cancellation.is_cancelled(){return Err(Error::Cancelled);}
                    let path=&paths[index];
                    if !plans.contains_key(path) {
                        let cap=budget.copy_read_limit(MAX_FILE_BYTES).map_err(|_|
                            Error::Limit("aggregate mutation plan exceeds 64 MiB")
                        )?;
                        let original=platform_fs(root.read_bytes(path,cap))?;
                        let source=crate::text_class::require_text(&original)?;
                        let text=source.to_owned();
                        budget.add(original.len(),text.len()).map_err(|_|
                            Error::Limit("aggregate mutation plan exceeds 64 MiB")
                        )?;
                        plans.insert(path.clone(),Plan{
                            original,text,predecessors:Vec::new(),
                        });
                    }
                    let plan=plans.get_mut(path).expect("inserted");
                    if plan.predecessors.iter().any(|new|new.contains(&op.old_text)) {
                        return Ok(ToolOutput::error(format!(
                            "edit {}/{} ({path}): old_text occurs within an earlier replacement",
                            index+1,args.edits.len()
                        )));
                    }
                    let hits=count_matches(&plan.text,&op.old_text);
                    if hits==0 {
                        let explanation=if prefixed_read_line(&op.old_text) {
                            "strip the read line-number prefix (digits and tab)"
                        } else {"old_text not found"};
                        return Ok(ToolOutput::error(format!(
                            "edit {}/{} ({path}): {explanation}",index+1,args.edits.len()
                        )));
                    }
                    if hits>1 && !op.replace_all {
                        let first=plan.text.match_indices(&op.old_text)
                            .take(MAX_AMBIGUOUS_LINES)
                            .map(|(at,_)|1+plan.text[..at].bytes().filter(|&b|b==b'\n').count())
                            .collect::<Vec<_>>();
                        return Ok(ToolOutput::error(format!(
                            "edit {}/{} ({path}): {hits} matches at lines {first:?}; make unique or replace_all",
                            index+1,args.edits.len()
                        )));
                    }
                    let count=if op.replace_all{hits}else{1};
                    let next_len=replacement_len(plan.text.len(),op.old_text.len(),
                        op.new_text.len(),count).ok_or(Error::Limit("edit expansion overflows"))?;
                    if next_len>MAX_FILE_BYTES {
                        return Err(Error::Limit("edited file exceeds 1 MiB"));
                    }
                    budget.replace_work(plan.text.len(),next_len).map_err(|_|
                        Error::Limit("aggregate mutation plan exceeds 64 MiB")
                    )?;
                    if count==1 {
                        let at=plan.text.find(&op.old_text).expect("counted match");
                        plan.text.replace_range(at..at+op.old_text.len(),&op.new_text);
                    }else if op.new_text.len()<=op.old_text.len() {
                        replace_literal_in_place(&mut plan.text,&op.old_text,&op.new_text);
                    }else{
                        plan.text=plan.text.replace(&op.old_text,&op.new_text);
                    }
                    plan.predecessors.push(op.new_text.clone());
                }
                let changed=plans.values().filter(|p|p.original!=p.text.as_bytes()).count();
                let mut staged=Vec::with_capacity(changed);
                for (path,plan) in &plans {
                    if ctx.cancellation.is_cancelled(){return Err(Error::Cancelled);}
                    if plan.original==plan.text.as_bytes(){continue;}
                    let file=platform_fs(root.stage_update(path,&plan.original,plan.text.as_bytes(),
                        &||ctx.cancellation.is_cancelled()))?;
                    staged.push(file);
                }
                if ctx.cancellation.is_cancelled(){return Err(Error::Cancelled);}
                let mut committed=0usize;
                for file in staged {
                    if ctx.cancellation.is_cancelled(){
                        return Err(Error::Protocol("multi-file edit interrupted during commit; inspect workspace"));
                    }
                    if platform_fs(file.commit()).is_err(){
                        return Err(Error::Protocol(
                            "multi-file edit commit uncertain or partial; inspect workspace before retrying"
                        ));
                    }
                    committed+=1;
                }
                Ok(ToolOutput::text(
                    aw_tools::mutation::compact_success(
                        committed,if legacy{Some(args.edits.len())}else{None}
                    )
                ))
            }).await.map_err(|_|Error::Protocol("edit worker failed; inspect workspace"))?;
            tool_failure(result, true)
        })
    }
}
