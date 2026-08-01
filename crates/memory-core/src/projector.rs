//! Core 对持久化 State 与 Graph Projector 的 deadline 感知门面。

use std::{future::Future, time::Instant};

use jc_memory_protocol::MemoryScope;
use jc_memory_storage::ProjectionTarget;

use crate::{MemoryEngine, MemoryError};

impl MemoryEngine {
    pub(crate) async fn catch_up_projections(
        &self,
        scope: &MemoryScope,
        deadline: Instant,
    ) -> Result<ProjectionTarget, MemoryError> {
        let target = self.projection.target(scope).await?;
        with_deadline(deadline, self.projection.catch_up_state(scope, target, 128)).await?;
        with_deadline(deadline, self.projection.catch_up_graph(scope, target, 128)).await?;
        Ok(target)
    }
}

async fn with_deadline<T, E, F>(deadline: Instant, future: F) -> Result<T, MemoryError>
where
    E: Into<MemoryError>,
    F: Future<Output = Result<T, E>>,
{
    tokio::time::timeout_at(deadline.into(), future)
        .await
        .map_err(|_| MemoryError::ProjectionTimedOut)?
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::with_deadline;

    #[tokio::test]
    async fn projection_deadline_has_a_distinct_error_category() {
        let result = with_deadline(Instant::now() + Duration::from_millis(1), async {
            tokio::time::sleep(Duration::from_millis(20)).await;
            Ok::<_, crate::MemoryError>(())
        })
        .await;

        assert!(matches!(
            result,
            Err(crate::MemoryError::ProjectionTimedOut)
        ));
    }
}
