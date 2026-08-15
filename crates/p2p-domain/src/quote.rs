use std::cmp::Ordering;

use serde::Serialize;

use crate::{
    AmountMode, CalculationError, EligibilityEvaluation, EligibilityFilters, ExactDecimal,
    RequestedAmount, StableId, UserIntent, ValidatedAd, evaluate_eligibility,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "intent")]
pub enum QuoteFlow {
    BuyAsset {
        fiat_cost: ExactDecimal,
        asset_received: ExactDecimal,
    },
    SellAsset {
        asset_required: ExactDecimal,
        fiat_proceeds: ExactDecimal,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SingleAdQuote {
    ad_id: StableId,
    price: ExactDecimal,
    flow: QuoteFlow,
}

impl SingleAdQuote {
    pub fn ad_id(&self) -> &StableId {
        &self.ad_id
    }

    pub fn price(&self) -> ExactDecimal {
        self.price
    }

    pub fn flow(&self) -> QuoteFlow {
        self.flow
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RankedOffer {
    ad: ValidatedAd,
    evaluation: EligibilityEvaluation,
    quote: Option<SingleAdQuote>,
    eligible_rank: Option<usize>,
}

impl RankedOffer {
    pub fn ad(&self) -> &ValidatedAd {
        &self.ad
    }

    pub fn evaluation(&self) -> &EligibilityEvaluation {
        &self.evaluation
    }

    pub fn quote(&self) -> Option<&SingleAdQuote> {
        self.quote.as_ref()
    }

    pub fn eligible_rank(&self) -> Option<usize> {
        self.eligible_rank
    }
}

pub fn single_ad_quote(
    intent: UserIntent,
    ad: &ValidatedAd,
    evaluation: &EligibilityEvaluation,
) -> Option<SingleAdQuote> {
    if !evaluation.eligible() {
        return None;
    }
    let flow = match intent {
        UserIntent::BuyAsset => QuoteFlow::BuyAsset {
            fiat_cost: evaluation.fiat_amount(),
            asset_received: evaluation.asset_amount(),
        },
        UserIntent::SellAsset => QuoteFlow::SellAsset {
            asset_required: evaluation.asset_amount(),
            fiat_proceeds: evaluation.fiat_amount(),
        },
    };
    Some(SingleAdQuote {
        ad_id: ad.stable_id().clone(),
        price: ad.price(),
        flow,
    })
}

pub fn rank_offers(
    intent: UserIntent,
    amount: RequestedAmount,
    filters: &EligibilityFilters,
    ads: &[ValidatedAd],
) -> Result<Vec<RankedOffer>, CalculationError> {
    let mut offers = ads
        .iter()
        .map(|ad| {
            let evaluation = evaluate_eligibility(intent, amount, filters, ad)?;
            let quote = single_ad_quote(intent, ad, &evaluation);
            Ok(RankedOffer {
                ad: ad.clone(),
                evaluation,
                quote,
                eligible_rank: None,
            })
        })
        .collect::<Result<Vec<_>, CalculationError>>()?;

    offers.sort_by(|left, right| compare_offers(intent, amount.mode(), left, right));
    let mut rank = 0_usize;
    for offer in &mut offers {
        if offer.evaluation.eligible() {
            rank += 1;
            offer.eligible_rank = Some(rank);
        }
    }
    Ok(offers)
}

fn compare_offers(
    intent: UserIntent,
    amount_mode: AmountMode,
    left: &RankedOffer,
    right: &RankedOffer,
) -> Ordering {
    right
        .evaluation
        .eligible()
        .cmp(&left.evaluation.eligible())
        .then_with(|| compare_economic_result(intent, amount_mode, left, right))
        .then_with(|| {
            right
                .ad
                .merchant()
                .completion_percent()
                .cmp(&left.ad.merchant().completion_percent())
        })
        .then_with(|| {
            right
                .ad
                .merchant()
                .monthly_orders()
                .cmp(&left.ad.merchant().monthly_orders())
        })
        .then_with(|| right.ad.observed_at_ms().cmp(&left.ad.observed_at_ms()))
        .then_with(|| left.ad.stable_id().cmp(right.ad.stable_id()))
}

fn compare_economic_result(
    intent: UserIntent,
    amount_mode: AmountMode,
    left: &RankedOffer,
    right: &RankedOffer,
) -> Ordering {
    if !left.evaluation.eligible() || !right.evaluation.eligible() {
        return Ordering::Equal;
    }
    match (intent, amount_mode) {
        (UserIntent::BuyAsset, AmountMode::Fiat) => right
            .evaluation
            .asset_amount()
            .cmp(&left.evaluation.asset_amount()),
        (UserIntent::BuyAsset, AmountMode::Asset) => left
            .evaluation
            .fiat_amount()
            .cmp(&right.evaluation.fiat_amount()),
        (UserIntent::SellAsset, AmountMode::Fiat) => left
            .evaluation
            .asset_amount()
            .cmp(&right.evaluation.asset_amount()),
        (UserIntent::SellAsset, AmountMode::Asset) => right
            .evaluation
            .fiat_amount()
            .cmp(&left.evaluation.fiat_amount()),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::str::FromStr;

    use crate::{AdvertiserSide, MerchantFacts, PaymentMethod, ValidatedAdInput};

    use super::*;

    fn decimal(value: &str) -> ExactDecimal {
        ExactDecimal::from_str(value).expect("fixture decimal")
    }

    fn ad(
        id: &str,
        side: AdvertiserSide,
        price: &str,
        completion: &str,
        orders: u64,
    ) -> ValidatedAd {
        ValidatedAd::new(ValidatedAdInput {
            stable_id: StableId::new(id).expect("ad id"),
            advertiser_side: side,
            price: decimal(price),
            min_fiat: decimal("1"),
            max_fiat: decimal("1000000"),
            available_asset: decimal("1000000"),
            payments: BTreeSet::from([PaymentMethod::new("BANK").expect("payment")]),
            merchant: MerchantFacts::new(
                StableId::new(format!("merchant-{id}")).expect("merchant id"),
                orders,
                decimal(completion),
                decimal("99"),
                false,
            )
            .expect("merchant"),
            observed_at_ms: 100,
        })
        .expect("ad")
    }

    #[test]
    fn buy_ranks_lower_price_while_sell_ranks_higher_price() {
        let amount = RequestedAmount::new(decimal("1000"), AmountMode::Fiat).expect("amount");
        let buy_ads = vec![
            ad("high", AdvertiserSide::Sell, "51", "99", 100),
            ad("low", AdvertiserSide::Sell, "50", "90", 10),
        ];
        let buy = rank_offers(
            UserIntent::BuyAsset,
            amount,
            &EligibilityFilters::neutral(),
            &buy_ads,
        )
        .expect("rank");
        assert_eq!(buy[0].ad().stable_id().as_str(), "low");

        let sell_ads = vec![
            ad("high", AdvertiserSide::Buy, "51", "90", 10),
            ad("low", AdvertiserSide::Buy, "50", "99", 100),
        ];
        let sell = rank_offers(
            UserIntent::SellAsset,
            amount,
            &EligibilityFilters::neutral(),
            &sell_ads,
        )
        .expect("rank");
        assert_eq!(sell[0].ad().stable_id().as_str(), "high");
    }

    #[test]
    fn transparent_ties_use_completion_orders_freshness_then_stable_id() {
        let amount = RequestedAmount::new(decimal("10"), AmountMode::Asset).expect("amount");
        let ads = vec![
            ad("b", AdvertiserSide::Sell, "50", "98", 100),
            ad("a", AdvertiserSide::Sell, "50", "98", 100),
            ad("better", AdvertiserSide::Sell, "50", "99", 1),
        ];
        let ranked = rank_offers(
            UserIntent::BuyAsset,
            amount,
            &EligibilityFilters::neutral(),
            &ads,
        )
        .expect("rank");
        let ids = ranked
            .iter()
            .map(|offer| offer.ad().stable_id().as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, ["better", "a", "b"]);
    }

    #[test]
    fn fiat_and_asset_modes_keep_symmetric_buy_sell_amount_semantics() {
        let buy_ad = ad("buy", AdvertiserSide::Sell, "50", "99", 100);
        let sell_ad = ad("sell", AdvertiserSide::Buy, "50", "99", 100);
        let fiat = RequestedAmount::new(decimal("100"), AmountMode::Fiat).expect("amount");
        let buy = rank_offers(
            UserIntent::BuyAsset,
            fiat,
            &EligibilityFilters::neutral(),
            std::slice::from_ref(&buy_ad),
        )
        .expect("buy");
        assert_eq!(
            buy[0].quote().expect("quote").flow(),
            QuoteFlow::BuyAsset {
                fiat_cost: decimal("100"),
                asset_received: decimal("2"),
            }
        );
        let sell = rank_offers(
            UserIntent::SellAsset,
            fiat,
            &EligibilityFilters::neutral(),
            &[sell_ad],
        )
        .expect("sell");
        assert_eq!(
            sell[0].quote().expect("quote").flow(),
            QuoteFlow::SellAsset {
                asset_required: decimal("2"),
                fiat_proceeds: decimal("100"),
            }
        );

        let asset = RequestedAmount::new(decimal("2"), AmountMode::Asset).expect("amount");
        let buy = rank_offers(
            UserIntent::BuyAsset,
            asset,
            &EligibilityFilters::neutral(),
            &[buy_ad],
        )
        .expect("buy");
        assert_eq!(
            buy[0].quote().expect("quote").flow(),
            QuoteFlow::BuyAsset {
                fiat_cost: decimal("100"),
                asset_received: decimal("2"),
            }
        );
    }

    #[test]
    fn ineligible_ads_never_outrank_full_amount_eligible_ads() {
        let amount = RequestedAmount::new(decimal("1000"), AmountMode::Fiat).expect("amount");
        let mut unavailable = ad("cheap", AdvertiserSide::Sell, "1", "100", 1000);
        unavailable = ValidatedAd::new(ValidatedAdInput {
            stable_id: unavailable.stable_id().clone(),
            advertiser_side: unavailable.advertiser_side(),
            price: unavailable.price(),
            min_fiat: unavailable.min_fiat(),
            max_fiat: unavailable.max_fiat(),
            available_asset: ExactDecimal::ZERO,
            payments: unavailable.payments().clone(),
            merchant: unavailable.merchant().clone(),
            observed_at_ms: unavailable.observed_at_ms(),
        })
        .expect("valid but unavailable ad");
        let ranked = rank_offers(
            UserIntent::BuyAsset,
            amount,
            &EligibilityFilters::neutral(),
            &[
                unavailable,
                ad("eligible", AdvertiserSide::Sell, "50", "90", 10),
            ],
        )
        .expect("rank");
        assert_eq!(ranked[0].ad().stable_id().as_str(), "eligible");
        assert_eq!(ranked[0].eligible_rank(), Some(1));
        assert_eq!(ranked[1].eligible_rank(), None);
    }
}
