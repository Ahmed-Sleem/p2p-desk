use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tokio::sync::Mutex;

use crate::policy::{MINIMUM_RATE_LIMIT_CIRCUIT, MINIMUM_WAF_CIRCUIT};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CircuitReason {
    RateLimited,
    WafOrBan,
    SchemaContract,
    SideContract,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum CircuitState {
    Closed,
    Timed {
        reason: CircuitReason,
        retry_at_ms: i64,
    },
    Persistent {
        reason: CircuitReason,
    },
}

#[derive(Clone, Debug)]
pub struct CircuitBreaker {
    state: Arc<Mutex<CircuitState>>,
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self::new()
    }
}

impl CircuitBreaker {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(CircuitState::Closed)),
        }
    }

    pub async fn state(&self) -> CircuitState {
        let mut state = self.state.lock().await;
        if let CircuitState::Timed { retry_at_ms, .. } = *state
            && now_ms() >= retry_at_ms
        {
            *state = CircuitState::Closed;
        }
        *state
    }

    pub async fn ensure_available(&self) -> Result<(), CircuitState> {
        match self.state().await {
            CircuitState::Closed => Ok(()),
            state => Err(state),
        }
    }

    pub async fn open_rate_limit(&self, retry_after: Option<Duration>) -> CircuitState {
        self.open_timed(
            CircuitReason::RateLimited,
            retry_after
                .unwrap_or(MINIMUM_RATE_LIMIT_CIRCUIT)
                .max(MINIMUM_RATE_LIMIT_CIRCUIT),
        )
        .await
    }

    pub async fn open_waf_or_ban(&self, retry_after: Option<Duration>) -> CircuitState {
        self.open_timed(
            CircuitReason::WafOrBan,
            retry_after
                .unwrap_or(MINIMUM_WAF_CIRCUIT)
                .max(MINIMUM_WAF_CIRCUIT),
        )
        .await
    }

    pub async fn open_persistent(&self, reason: CircuitReason) -> CircuitState {
        debug_assert!(matches!(
            reason,
            CircuitReason::SchemaContract | CircuitReason::SideContract
        ));
        let state = CircuitState::Persistent { reason };
        *self.state.lock().await = state;
        state
    }

    /// Only an explicit successful diagnostic may clear a persistent contract circuit.
    pub async fn close_after_successful_diagnostic(&self) {
        *self.state.lock().await = CircuitState::Closed;
    }

    async fn open_timed(&self, reason: CircuitReason, duration: Duration) -> CircuitState {
        let retry_at_ms = now_ms().saturating_add(duration_to_i64_ms(duration));
        let mut current = self.state.lock().await;
        let state = match *current {
            CircuitState::Persistent { .. } => *current,
            CircuitState::Timed {
                retry_at_ms: existing,
                ..
            } if existing >= retry_at_ms => *current,
            _ => CircuitState::Timed {
                reason,
                retry_at_ms,
            },
        };
        *current = state;
        state
    }
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, duration_to_i64_ms)
}

fn duration_to_i64_ms(duration: Duration) -> i64 {
    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rate_and_waf_circuits_apply_approved_minimums() {
        let breaker = CircuitBreaker::new();
        let before = now_ms();
        let rate = breaker.open_rate_limit(Some(Duration::from_secs(1))).await;
        let CircuitState::Timed { retry_at_ms, .. } = rate else {
            panic!("expected timed circuit");
        };
        assert!(retry_at_ms >= before + 60_000);

        breaker.close_after_successful_diagnostic().await;
        let before = now_ms();
        let waf = breaker.open_waf_or_ban(None).await;
        let CircuitState::Timed { retry_at_ms, .. } = waf else {
            panic!("expected timed circuit");
        };
        assert!(retry_at_ms >= before + 900_000);
    }

    #[tokio::test]
    async fn persistent_contract_circuit_requires_explicit_diagnostic_close() {
        let breaker = CircuitBreaker::new();
        breaker.open_persistent(CircuitReason::SchemaContract).await;
        assert_eq!(
            breaker.ensure_available().await,
            Err(CircuitState::Persistent {
                reason: CircuitReason::SchemaContract
            })
        );
        breaker.close_after_successful_diagnostic().await;
        assert_eq!(breaker.ensure_available().await, Ok(()));
    }
}
