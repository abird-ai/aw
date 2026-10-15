//! Application command surface for explicit, tool-free context flow.
//! The core owns synthesis; the store owns permission, snapshots and receipts.
//! Frontends never call a raw HTTP provider or invent summary history.
use crate::sessions::PersistentSession;
use aw_agent_core::{Agent, Error, PrincipalId, Result};
use aw_store::{ApplyContext, PrepareContext, TargetHead};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::future::Future;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Sources,
    Targets,
    TargetAt {
        session_id: String,
        branch: String,
    },
    Branches {
        session_id: String,
    },
    Target,
    Prepare {
        request: PrepareContext,
    },
    Plan {
        plan_id: String,
    },
    Manual {
        plan_id: String,
        request_id: String,
        text: String,
        #[serde(default)]
        from: Option<String>,
    },
    Generate {
        plan_id: String,
        request_id: String,
        account_id: String,
        model: String,
        confirmed_disclosure: bool,
    },
    Operation {
        id: String,
    },
    Artifact {
        artifact_id: String,
    },
    Apply {
        request: ApplyContext,
    },
    Pending,
    Application {
        request_id: String,
    },
    DiscardPending {
        id: String,
    },
    /// Owner-authorized read-only source sharing. A browser host must additionally
    /// restrict `principal` to its known current realm participants.
    GrantRead {
        session_id: String,
        principal: String,
        allow: bool,
        #[serde(default)]
        summary_accounts: Vec<String>,
    },
}
impl Command {
    pub fn is_generation(&self) -> bool {
        matches!(self, Self::Generate { .. })
    }
    pub fn mutates_view(&self) -> bool {
        matches!(self, Self::Apply { .. })
    }
}
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> Result<T> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|_| Error::Storage("context operation worker failed"))?
}
fn value<T: Serialize>(v: T) -> Result<Value> {
    serde_json::to_value(v).map_err(|_| Error::Storage("context response serialization failed"))
}
fn compatible_target(p: &PersistentSession, t: &TargetHead) -> Result<()> {
    // Selection is explicit, but cannot silently rebind this executor. Merely
    // preparing another branch leaves the currently viewed branch unchanged.
    let destination = p.store.session(&p.actor, &t.session_id)?;
    crate::sessions::check_binding(
        &p.binding,
        &destination.binding,
        p.allow_transfer && t.session_id == p.handle.session_id(),
    )
}
/// Authenticated native host seam. The caller supplies the already-authorized
/// active controller; no actor, endpoint, payer or grant comes from Command.
/// Requests are serialized with the controller's ordinary mutation lock. Paid
/// generation does not hold a SQLite/branch writer lock while awaiting a model.
pub async fn execute<C: Future<Output = ()> + Send>(
    agent: &mut Agent,
    p: &mut PersistentSession,
    command: Command,
    cancel: C,
) -> Result<Value> {
    let store = p.store.clone();
    let actor = p.actor.clone();
    match command {
        Command::Sources=>blocking(move||Ok(Value::Array(store.list(&actor)?.into_iter()
            .map(|s|json!({"id":s.id,"title":s.title,"owner":s.owner,"imported":s.imported})).collect()))).await,
        Command::Targets=>{let binding=p.binding.clone();blocking(move||{
            let values=store.list(&actor)?.into_iter().filter(|s|s.owner==actor&&crate::sessions::check_binding(&binding,&s.binding,false).is_ok()).map(|s|json!({"id":s.id,"title":s.title,"owner":s.owner})).collect::<Vec<_>>();Ok(Value::Array(values))
        }).await},
        Command::TargetAt{session_id,branch}=>{let prior=p.clone();blocking(move||{
            let t=store.context_target(&actor,&session_id,&branch)?;compatible_target(&prior,&t)?;value(t)
        }).await},
        Command::Branches{session_id}=>blocking(move||value(store.branches(&actor,&session_id)?)).await,
        Command::Target=>{let session=p.handle.session_id().to_owned();let branch=p.handle.branch()?.id;
            blocking(move||value(store.context_target(&actor,&session,&branch)?)).await},
        Command::Prepare{request}=>{compatible_target(p,&request.target)?;
            let plan=blocking(move||store.context_prepare(&actor,request)).await?;
            Ok(json!({"input_sha256":plan.digest()?,"plan":plan}))},
        Command::Plan{plan_id}=>{let plan=blocking(move||store.context_plan(&actor,&plan_id)).await?;Ok(json!({"input_sha256":plan.digest()?,"plan":plan}))},
        Command::Manual{plan_id,request_id,text,from}=>blocking(move||value(store.context_manual(&actor,&plan_id,&request_id,&text,from)?)).await,
        Command::Operation{id}=>blocking(move||value(store.context_operation(&actor,&id)?)).await,
        Command::Artifact{artifact_id}=>blocking(move||value(store.context_artifact(&actor,&artifact_id)?)).await,
        Command::Apply{request}=>{
            compatible_target(p,&request.target)?;
            let result=blocking(move||store.context_apply(&actor,request)).await?;
            let prior=p.clone();let session=result.session.id.clone();let branch=result.branch.id.clone();
            let new=blocking(move||prior.open_session(&session,&branch)).await?;
            let load=new.clone();let (history,journal)=blocking(move||Ok((load.view()?.history,load.handle))).await?;
            agent.load_history(history,Some(std::sync::Arc::new(journal)))?;*p=new;
            value(result)
        },
        Command::Application{request_id}=>blocking(move||value(store.context_application(&actor,&request_id)?)).await,
        Command::Pending=>{let session=p.handle.session_id().to_owned();let branch=p.handle.branch()?.id;
            blocking(move||value(store.context_pending(&actor,&session,&branch)?)).await},
        Command::DiscardPending{id}=>blocking(move||{store.context_discard_pending(&actor,&id)?;Ok(json!({"discarded":true}))}).await,
        Command::GrantRead{session_id,principal,allow,summary_accounts}=>{
            let principal=PrincipalId::new(principal)?;
            blocking(move||{store.context_grant_read(&actor,&session_id,&principal,allow,summary_accounts)?;
                Ok(json!({"read_grant":allow,"append_grant":false,"note":"read access is not provider or destination disclosure authority"}))}).await
        },
        Command::Generate{plan_id,request_id,account_id,model,confirmed_disclosure}=>{
            if !confirmed_disclosure{return Err(Error::Denied("confirm exact source evidence disclosure to the selected summarizer"));}
            let selected=agent.provider_accounts().select(&actor,&account_id,&model)?;
            let identity=selected.identity().clone();
            let existing=blocking({let store=store.clone();let actor=actor.clone();let plan_id=plan_id.clone();let request_id=request_id.clone();move||store.context_generation_status(&actor,&plan_id,&request_id,&identity)}).await?;
            if let Some(operation)=existing{return value(operation);}
            let executor=agent.summary_executor()?;
            let plan=blocking({let s=store.clone();let a=actor.clone();let id=plan_id.clone();move||s.context_plan(&a,&id)}).await?;
            compatible_target(p,&plan.target)?;
            let journal=blocking({let s=store.clone();let a=actor.clone();let id=plan_id.clone();let request=request_id.clone();move||s.summary_journal(&a,&id,&request)}).await?;
            let result=executor.execute(&selected,&plan.input()?,&journal,cancel).await;
            // A summary consumes a global invocation ID, not a chat turn. Always
            // reload the next ID after settlement or failure; no stale local IDs.
            let prior=p.clone();let session=p.handle.session_id().to_owned();let branch=p.handle.branch()?.id;
            let fresh=blocking(move||prior.open_session(&session,&branch)).await;
            match fresh {
                Ok(new)=>{let load=new.clone();let (history,handle)=blocking(move||Ok((load.view()?.history,load.handle))).await?;
                    agent.load_history(history,Some(std::sync::Arc::new(handle)))?;*p=new;},
                Err(e)=>return Err(e), // denied derived history must not be returned
            }
            result?;
            blocking(move||value(store.context_operation(&actor,&request_id)?)).await
        },
    }
}
