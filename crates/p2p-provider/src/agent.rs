use std::collections::BTreeSet;

use p2p_domain::{ExactDecimal, MarketPair, PaymentMethod, RequestSide};
use reqwest::header::{ACCEPT, HeaderValue};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio_util::sync::CancellationToken;

use crate::circuit::{CircuitBreaker, CircuitState};
use crate::contract::{DecimalToken, SafeProviderText};
use crate::policy::{AGENT_QUOTE_ENDPOINT, AGENT_TRADE_METHODS_ENDPOINT};
use crate::transport::{
    GlobalRequestGate, TransportError, build_http_client, collect_bounded_response,
};

const SUCCESS_CODE: &str = "000000";

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTradeMethod {
    pub identifier: PaymentMethod,
    pub name: SafeProviderText,
    pub short_name: Option<SafeProviderText>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTradeMethods {
    pub fiat: String,
    pub observed_at_ms: i64,
    pub methods: Vec<AgentTradeMethod>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentQuote {
    pub pair: MarketPair,
    pub request_side: RequestSide,
    pub observed_at_ms: i64,
    pub price: ExactDecimal,
    pub asset_scale: u8,
    pub fiat_scale: u8,
    pub price_scale: u8,
}

/// Agent results are separate health/metadata types. They cannot be converted
/// into `NormalizedAd` or `Acquisition` and are never a primary-data fallback.
#[derive(Clone)]
pub struct AgentMetadataClient {
    client: reqwest::Client,
    request_gate: GlobalRequestGate,
    circuit: CircuitBreaker,
}

#[derive(Clone, Debug, Error, PartialEq)]
pub enum AgentError {
    #[error("Agent metadata request was cancelled")]
    Cancelled,
    #[error("global provider circuit is open")]
    CircuitOpen(CircuitState),
    #[error("Agent metadata HTTP request failed with status {0}")]
    Http(u16),
    #[error("Agent metadata transport failed")]
    Transport(TransportError),
    #[error("Agent metadata contract validation failed")]
    Contract,
}

impl AgentError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Cancelled => "agent-metadata-cancelled",
            Self::CircuitOpen(_) => "agent-circuit-open",
            Self::Http(_) => "agent-http-error",
            Self::Transport(_) => "agent-transport-error",
            Self::Contract => "agent-contract-error",
        }
    }
}

impl AgentMetadataClient {
    pub fn new(
        request_gate: GlobalRequestGate,
        circuit: CircuitBreaker,
    ) -> Result<Self, AgentError> {
        Ok(Self {
            client: build_http_client().map_err(AgentError::Transport)?,
            request_gate,
            circuit,
        })
    }

    pub async fn trade_methods(
        &self,
        fiat: &str,
        cancellation: CancellationToken,
    ) -> Result<AgentTradeMethods, AgentError> {
        validate_symbol(fiat)?;
        let response = self
            .get(
                AGENT_TRADE_METHODS_ENDPOINT,
                &[("fiat", fiat)],
                &cancellation,
            )
            .await?;
        let envelope: RawEnvelope<Vec<RawTradeMethod>> =
            serde_json::from_slice(&response.body).map_err(|_| AgentError::Contract)?;
        validate_envelope(&envelope)?;
        let mut identifiers = BTreeSet::new();
        let mut methods = Vec::with_capacity(envelope.data.len());
        for raw in envelope.data {
            let identifier =
                PaymentMethod::new(raw.identifier).map_err(|_| AgentError::Contract)?;
            let name = SafeProviderText::new(&raw.trade_method_name).ok_or(AgentError::Contract)?;
            let short_name = raw
                .trade_method_short_name
                .as_deref()
                .and_then(SafeProviderText::new);
            if !identifiers.insert(identifier.clone()) {
                return Err(AgentError::Contract);
            }
            methods.push(AgentTradeMethod {
                identifier,
                name,
                short_name,
            });
        }
        Ok(AgentTradeMethods {
            fiat: fiat.to_owned(),
            observed_at_ms: response.received_at_ms,
            methods,
        })
    }

