//! Shared state for commands that talk to the API: one resolved key and
//! client per auth chain, live profile fetches, and snapshots.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

use miette::Report;
use ndac_api::{Client, Error as ApiError, Profile};
use ndac_core::{
    auth::{self, SystemEnv},
    client::api_client,
    config::{DesiredProfile, Workspace},
    plan::{self, Plan, ProfilePlan, RemovedProfile},
    snapshot::{SnapshotKind, SnapshotStore},
};
use tokio::{runtime::Runtime, task::JoinSet};

use super::{Failure, cwd, runtime, user_agent};
use crate::{cli::GlobalArgs, output::Output};

pub(crate) struct Session<'a> {
    pub(crate) ws: Workspace,
    pub(crate) rt: Runtime,
    pub(crate) store: SnapshotStore,
    global: &'a GlobalArgs,
    cwd: PathBuf,
    clients: BTreeMap<String, Client>,
    warned: BTreeSet<String>,
}

impl<'a> Session<'a> {
    pub(crate) fn new(ws: Workspace, global: &'a GlobalArgs) -> Result<Self, Failure> {
        let store = SnapshotStore::new(&ws.root_dir, &ws.provider());
        Ok(Self {
            ws,
            rt: runtime()?,
            store,
            global,
            cwd: cwd()?,
            clients: BTreeMap::new(),
            warned: BTreeSet::new(),
        })
    }

    /// The client for `profile`'s auth chain. Profiles sharing a chain share
    /// one resolution, so a `cmd:` source prompts at most once.
    pub(crate) fn client(&mut self, profile: &str) -> Result<Client, Failure> {
        let chain = auth::chain_for(&self.ws, Some(profile), &self.global.auth_source)?;
        let cache_key = chain
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        if let Some(c) = self.clients.get(&cache_key) {
            return Ok(c.clone());
        }
        let ctx = auth::context(
            &self.ws,
            Some(profile),
            &self.cwd,
            self.global.allow_tracked_secret,
            &SystemEnv,
        );
        let resolved = auth::resolve(&chain, &ctx, &SystemEnv)?;
        for w in resolved.warnings {
            if self.warned.insert(w.clone()) {
                Output::warn(&w);
            }
        }
        let client =
            api_client(&self.ws.provider(), resolved.key, &user_agent()).map_err(Failure::api)?;
        self.clients.insert(cache_key, client.clone());
        Ok(client)
    }

    /// Fetches the live state of every profile that has an ID, concurrently
    /// (the client caps requests in flight).
    pub(crate) fn fetch_live(
        &mut self,
        desired: &[DesiredProfile],
    ) -> Result<Vec<Option<Profile>>, Failure> {
        let mut jobs = Vec::new();
        for (i, d) in desired.iter().enumerate() {
            if let Some(id) = &d.id {
                jobs.push((i, self.client(&d.key)?, id.clone(), d.key.clone()));
            }
        }
        let mut live: Vec<Option<Profile>> = vec![None; desired.len()];
        let results = self.rt.block_on(async move {
            let mut set = JoinSet::new();
            for (i, client, id, key) in jobs {
                set.spawn(async move { (i, key, id.clone(), client.get_profile(&id).await) });
            }
            let mut out = Vec::new();
            while let Some(r) = set.join_next().await {
                out.push(r);
            }
            out
        });
        for r in results {
            let (i, key, id, result) =
                r.map_err(|e| Failure::Other(Report::msg(format!("task failed: {e}"))))?;
            match result {
                Ok(p) => live[i] = Some(p),
                Err(ApiError::NotFound { .. }) => {
                    return Err(Failure::Api(Report::msg(format!(
                        "profile \"{key}\": NextDNS profile {id} does not exist; remove `id` to create it, or fix the ID"
                    ))));
                }
                Err(e) => return Err(Failure::api(e)),
            }
        }
        Ok(live)
    }

    /// Diffs and classifies every profile against its live state and the
    /// latest post-apply snapshot.
    pub(crate) fn plan(
        &self,
        desired: &[DesiredProfile],
        live: &[Option<Profile>],
    ) -> Result<Plan, Failure> {
        let mut profiles: Vec<ProfilePlan> = Vec::new();
        for (d, l) in desired.iter().zip(live) {
            let mut p = plan::diff(d, l.as_ref());
            let snap = self
                .store
                .latest(&d.key, SnapshotKind::PostApply)
                .map_err(|e| Failure::Other(Report::msg(format!("cannot read snapshots: {e}"))))?;
            // A snapshot of another profile ID (the file now points
            // elsewhere) is not a valid baseline.
            let snap = snap.filter(|s| d.id.as_deref() == Some(s.id.as_str()));
            plan::classify(&mut p, d, l.as_ref(), snap.as_ref().map(|s| &s.data));
            profiles.push(p);
        }
        let configured: BTreeSet<&str> = self.ws.profiles.keys().map(String::as_str).collect();
        let mut removed = Vec::new();
        let known = self
            .store
            .profiles()
            .map_err(|e| Failure::Other(Report::msg(format!("cannot read snapshots: {e}"))))?;
        for key in known
            .into_iter()
            .filter(|k| !configured.contains(k.as_str()))
        {
            let id = self
                .store
                .list(Some(&key))
                .ok()
                .and_then(|l| l.last().cloned())
                .and_then(|e| self.store.load(&e).ok())
                .map(|s| s.id);
            removed.push(RemovedProfile { key, id });
        }
        Ok(Plan { profiles, removed })
    }
}
