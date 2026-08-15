use std::collections::BTreeSet;

use serde::Serialize;

use crate::{
    AmountMode, CalculationError, EligibilityFilters, ExactDecimal, RequestedAmount, SingleAdQuote,
    UserIntent, ValidatedAd, rank_offers,
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SensitivityPoint {
    pub amount: ExactDecimal,
    pub amount_mode: AmountMode,
    pub eligible_ad_count: usize,
    pub eligible_merchant_count: usize,
    pub best_single_ad_quote: Option<SingleAdQuote>,
}

pub fn amount_sensitivity(
    intent: UserIntent,
    amounts: &[RequestedAmount],
    filters: &EligibilityFilters,
    ads: &[ValidatedAd],
) -> Result<Vec<SensitivityPoint>, CalculationError> {
    if amounts.is_empty()
        || amounts
            .windows(2)
            .any(|pair| pair[0].mode() != pair[1].mode() || pair[0].value() >= pair[1].value())
    {
        return Err(CalculationError::InvalidSensitivityAmounts);
    }

    amounts
        .iter()
        .copied()
        .map(|amount| {
            let ranked = rank_offers(intent, amount, filters, ads)?;
            let eligible = ranked
                .iter()
                .filter(|offer| offer.evaluation().eligible())
                .collect::<Vec<_>>();
            let eligible_merchants = eligible
                .iter()
                .map(|offer| offer.ad().merchant().stable_id())
                .collect::<BTreeSet<_>>()
                .len();
            let best_single_ad_quote = eligible.first().and_then(|offer| offer.quote()).cloned();
            Ok(SensitivityPoint {
                amount: amount.value(),
                amount_mode: amount.mode(),
                eligible_ad_count: eligible.len(),
                eligible_merchant_count: eligible_merchants,
                best_single_ad_quote,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::str::FromStr;

    use crate::{AdvertiserSide, MerchantFacts, PaymentMethod, StableId, ValidatedAdInput};

    use super::*;

    fn decimal(value: &str) -> ExactDecimal {
        ExactDecimal::from_str(value).expect("fixture decimal")
    }

    fn ad(id: &str, maximum: &str, merchant: &str) -> ValidatedAd {
        ValidatedAd::new(ValidatedAdInput {
            stable_id: StableId::new(id).expect("id"),
            advertiser_side: AdvertiserSide::Sell,
            price: decimal("50"),
            min_fiat: decimal("1"),
            max_fiat: decimal(maximum),
            available_asset: decimal("1000"),
            payments: BTreeSet::from([PaymentMethod::new("BANK").expect("payment")]),
            merchant: MerchantFacts::new(
                StableId::new(merchant).expect("merchant"),
                100,
                decimal("99"),
                decimal("99"),
                false,
            )
            .expect("merchant facts"),
            observed_at_ms: 100,
        })
        .expect("ad")
    }

    #[test]
    fn sensitivity_revalidates_limits_and_deduplicates_merchants_at_each_amount() {
        let amounts = [
            RequestedAmount::new(decimal("100"), AmountMode::Fiat).expect("amount"),
            RequestedAmount::new(decimal("1000"), AmountMode::Fiat).expect("amount"),
        ];
        let points = amount_sensitivity(
            UserIntent::BuyAsset,
            &amounts,
            &EligibilityFilters::neutral(),
            &[
                ad("a", "500", "merchant-one"),
                ad("b", "2000", "merchant-one"),
                ad("c", "2000", "merchant-two"),
            ],
        )
        .expect("sensitivity");
        assert_eq!(points[0].eligible_ad_count, 3);
        assert_eq!(points[0].eligible_merchant_count, 2);
        assert_eq!(points[1].eligible_ad_count, 2);
        assert_eq!(points[1].eligible_merchant_count, 2);
    }

    #[test]
    fn sensitivity_rejects_unsorted_or_mixed_mode_amounts() {
        let invalid = [
            RequestedAmount::new(decimal("100"), AmountMode::Fiat).expect("amount"),
            RequestedAmount::new(decimal("10"), AmountMode::Asset).expect("amount"),
        ];
        assert_eq!(
            amount_sensitivity(
                UserIntent::BuyAsset,
                &invalid,
                &EligibilityFilters::neutral(),
                &[]
            ),
            Err(CalculationError::InvalidSensitivityAmounts)
        );
    }
}
