use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use p2p_domain::{MarketPair, PaymentLogic, RequestSide, ResultsTarget, StableId, Symbol};
use serde::Serialize;
use thiserror::Error;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::agent::{AgentError, AgentMetadataClient, AgentQuote, AgentTradeMethods};
use crate::catalog::VerifiedPair;
use crate::circuit::{CircuitBreaker, CircuitState, now_ms};
use crate::scheduler::{
    Acquisition, AcquisitionProgress, AcquisitionRequest, ProviderError, ProviderService,
};
use crate::transport::{GlobalRequestGate, ReqwestPageTransport};

#[derive(Clone)]
pub struct LiveProviderRuntime {
    primary: ProviderService<ReqwestPageTransport>,
    agent: AgentMetadataClient,
    operation_lock: Arc<Mutex<()>>,
    request_sequence: Arc<AtomicU64>,
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum RuntimeBuildError {
    #[error("primary provider HTTP client could not be built")]
    PrimaryTransport,
    #[error("Agent metadata HTTP client could not be built")]
    AgentTransport,
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum PairCheckError {
    #[error("pair symbols are not canonical or distinct")]
    InvalidPair,
    #[error("two-side primary validation failed")]
    Primary(ProviderError),
    #[error("validated pair catalog record could not be constructed")]
    Catalog,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairCheckResult {
    pub verified: VerifiedPair,
    pub acquisition: Acquisition,
    pub agent_trade_methods: Option<AgentTradeMethods>,
    pub agent_warning: Option<&'static str>,
}

impl LiveProviderRuntime {
    pub fn new() -> Result<Self, RuntimeBuildError> {
        let gate = GlobalRequestGate::production();
        let circuit = CircuitBreaker::new();
        let transport =
            ReqwestPageTransport::new().map_err(|_| RuntimeBuildError::PrimaryTransport)?;
        let primary = ProviderService::with_shared(transport, gate.clone(), circuit.clone());
        let agent = AgentMetadataClient::new(gate, circuit)
            .map_err(|_| RuntimeBuildError::AgentTransport)?;
        Ok(Self {
            primary,
            agent,
            operation_lock: Arc::new(Mutex::new(())),
            request_sequence: Arc::new(AtomicU64::new(1)),
        })
    }

    pub fn next_request_id(&self) -> StableId {
        let sequence = self.request_sequence.fetch_add(1, Ordering::Relaxed);
        StableId::new(format!("request-{}-{sequence}", now_ms()))
            .expect("generated request ID is bounded printable ASCII")
    }

    pub async fn acquire<F>(
        &self,
        request: AcquisitionRequest,
        cancellation: CancellationToken,
        on_progress: F,
    ) -> Result<Acquisition, ProviderError>
    where
        F: Fn(AcquisitionProgress) + Send + Sync,
    {
        let _operation = tokio::select! {
            () = cancellation.cancelled() => return Err(ProviderError::Cancelled),
            guard = self.operation_lock.lock() => guard,
        };
        self.primary
            .acquire(request, cancellation, on_progress)
            .await
    }

    pub async fn check_pair<F>(
        &self,
        asset: &str,
        fiat: &str,
        cancellation: CancellationToken,
        on_progress: F,
    ) -> Result<PairCheckResult, PairCheckError>
    where
        F: Fn(AcquisitionProgress) + Send + Sync,
    {
        let asset = Symbol::new(asset).map_err(|_| PairCheckError::InvalidPair)?;
        let fiat = Symbol::new(fiat).map_err(|_| PairCheckError::InvalidPair)?;
        let pair = MarketPair::new(asset, fiat).map_err(|_| PairCheckError::InvalidPair)?;
        let _operation = tokio::select! {
            () = cancellation.cancelled() => {
                return Err(PairCheckError::Primary(ProviderError::Cancelled));
            }
            guard = self.operation_lock.lock() => guard,
        };

        let prior_persistent = match self.primary.circuit().state().await {
            CircuitState::Persistent { reason } => {
                self.primary
                    .circuit()
                    .close_after_successful_diagnostic()
                    .await;
                Some(reason)
            }
            _ => None,
        };
        let request = AcquisitionRequest {
            request_id: self.next_request_id(),
            pair: pair.clone(),
            transaction_amount: None,
            selected_payment_methods: BTreeSet::new(),
            payment_logic: PaymentLogic::Any,
            target: ResultsTarget::new(20).expect("approved minimum target"),
        };
        let acquisition = match self
            .primary
            .acquire(request, cancellation.clone(), on_progress)
            .await
        {
            Ok(value) => value,
            Err(error) => {
                if let Some(reason) = prior_persistent {
                    self.primary.circuit().open_persistent(reason).await;
                }
                return Err(PairCheckError::Primary(error));
            }
        };
        let verified = VerifiedPair::from_acquisition(&acquisition, now_ms())
            .map_err(|_| PairCheckError::Catalog)?;
        let (agent_trade_methods, agent_warning) = match self
            .agent
            .trade_methods(pair.fiat().as_str(), cancellation)
            .await
        {
            Ok(methods) => (Some(methods), None),
            Err(AgentError::Cancelled) => {
                return Err(PairCheckError::Primary(ProviderError::Cancelled));
            }
            Err(error) => (None, Some(error.code())),
        };
        Ok(PairCheckResult {
            verified,
            acquisition,
            agent_trade_methods,
            agent_warning,
        })
    }

    pub async fn agent_quote(
        &self,
        pair: &MarketPair,
        request_side: RequestSide,
        cancellation: CancellationToken,
    ) -> Result<AgentQuote, AgentError> {
        let _operation = tokio::select! {
            () = cancellation.cancelled() => return Err(AgentError::Cancelled),
            guard = self.operation_lock.lock() => guard,
        };
        self.agent.quote(pair, request_side, cancellation).await
    }

    pub async fn circuit_state(&self) -> CircuitState {
        self.primary.circuit().state().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_request_ids_are_nonsecret_and_unique() {
        let sequence = AtomicU64::new(1);
        let first = sequence.fetch_add(1, Ordering::Relaxed);
        let second = sequence.fetch_add(1, Ordering::Relaxed);
        let first = StableId::new(format!("request-1-{first}")).expect("first");
        let second = StableId::new(format!("request-1-{second}")).expect("second");
        assert_ne!(first, second);
        assert!(!first.as_str().contains("USDT"));
    }
}
