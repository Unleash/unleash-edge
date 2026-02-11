use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::{pin::Pin, sync::Arc};
use tokio::sync::watch::Sender;
use tracing::warn;
use ulid::Ulid;
use unleash_edge_cli::EdgeArgs;
use unleash_edge_http_client::UnleashClient;
use unleash_edge_persistence::EdgePersistence;
use unleash_edge_types::{
    EdgeResult, RefreshState,
    enterprise::{ApplicationLicenseState, LicenseState},
    errors::EdgeError,
    tokens::EdgeToken,
};

pub fn create_enterprise_heartbeat_task(
    unleash_client: Arc<UnleashClient>,
    token: EdgeToken,
    refresh_state_tx: Sender<RefreshState>,
    connection_id: Ulid,
    app_license_state: ApplicationLicenseState,
    persistence: Option<Arc<dyn EdgePersistence>>,
) -> Pin<Box<dyn Future<Output = ()> + Send>> {
    Box::pin(async move {
        let sleep_duration = tokio::time::Duration::from_secs(90);
        loop {
            tokio::time::sleep(sleep_duration).await;
            let license_state = unleash_client
                .send_heartbeat(&token.clone(), &connection_id)
                .await;

            if let Ok(new_state) = license_state {
                app_license_state.set(new_state);
                if new_state == LicenseState::Invalid {
                    warn!(
                        "Edge license is invalid, features will not be refreshed until this is resolved. This needs to be fixed in your upstream Unleash instance."
                    );
                }

                let _ = refresh_state_tx.send(new_state.into());

                if let Some(persistence) = &persistence {
                    let _ = persistence.save_license_state(&new_state).await;
                }
            }
        }
    })
}

pub fn enforce_single_backend_token_per_env(args: &EdgeArgs) -> EdgeResult<()> {
    if !(args.delta || args.streaming) {
        return Ok(());
    }

    let mut tokens_by_env: HashMap<String, Vec<EdgeToken>> = HashMap::new();
    for token_string in &args.tokens {
        if let Ok(token) = EdgeToken::from_str(token_string) {
            if let Some(environment) = token.environment.clone() {
                tokens_by_env.entry(environment).or_default().push(token);
            }
        }
    }

    let mut offenders: Vec<(String, Vec<EdgeToken>)> = tokens_by_env
        .into_iter()
        .filter(|(_, tokens)| tokens.len() > 1)
        .collect();
    if offenders.is_empty() {
        return Ok(());
    }

    offenders.sort_by(|left, right| left.0.cmp(&right.0));

    let mut lines = Vec::new();
    lines.push("In DELTA/STREAMING mode, only one token per environment is allowed.".to_string());
    for (environment, tokens) in offenders {
        let mut projects: HashSet<String> = HashSet::new();
        for token in tokens {
            for project in token.projects.iter() {
                projects.insert(project.clone());
            }
        }
        let mut project_list: Vec<String> = projects.into_iter().collect();
        project_list.sort();
        let project_hint = if project_list.is_empty() || project_list.iter().any(|p| p == "*") {
            "*".to_string()
        } else {
            project_list.join(", ")
        };

        lines.push(format!(
            "Provide a single merged-scope token for {environment} covering projects: {project_hint} (or * if intended)."
        ));
    }

    Err(EdgeError::InvalidTokenConfig(lines.join("\n")))
}
