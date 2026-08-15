use p2p_domain::{MarketPair, StableId};
use serde::Serialize;

use crate::policy::ADAPTER_VERSION;
use crate::scheduler::Acquisition;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifiedPair {
    pub pair: MarketPair,
    pub verified_at_ms: i64,
    pub adapter_version: &'static str,
    pub observed_payment_methods: Vec<StableId>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DisabledPair {
    pub pair: MarketPair,
    pub disabled_at_ms: i64,
    pub reason_code: StableId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum PairCatalogEntry {
    Verified(VerifiedPair),
    Disabled(DisabledPair),
}

impl VerifiedPair {
    pub fn from_acquisition(
        acquisition: &Acquisition,
        verified_at_ms: i64,
    ) -> Result<Self, p2p_domain::DomainValidationError> {
        let methods = acquisition
            .buy
            .ads
            .iter()
            .chain(&acquisition.sell.ads)
            .flat_map(|normalized| normalized.ad.payments())
            .map(|method| StableId::new(method.as_str()))
            .collect::<Result<Vec<_>, _>>()?;
        let mut observed_payment_methods = methods;
        observed_payment_methods.sort();
        observed_payment_methods.dedup();
        Ok(Self {
            pair: acquisition.pair.clone(),
            verified_at_ms,
            adapter_version: ADAPTER_VERSION,
            observed_payment_methods,
        })
    }
}

#[cfg(test)]
mod tests {
    use p2p_domain::Symbol;

    use super::*;

    #[test]
    fn disabled_pair_requires_an_explicit_reason_and_time() {
        let pair = MarketPair::new(
            Symbol::new("USDT").expect("asset"),
            Symbol::new("EGP").expect("fiat"),
        )
        .expect("pair");
        let entry = PairCatalogEntry::Disabled(DisabledPair {
            pair,
            disabled_at_ms: 123,
            reason_code: StableId::new("provider-unavailable").expect("reason"),
        });
        let value = serde_json::to_value(entry).expect("serialize");
        assert_eq!(value["state"], "disabled");
        assert_eq!(value["disabledAtMs"], 123);
    }
}
