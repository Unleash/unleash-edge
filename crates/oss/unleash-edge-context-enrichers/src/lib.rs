//! Context enrichment support for Unleash Edge.

pub mod pool;
pub mod protocol;
pub mod worker;

pub use pool::WorkerPool;
pub use worker::{EnrichmentError, NodeWorker};

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, time::Instant};

    use tokio::{
        task::JoinSet,
        time::{Duration, sleep},
    };

    use super::*;

    fn script_path(script_name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("scripts")
            .join(script_name)
    }

    async fn wait_for_pid_change(worker: &NodeWorker, old_pid: u32) -> Result<(), EnrichmentError> {
        for _ in 0..40 {
            let new_pid = worker.pid();
            if new_pid != 0 && new_pid != old_pid {
                return Ok(());
            }
            sleep(Duration::from_millis(25)).await;
        }

        Err(EnrichmentError::WorkerUnavailable(
            "worker PID did not change after timeout".into(),
        ))
    }

    #[tokio::test]
    async fn responses_are_correlated_by_id() -> Result<(), EnrichmentError> {
        let worker = NodeWorker::start(
            10,
            script_path("worker.js"),
            script_path("example-enricher.js"),
        )
        .await?;
        let mut join_set = JoinSet::new();

        for (user_id, delay_ms) in [("alice", 120_u64), ("bob", 20), ("carol", 80), ("dave", 5)] {
            let worker = worker.clone();
            join_set.spawn(async move {
                let result = worker
                    .execute(
                        serde_json::json!({
                            "userId": user_id,
                            "properties": { "delayMs": delay_ms }
                        }),
                        Duration::from_secs(2),
                    )
                    .await?;
                Ok::<_, EnrichmentError>((user_id, result))
            });
        }

        while let Some(result) = join_set.join_next().await {
            let (expected_user_id, context) =
                result.map_err(|error| EnrichmentError::WorkerUnavailable(error.to_string()))??;
            assert_eq!(context["userId"], expected_user_id);
            assert_eq!(context["properties"]["enriched"], true);
        }

        Ok(())
    }

    #[tokio::test]
    async fn work_is_multiplexed_on_one_worker() -> Result<(), EnrichmentError> {
        let worker = NodeWorker::start(
            11,
            script_path("worker.js"),
            script_path("example-enricher.js"),
        )
        .await?;
        let started = Instant::now();
        let mut join_set = JoinSet::new();

        for user_id in ["one", "two", "three"] {
            let worker = worker.clone();
            join_set.spawn(async move {
                worker
                    .execute(
                        serde_json::json!({
                            "userId": user_id,
                            "properties": { "delayMs": 200 }
                        }),
                        Duration::from_secs(2),
                    )
                    .await
            });
        }

        while let Some(result) = join_set.join_next().await {
            result.map_err(|error| EnrichmentError::WorkerUnavailable(error.to_string()))??;
        }

        assert!(
            started.elapsed() < Duration::from_millis(500),
            "three 200ms jobs should multiplex below 500ms, elapsed {:?}",
            started.elapsed()
        );

        Ok(())
    }

    #[tokio::test]
    async fn timeout_restarts_worker() -> Result<(), EnrichmentError> {
        let worker = NodeWorker::start(
            12,
            script_path("worker.js"),
            script_path("timeout-enricher.js"),
        )
        .await?;
        let old_pid = worker.pid();

        let result = worker
            .execute(
                serde_json::json!({
                    "userId": "timeout",
                    "properties": { "hang": true }
                }),
                Duration::from_millis(100),
            )
            .await;

        assert!(matches!(result, Err(EnrichmentError::Timeout)));
        wait_for_pid_change(&worker, old_pid).await?;
        assert_ne!(old_pid, worker.pid());

        let result = worker
            .execute(
                serde_json::json!({
                    "userId": "recovered",
                    "properties": {}
                }),
                Duration::from_secs(2),
            )
            .await?;
        assert_eq!(result["properties"]["enriched"], true);

        Ok(())
    }

    #[tokio::test]
    async fn customer_console_stdout_does_not_corrupt_protocol() -> Result<(), EnrichmentError> {
        let worker = NodeWorker::start(
            13,
            script_path("worker.js"),
            script_path("example-enricher.js"),
        )
        .await?;

        let result = worker
            .execute(
                serde_json::json!({
                    "userId": "logs-to-console",
                    "properties": {
                        "delayMs": 1,
                        "log": true
                    }
                }),
                Duration::from_secs(2),
            )
            .await?;

        assert_eq!(result["userId"], "logs-to-console");
        assert_eq!(result["properties"]["enriched"], true);

        Ok(())
    }
}