    pub async fn quote(
        &self,
        pair: &MarketPair,
        request_side: RequestSide,
        cancellation: CancellationToken,
    ) -> Result<AgentQuote, AgentError> {
        let side = match request_side {
            RequestSide::Buy => "BUY",
            RequestSide::Sell => "SELL",
        };
        let response = self
            .get(
                AGENT_QUOTE_ENDPOINT,
                &[
                    ("fiat", pair.fiat().as_str()),
                    ("asset", pair.asset().as_str()),
                    ("tradeType", side),
                ],
                &cancellation,
            )
            .await?;
        let envelope: RawEnvelope<RawQuote> =
            serde_json::from_slice(&response.body).map_err(|_| AgentError::Contract)?;
        validate_envelope(&envelope)?;
        let raw = envelope.data;
        if raw.asset != pair.asset().as_str()
            || raw.fiat != pair.fiat().as_str()
            || raw.asset_scale > 28
            || raw.fiat_scale > 28
            || raw.price_scale > 28
        {
            return Err(AgentError::Contract);
        }
        let price = raw.price.parse().map_err(|_| AgentError::Contract)?;
        if !price.is_positive() {
            return Err(AgentError::Contract);
        }
        Ok(AgentQuote {
            pair: pair.clone(),
            request_side,
            observed_at_ms: response.received_at_ms,
            price,
            asset_scale: raw.asset_scale,
            fiat_scale: raw.fiat_scale,
            price_scale: raw.price_scale,
        })
    }

    async fn get(
        &self,
        endpoint: &'static str,
        query: &[(&str, &str)],
        cancellation: &CancellationToken,
    ) -> Result<crate::transport::TransportResponse, AgentError> {
        self.circuit
            .ensure_available()
            .await
            .map_err(AgentError::CircuitOpen)?;
        let request = self
            .client
            .get(endpoint)
            .header(ACCEPT, HeaderValue::from_static("application/json"))
            .query(query)
            .send();
        let response = self
            .request_gate
            .run(cancellation, async {
                let response = tokio::select! {
                    () = cancellation.cancelled() => return Err(TransportError::Cancelled),
                    result = request => result.map_err(|error| {
                        if error.is_connect() && error.is_timeout() {
                            TransportError::ConnectTimeout
                        } else if error.is_timeout() {
                            TransportError::RequestTimeout
                        } else {
                            TransportError::Network
                        }
                    })?,
                };
                collect_bounded_response(response, cancellation).await
            })
            .await
            .map_err(|error| match error {
                TransportError::Cancelled => AgentError::Cancelled,
                other => AgentError::Transport(other),
            })?;
        match response.status {
            200 => Ok(response),
            429 => {
                let state = self.circuit.open_rate_limit(response.retry_after).await;
                Err(AgentError::CircuitOpen(state))
            }
            403 | 418 => {
                let state = self.circuit.open_waf_or_ban(response.retry_after).await;
                Err(AgentError::CircuitOpen(state))
            }
            status => Err(AgentError::Http(status)),
        }
    }
}

#[derive(Debug, Deserialize)]
struct RawEnvelope<T> {
    code: String,
    success: bool,
    data: T,
}

fn validate_envelope<T>(envelope: &RawEnvelope<T>) -> Result<(), AgentError> {
    if envelope.success && envelope.code == SUCCESS_CODE {
        Ok(())
    } else {
        Err(AgentError::Contract)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawTradeMethod {
    identifier: String,
    trade_method_name: String,
    trade_method_short_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawQuote {
    asset: String,
    asset_scale: u8,
    fiat: String,
    fiat_scale: u8,
    price: DecimalToken,
    price_scale: u8,
}

fn validate_symbol(value: &str) -> Result<(), AgentError> {
    if (2..=20).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
    {
        Ok(())
    } else {
        Err(AgentError::Contract)
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use p2p_domain::Symbol;
    use serde_json::json;

    use super::*;

    #[test]
    fn agent_quote_contract_keeps_json_number_exact_and_pair_bound() {
        let pair = MarketPair::new(
            Symbol::new("USDT").expect("asset"),
            Symbol::new("EGP").expect("fiat"),
        )
        .expect("pair");
        let raw: RawEnvelope<RawQuote> = serde_json::from_value(json!({
            "code": "000000",
            "success": true,
            "data": {
                "asset": "USDT", "assetScale": 8,
                "fiat": "EGP", "fiatScale": 2,
                "price": 50.125, "priceScale": 3,
                "fiatSymbol": "E£"
            }
        }))
        .expect("contract");
        validate_envelope(&raw).expect("success");
        assert_eq!(raw.data.asset, pair.asset().as_str());
        assert_eq!(
            raw.data.price.parse().expect("exact").canonical(),
            ExactDecimal::from_str("50.125")
                .expect("expected")
                .canonical()
        );
    }

    #[test]
    fn agent_types_have_no_primary_ad_conversion_or_fields() {
        let method = AgentTradeMethod {
            identifier: PaymentMethod::new("SYNTHETIC_PAY").expect("id"),
            name: SafeProviderText::new("Synthetic payment").expect("name"),
            short_name: SafeProviderText::new("Synthetic"),
        };
        let value = serde_json::to_value(method).expect("serialize");
        assert!(value.get("ad").is_none());
        assert!(value.get("price").is_none());
        assert!(value.get("merchant").is_none());
    }
}
