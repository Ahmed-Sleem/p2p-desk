use std::time::Duration;

use serde::Serialize;

pub const SOURCE_LABEL: &str = "Experimental Binance P2P Web";
pub const SOURCE_ROLE: &str = "Unsupported website search contract; sole advertisement dataset";
pub const ADAPTER_VERSION: &str = "p2p-desk-binance-web-1.0.0";
pub const DISCLOSURE_VERSION: u32 = 1;

pub const WEB_SEARCH_ENDPOINT: &str = "https://p2p.binance.com/bapi/c2c/v2/friendly/c2c/adv/search";
pub const AGENT_TRADE_METHODS_ENDPOINT: &str =
    "https://www.binance.com/bapi/c2c/v1/public/c2c/agent/trade-methods";
pub const AGENT_QUOTE_ENDPOINT: &str =
    "https://www.binance.com/bapi/c2c/v1/public/c2c/agent/quote-price";

pub const PAGE_SIZE: u8 = 20;
pub const MAX_PAGES_PER_SIDE: u8 = 50;
pub const MINIMUM_START_GAP: Duration = Duration::from_millis(500);
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
pub const MAX_ATTEMPTS: u8 = 3;
pub const FIRST_RETRY_DELAY: Duration = Duration::from_secs(1);
pub const SECOND_RETRY_DELAY: Duration = Duration::from_secs(2);
pub const MINIMUM_RATE_LIMIT_CIRCUIT: Duration = Duration::from_secs(60);
pub const MINIMUM_WAF_CIRCUIT: Duration = Duration::from_secs(15 * 60);
pub const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_PROVIDER_TOTAL: u32 = 1_000_000;

pub const DISCLOSURE_SUMMARY: &str = "P2P Desk reads a Binance website search used without a published stable developer contract. It can change, rate-limit, block, or become unavailable without notice.";
pub const DISCLOSURE_NO_FALLBACK: &str = "Source failures fail closed. Cached, historical, Agent, secondary, or fabricated values are never shown as live advertisements.";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPolicy {
    pub source_label: &'static str,
    pub source_role: &'static str,
    pub adapter_version: &'static str,
    pub disclosure_version: u32,
    pub disclosure_summary: &'static str,
    pub no_fallback_statement: &'static str,
    pub acknowledgement_required: bool,
    pub page_size: u8,
    pub max_pages_per_side: u8,
    pub minimum_start_gap_ms: u64,
    pub connect_timeout_ms: u64,
    pub request_timeout_ms: u64,
    pub max_attempts: u8,
}

pub const PROVIDER_POLICY: ProviderPolicy = ProviderPolicy {
    source_label: SOURCE_LABEL,
    source_role: SOURCE_ROLE,
    adapter_version: ADAPTER_VERSION,
    disclosure_version: DISCLOSURE_VERSION,
    disclosure_summary: DISCLOSURE_SUMMARY,
    no_fallback_statement: DISCLOSURE_NO_FALLBACK,
    acknowledgement_required: true,
    page_size: PAGE_SIZE,
    max_pages_per_side: MAX_PAGES_PER_SIDE,
    minimum_start_gap_ms: MINIMUM_START_GAP.as_millis() as u64,
    connect_timeout_ms: CONNECT_TIMEOUT.as_millis() as u64,
    request_timeout_ms: REQUEST_TIMEOUT.as_millis() as u64,
    max_attempts: MAX_ATTEMPTS,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_matches_the_approved_non_fallback_bounds() {
        assert_eq!(PAGE_SIZE, 20);
        assert_eq!(MAX_PAGES_PER_SIDE, 50);
        assert_eq!(MINIMUM_START_GAP, Duration::from_millis(500));
        assert_eq!(CONNECT_TIMEOUT, Duration::from_secs(10));
        assert_eq!(REQUEST_TIMEOUT, Duration::from_secs(30));
        assert_eq!(MAX_ATTEMPTS, 3);
        let serialized = serde_json::to_value(PROVIDER_POLICY).expect("serialize policy");
        assert_eq!(serialized["acknowledgementRequired"], true);
        assert!(PROVIDER_POLICY.no_fallback_statement.contains("never"));
    }

    #[test]
    fn every_network_destination_is_an_exact_https_allowlist_constant() {
        for endpoint in [
            WEB_SEARCH_ENDPOINT,
            AGENT_TRADE_METHODS_ENDPOINT,
            AGENT_QUOTE_ENDPOINT,
        ] {
            assert!(endpoint.starts_with("https://"));
            assert!(!endpoint.contains(['?', '#']));
        }
    }
}
