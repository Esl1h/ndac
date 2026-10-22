//! Runs planned calls for one profile, in order, stopping at the first
//! error or when cancelled.

use std::sync::atomic::{AtomicBool, Ordering};

use ndac_api::{Client, Error};

use super::planner::ApiCall;

/// What happened while executing a profile's calls.
#[derive(Debug, Default)]
pub struct Outcome {
    /// Calls that completed.
    pub done: usize,
    /// ID returned by `POST /profiles`.
    pub created_id: Option<String>,
    /// The call that failed and why.
    pub error: Option<(String, Error)>,
    /// Stopped by Ctrl-C before every call ran.
    pub cancelled: bool,
}

/// Executes `calls` for profile `id` (`None` only before a create).
///
/// The in-flight call always completes; `cancel` is checked between calls.
pub async fn execute(
    client: &Client,
    id: Option<&str>,
    calls: &[ApiCall],
    cancel: &AtomicBool,
) -> Outcome {
    let mut out = Outcome::default();
    let mut id = id.map(str::to_owned);
    for call in calls {
        if cancel.load(Ordering::SeqCst) {
            out.cancelled = true;
            break;
        }
        tracing::info!(call = %call.describe(), "applying");
        let result = run(client, id.as_deref(), call).await;
        match result {
            Ok(Some(new_id)) => {
                id = Some(new_id.clone());
                out.created_id = Some(new_id);
                out.done += 1;
            }
            Ok(None) => out.done += 1,
            Err(e) => {
                out.error = Some((call.describe(), e));
                break;
            }
        }
    }
    out
}

async fn run(client: &Client, id: Option<&str>, call: &ApiCall) -> Result<Option<String>, Error> {
    if let ApiCall::CreateProfile { body } = call {
        return client.create_profile(body).await.map(Some);
    }
    // Every other call targets an existing profile; the planner only emits
    // them with an ID.
    let Some(id) = id else {
        return Err(Error::NotFound {
            endpoint: "/profiles/:profile",
        });
    };
    match call {
        ApiCall::CreateProfile { .. } => Ok(()),
        ApiCall::DeleteProfile => client.delete_profile(id).await,
        ApiCall::PatchProfile { body } => client.patch_profile(id, body).await,
        ApiCall::PatchObject { path, body } => client.patch_object(id, *path, body).await,
        ApiCall::PutList { list, items } => client.put_list(id, *list, items).await,
        ApiCall::AddItem {
            list,
            id: item,
            body,
        } => client.add_item(id, *list, item, body).await,
        ApiCall::PatchItem {
            list,
            id: item,
            body,
        } => client.patch_item(id, *list, item, body).await,
        ApiCall::DeleteItem { list, id: item } => client.delete_item(id, *list, item).await,
        #[cfg(feature = "unstable-endpoints")]
        ApiCall::AddRewrite { name, content } => client.add_rewrite(id, name, content).await,
        #[cfg(feature = "unstable-endpoints")]
        ApiCall::DeleteRewrite { name, content } => client.delete_rewrite(id, name, content).await,
    }
    .map(|()| None)
}
