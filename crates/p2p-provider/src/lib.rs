mod agent;
mod catalog;
mod circuit;
mod contract;
mod policy;
mod runtime;
mod scheduler;
mod transport;

pub use agent::{AgentError, AgentMetadataClient, AgentQuote, AgentTradeMethod, AgentTradeMethods};
pub use catalog::{DisabledPair, PairCatalogEntry, VerifiedPair};
pub use circuit::{CircuitBreaker, CircuitReason, CircuitState};
pub use contract::{
    ContractError, NormalizedAd, RecordRejection, RecordRejectionCode, SafeProviderText,
    ValidatedPage, WebSearchPageRequest, validate_web_search_page,
};
pub use policy::{
    ADAPTER_VERSION, AGENT_QUOTE_ENDPOINT, AGENT_TRADE_METHODS_ENDPOINT, DISCLOSURE_NO_FALLBACK,
    DISCLOSURE_SUMMARY, DISCLOSURE_VERSION, PROVIDER_POLICY, ProviderPolicy, SOURCE_LABEL,
    SOURCE_ROLE, WEB_SEARCH_ENDPOINT,
};
pub use runtime::{LiveProviderRuntime, PairCheckError, PairCheckResult, RuntimeBuildError};
pub use scheduler::{
    Acquisition, AcquisitionEligibility, AcquisitionProgress, AcquisitionRequest,
    PaginationFailure, ProgressStage, ProviderError, ProviderService, SideAcquisition,
    SideProgress,
};
pub use transport::{
    GlobalRequestGate, PageTransport, ReqwestPageTransport, TransportError, TransportFuture,
    TransportRequest, TransportResponse,
};
