use serde::{Deserialize, Serialize};

/// User-facing action. This must never be inferred from provider bucket names.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UserIntent {
    BuyAsset,
    SellAsset,
}

/// Request value required by the Experimental Binance P2P Web adapter.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum RequestSide {
    Buy,
    Sell,
}

/// Advertiser action represented by a returned advertisement.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum AdvertiserSide {
    Buy,
    Sell,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AmountMode {
    Fiat,
    Asset,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum PaymentLogic {
    Any,
    All,
}

impl UserIntent {
    pub const fn request_side(self) -> RequestSide {
        match self {
            Self::BuyAsset => RequestSide::Buy,
            Self::SellAsset => RequestSide::Sell,
        }
    }

    pub const fn expected_advertiser_side(self) -> AdvertiserSide {
        match self {
            Self::BuyAsset => AdvertiserSide::Sell,
            Self::SellAsset => AdvertiserSide::Buy,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_intent_mapping_corrects_the_old_side_inversion() {
        assert_eq!(UserIntent::BuyAsset.request_side(), RequestSide::Buy);
        assert_eq!(
            UserIntent::BuyAsset.expected_advertiser_side(),
            AdvertiserSide::Sell
        );
        assert_eq!(UserIntent::SellAsset.request_side(), RequestSide::Sell);
        assert_eq!(
            UserIntent::SellAsset.expected_advertiser_side(),
            AdvertiserSide::Buy
        );
    }

    #[test]
    fn sides_serialize_to_provider_facing_uppercase_values() {
        assert_eq!(
            serde_json::to_string(&RequestSide::Buy).expect("serialize"),
            "\"BUY\""
        );
        assert_eq!(
            serde_json::to_string(&AdvertiserSide::Sell).expect("serialize"),
            "\"SELL\""
        );
    }
}
