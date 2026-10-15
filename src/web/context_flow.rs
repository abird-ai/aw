//! Authenticated context commands. Generation is a separate background task,
//! never a fake chat prompt or an unrestricted tool-bearing Agent run.
use super::*;
use crate::context_flow::{self, Command};
use std::collections::BTreeMap;
#[derive(Clone)]
pub(super) struct Job {
    pub id: String,
    pub fingerprint: String,
    pub cancel: CancellationToken,
    pub result: Option<std::result::Result<serde_json::Value, String>>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RequestBody {
    pub expected_revision: u64,
    pub command: Command,
}

pub(super) async fn script() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        include_str!("../../web/context-flow.js"),
    )
}
/// Minimal authorized recovery metadata; never returns protected source content.
pub(super) async fn recovery(State(app): State<AppState>) -> Json<serde_json::Value> {
    let state = lock(&app.0.session.state);
    Json(
        serde_json::json!({"revision":state.snapshot.revision,"busy":state.snapshot.pending.is_some()}),
    )
}
pub(super) async fn recipients(State(app): State<AppState>) -> Json<BTreeMap<String, String>> {
    Json(lock(&app.0.context_recipients).clone())
}
pub(super) async fn cancel_job(
    State(app): State<AppState>,
    payload: std::result::Result<Json<CancelJob>, axum::extract::rejection::JsonRejection>,
) -> ApiResult<Json<serde_json::Value>> {
    let Json(body) =
        payload.map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid summary cancellation"))?;
    let job = lock(&app.0.context_job);
    if let Some(job) = job.as_ref().filter(|j| j.id == body.request_id) {
        job.cancel.cancel();
    }
    Ok(Json(
        serde_json::json!({"requested":true,"request_id":body.request_id}),
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CancelJob {
    pub(super) request_id: String,
}

pub(super) async fn action(
    State(app): State<AppState>,
    payload: std::result::Result<Json<RequestBody>, axum::extract::rejection::JsonRejection>,
) -> ApiResult<Response> {
    let Json(body) =
        payload.map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid context command"))?;
    let p = current_persistence(&app)?;
    // Reads require no Agent lock and can observe generation in progress.
    let store = p.store.clone();
    let actor = p.actor.clone();
    let read=match &body.command {
        Command::Sources=>Some(store_work(move||{
            let values=store.list(&actor)?;
            Ok(serde_json::Value::Array(values.into_iter().map(|s|serde_json::json!({"id":s.id,"title":s.title,"owner":s.owner,"imported":s.imported})).collect()))
        }).await?),
        Command::Targets=>{let binding=p.binding.clone();Some(store_work(move||{
            let values=store.list(&actor)?.into_iter().filter(|s|s.owner==actor&&crate::sessions::check_binding(&binding,&s.binding,false).is_ok());
            Ok(serde_json::Value::Array(values.map(|s|serde_json::json!({"id":s.id,"title":s.title,"owner":s.owner})).collect()))
        }).await?)},
        Command::TargetAt{session_id,branch}=>{let id=session_id.clone();let branch=branch.clone();let binding=p.binding.clone();let owned=app.0.owned_sessions_only;
            Some(store_work(move||{let session=store.session(&actor,&id)?;
                if owned&&session.owner!=actor{return Err(Error::Denied("team destination must be owned"));}
                crate::sessions::check_binding(&binding,&session.binding,false)?;
                Ok(serde_json::json!(store.context_target(&actor,&id,&branch)?))
            }).await?)},
        Command::Branches{session_id}=>{let id=session_id.clone();Some(store_work(move||Ok(serde_json::json!(store.branches(&actor,&id)?))).await?)},
        Command::Target=>{let id=p.handle.session_id().to_owned();let branch=p.handle.branch().map_err(api_error)?.id;
            Some(store_work(move||Ok(serde_json::json!(store.context_target(&actor,&id,&branch)?))).await?)},
        Command::Plan{plan_id}=>{let id=plan_id.clone();Some(store_work(move||{let plan=store.context_plan(&actor,&id)?;Ok(serde_json::json!({"input_sha256":plan.digest()?,"plan":plan}))}).await?)},
        Command::Artifact{artifact_id}=>{let id=artifact_id.clone();Some(store_work(move||Ok(serde_json::json!(store.context_artifact(&actor,&id)?))).await?)},
        Command::Application{request_id}=>{let id=request_id.clone();Some(store_work(move||Ok(serde_json::json!(store.context_application(&actor,&id)?))).await?)},
        Command::Pending=>{let id=p.handle.session_id().to_owned();let branch=p.handle.branch().map_err(api_error)?.id;
            Some(store_work(move||Ok(serde_json::json!(store.context_pending(&actor,&id,&branch)?))).await?)},
        Command::Operation{id}=>{
            let remembered=lock(&app.0.context_job).as_ref().filter(|j|j.id==*id).cloned();
            if let Some(job)=remembered {
                if let Some(result)=job.result {match result {
                    Ok(_)=>{let request=id.clone();let value=store_work(move||Ok(serde_json::json!(store.context_operation(&actor,&request)?))).await?;return Ok(Json(value).into_response());},
                    Err(_)=>return Ok(Json(serde_json::json!({"request_id":id,"state":"failed","error":"summary operation failed or access changed; inspect permitted receipt"})).into_response())
                }}
                return Ok(Json(serde_json::json!({"request_id":id,"state":"started"})).into_response());
            }
            let id=id.clone();Some(store_work(move||Ok(serde_json::json!(store.context_operation(&actor,&id)?))).await?)
        },
        _=>None,
    };
    if let Some(value) = read {
        return Ok(Json(value).into_response());
    }
    let selected_target = match &body.command {
        Command::Prepare { request } => Some(request.target.session_id.clone()),
        Command::Apply { request } => Some(request.target.session_id.clone()),
        Command::Generate { plan_id, .. } => {
            let store = p.store.clone();
            let actor = p.actor.clone();
            let id = plan_id.clone();
            Some(
                store_work(move || {
                    store
                        .context_plan(&actor, &id)
                        .map(|plan| plan.target.session_id)
                })
                .await?,
            )
        }
        _ => None,
    };
    if app.0.owned_sessions_only {
        if let Some(id) = selected_target {
            let store = p.store.clone();
            let actor = p.actor.clone();
            store_work(move || {
                if store.session(&actor, &id)?.owner != actor {
                    return Err(Error::Denied("team destination must remain owned"));
                }
                Ok(())
            })
            .await?;
        }
    }
    let _admission = app.0.session.admission.lock().await;
    if !auth_alive(&app) {
        return Err(ApiError(StatusCode::UNAUTHORIZED, "session expired"));
    }
    if let Command::Generate { request_id, .. } = &body.command {
        let fp = format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&body.command)
                    .map_err(|_| ApiError(StatusCode::BAD_REQUEST, "invalid summary command"))?
            )
        );
        let prior = lock(&app.0.context_job).clone();
        if let Some(job) = prior.filter(|j| j.id == *request_id) {
            if job.fingerprint != fp {
                return Err(ApiError(
                    StatusCode::CONFLICT,
                    "summary request ID has another input",
                ));
            }
            return Ok((StatusCode::ACCEPTED,Json(serde_json::json!({"request_id":request_id,"state":"accepted","lookup_only":true}))).into_response());
        }
        idle_revision(&app, body.expected_revision)?;
        // Destination must be the actor's owned active session in team mode.
        ensure_context_owner(&app, &p).await?;
        let token = CancellationToken::new();
        let req = request_id.clone();
        let accepted_id = req.clone();
        *lock(&app.0.context_job) = Some(Job {
            id: req.clone(),
            fingerprint: fp,
            cancel: token.clone(),
            result: None,
        });
        let owner = app.clone();
        let command = body.command;
        tokio::spawn(async move {
            let mut agent = owner.0.session.agent.lock().await;
            let mut current = p;
            let task=std::panic::AssertUnwindSafe(context_flow::execute(&mut agent,&mut current,command,async {
                loop {tokio::select!{_=token.cancelled()=>break,_=owner.0.shutdown.cancelled()=>break,
                    _=tokio::time::sleep(Duration::from_millis(500))=>{if !auth_alive(&owner){break;}}}}
            })).catch_unwind().await;
            let result = match task {
                Ok(r) => r,
                Err(_) => Err(Error::Storage(
                    "summary worker panicked; inspect durable operation status before any retry",
                )),
            };
            *lock(&owner.0.session.persistence) = Some(current.clone());
            let saved = store_work(move || saved_selection(&current)).await;
            if let Ok(saved) = saved {
                update_projection(&owner, &agent, Some(saved), false);
            }
            let mut job = lock(&owner.0.context_job);
            if let Some(j) = job.as_mut().filter(|j| j.id == req) {
                j.result = Some(result.map_err(|e| e.to_string()));
            }
        });
        return Ok((
            StatusCode::ACCEPTED,
            Json(serde_json::json!({"request_id":accepted_id,"state":"accepted"})),
        )
            .into_response());
    }
    idle_revision(&app, body.expected_revision)?;
    ensure_context_owner(&app, &p).await?;
    if let Command::GrantRead {
        principal,
        summary_accounts,
        ..
    } = &body.command
    {
        // Team source releases are restricted to currently configured identities,
        // not arbitrary actor strings supplied by a browser.
        if !matches!(app.1, Authority::Local(_))
            && !lock(&app.0.context_recipients).contains_key(principal)
        {
            return Err(ApiError(
                StatusCode::FORBIDDEN,
                "recipient is not an enabled participant",
            ));
        }
        if summary_accounts.len() > 16 {
            return Err(ApiError(
                StatusCode::BAD_REQUEST,
                "too many summary-account grants",
            ));
        }
    }
    let changes_view = body.command.mutates_view();
    let mut agent = app.0.session.agent.lock().await;
    let mut current = p;
    let result = context_flow::execute(
        &mut agent,
        &mut current,
        body.command,
        std::future::pending::<()>(),
    )
    .await
    .map_err(api_error)?;
    if changes_view {
        let saved = store_work({
            let p = current.clone();
            move || saved_selection(&p)
        })
        .await?;
        *lock(&app.0.session.persistence) = Some(current);
        update_projection(&app, &agent, Some(saved), false);
    }
    Ok(Json(result).into_response())
}
async fn ensure_context_owner(app: &AppState, p: &PersistentSession) -> ApiResult<()> {
    if app.0.owned_sessions_only {
        let p = p.clone();
        store_work(move || {
            if p.store.session(&p.actor, p.handle.session_id())?.owner != p.actor {
                return Err(Error::Denied("team target must remain an owned session"));
            }
            Ok(())
        })
        .await?;
    }
    Ok(())
}
/// Gate cached snapshot/SSE data against source revocation. A protected client
/// gets a clear-and-close event, not a stale cached transcript. Delivered data
/// cannot be recalled; this controls future disclosures.
pub(super) async fn visible(app: &AppState) -> ApiResult<()> {
    let p = lock(&app.0.session.persistence).clone();
    if let Some(p) = p {
        let session = p.handle.session_id().to_owned();
        store_work(move || p.store.session(&p.actor, &session).map(|_| ())).await?;
    }
    Ok(())
}
