use std::collections::BTreeSet;

use serde::Serialize;

use crate::{
    AmountMode, CalculationError, EligibilityFilters, ExactDecimal, PaymentLogic, PaymentMethod,
    RequestedAmount, UserIntent, ValidatedAd,
};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum EligibilityReason {
    WrongAdvertiserSide,
    BelowMinimum,
    AboveMaximum,
    InsufficientAvailability,
    PaymentMismatch,
    BelowMinimumOrders,
    BelowMinimumCompletion,
    BelowMinimumPositive,
    NotPro,
    AboveMaximumBuyPrice,
    BelowMinimumSellPrice,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EligibilityEvaluation {
    eligible: bool,
    reasons: BTreeSet<EligibilityReason>,
    fiat_amount: ExactDecimal,
    asset_amount: ExactDecimal,
    compatible_payments: BTreeSet<PaymentMethod>,
}

impl EligibilityEvaluation {
    pub fn eligible(&self) -> bool {
        self.eligible
    }

    pub fn reasons(&self) -> &BTreeSet<EligibilityReason> {
        &self.reasons
    }

    pub fn fiat_amount(&self) -> ExactDecimal {
        self.fiat_amount
    }

    pub fn asset_amount(&self) -> ExactDecimal {
        self.asset_amount
    }

    pub fn compatible_payments(&self) -> &BTreeSet<PaymentMethod> {
        &self.compatible_payments
    }
}

pub fn evaluate_eligibility(
    intent: UserIntent,
    amount: RequestedAmount,
    filters: &EligibilityFilters,
    ad: &ValidatedAd,
) -> Result<EligibilityEvaluation, CalculationError> {
    let mut reasons = BTreeSet::new();
    if ad.advertiser_side() != intent.expected_advertiser_side() {
        reasons.insert(EligibilityReason::WrongAdvertiserSide);
    }

    let (fiat_amount, asset_amount) = match amount.mode() {
        AmountMode::Fiat => (amount.value(), amount.value().checked_div(ad.price())?),
        AmountMode::Asset => (amount.value().checked_mul(ad.price())?, amount.value()),
    };

    if fiat_amount < ad.min_fiat() {
        reasons.insert(EligibilityReason::BelowMinimum);
    }
    if fiat_amount > ad.max_fiat() {
        reasons.insert(EligibilityReason::AboveMaximum);
    }
    if asset_amount > ad.available_asset() {
        reasons.insert(EligibilityReason::InsufficientAvailability);
    }

    let compatible_payments = compatible_payments(filters, ad);
    let payment_matches = if filters.selected_payments().is_empty() {
        true
    } else {
        match filters.payment_logic() {
            PaymentLogic::Any => !compatible_payments.is_empty(),
            PaymentLogic::All => filters.selected_payments().is_subset(ad.payments()),
        }
    };
    if !payment_matches {
        reasons.insert(EligibilityReason::PaymentMismatch);
    }

    if ad.merchant().monthly_orders() < filters.minimum_orders() {
        reasons.insert(EligibilityReason::BelowMinimumOrders);
    }
    if ad.merchant().completion_percent() < filters.minimum_completion_percent() {
        reasons.insert(EligibilityReason::BelowMinimumCompletion);
    }
    if ad.merchant().positive_percent() < filters.minimum_positive_percent() {
        reasons.insert(EligibilityReason::BelowMinimumPositive);
    }
    if filters.pro_only() && !ad.merchant().is_pro() {
        reasons.insert(EligibilityReason::NotPro);
    }

    match intent {
        UserIntent::BuyAsset => {
            if filters
                .maximum_buy_price()
                .is_some_and(|maximum| ad.price() > maximum)
            {
                reasons.insert(EligibilityReason::AboveMaximumBuyPrice);
            }
        }
        UserIntent::SellAsset => {
            if filters
                .minimum_sell_price()
                .is_some_and(|minimum| ad.price() < minimum)
            {
                reasons.insert(EligibilityReason::BelowMinimumSellPrice);
            }
        }
    }

    Ok(EligibilityEvaluation {
        eligible: reasons.is_empty(),
        reasons,
        fiat_amount,
        asset_amount,
        compatible_payments,
    })
}

fn compatible_payments(filters: &EligibilityFilters, ad: &ValidatedAd) -> BTreeSet<PaymentMethod> {
    if filters.selected_payments().is_empty() {
        return ad.payments().clone();
    }
    filters
        .selected_payments()
        .intersection(ad.payments())
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use crate::{
        AdvertiserSide, EligibilityFiltersInput, MerchantFacts, StableId, ValidatedAdInput,
    };

    use super::*;

    fn decimal(value: &str) -> ExactDecimal {
        ExactDecimal::from_str(value).expect("fixture decimal")
    }

    fn ad(side: AdvertiserSide) -> ValidatedAd {
        ValidatedAd::new(ValidatedAdInput {
            stable_id: StableId::new("ad-1").expect("id"),
            advertiser_side: side,
            price: decimal("50"),
            min_fiat: decimal("100"),
            max_fiat: decimal("1000"),
            available_asset: decimal("15"),
            payments: [PaymentMethod::new("BANK").expect("payment")]
                .into_iter()
                .collect(),
            merchant: MerchantFacts::new(
                StableId::new("merchant-1").expect("id"),
                100,
                decimal("98"),
                decimal("99"),
                true,
            )
            .expect("merchant"),
            observed_at_ms: 100,
        })
        .expect("ad")
    }

    #[test]
    fn eligibility_uses_unrounded_amount_limits_and_availability() {
        let amount = RequestedAmount::new(
            decimal("15.0000000000000000000000000001"),
            AmountMode::Asset,
        )
        .expect("amount");
        let result = evaluate_eligibility(
            UserIntent::BuyAsset,
            amount,
            &EligibilityFilters::neutral(),
            &ad(AdvertiserSide::Sell),
        )
        .expect("evaluate");
        assert!(!result.eligible());
        assert!(
            result
                .reasons()
                .contains(&EligibilityReason::InsufficientAvailability)
        );
    }

    #[test]
    fn any_and_all_payment_logic_are_not_conflated() {
        let bank = PaymentMethod::new("BANK").expect("payment");
        let cash = PaymentMethod::new("CASH").expect("payment");
        let selected = [bank, cash].into_iter().collect();
        let mut input = EligibilityFiltersInput {
            selected_payments: selected,
            payment_logic: PaymentLogic::Any,
            minimum_orders: 0,
            minimum_completion_percent: ExactDecimal::ZERO,
            minimum_positive_percent: ExactDecimal::ZERO,
            pro_only: false,
            maximum_buy_price: None,
            minimum_sell_price: None,
        };
        let amount = RequestedAmount::new(decimal("500"), AmountMode::Fiat).expect("amount");
        let any = evaluate_eligibility(
            UserIntent::BuyAsset,
            amount,
            &EligibilityFilters::new(input.clone()).expect("filters"),
            &ad(AdvertiserSide::Sell),
        )
        .expect("evaluate");
        assert!(any.eligible());

        input.payment_logic = PaymentLogic::All;
        let all = evaluate_eligibility(
            UserIntent::BuyAsset,
            amount,
            &EligibilityFilters::new(input).expect("filters"),
            &ad(AdvertiserSide::Sell),
        )
        .expect("evaluate");
        assert!(!all.eligible());
        assert!(all.reasons().contains(&EligibilityReason::PaymentMismatch));
    }

    #[test]
    fn side_inversion_is_a_hard_eligibility_failure() {
        let amount = RequestedAmount::new(decimal("500"), AmountMode::Fiat).expect("amount");
        let result = evaluate_eligibility(
            UserIntent::BuyAsset,
            amount,
            &EligibilityFilters::neutral(),
            &ad(AdvertiserSide::Buy),
        )
        .expect("evaluate");
        assert!(
            result
                .reasons()
                .contains(&EligibilityReason::WrongAdvertiserSide)
        );
    }

    #[test]
    fn merchant_and_price_filters_report_each_transparent_reason() {
        let filters = EligibilityFilters::new(EligibilityFiltersInput {
            selected_payments: BTreeSet::new(),
            payment_logic: PaymentLogic::Any,
            minimum_orders: 101,
            minimum_completion_percent: decimal("99"),
            minimum_positive_percent: decimal("99.5"),
            pro_only: false,
            maximum_buy_price: Some(decimal("49.99")),
            minimum_sell_price: None,
        })
        .expect("filters");
        let result = evaluate_eligibility(
            UserIntent::BuyAsset,
            RequestedAmount::new(decimal("500"), AmountMode::Fiat).expect("amount"),
            &filters,
            &ad(AdvertiserSide::Sell),
        )
        .expect("evaluate");
        assert_eq!(
            result.reasons(),
            &BTreeSet::from([
                EligibilityReason::BelowMinimumOrders,
                EligibilityReason::BelowMinimumCompletion,
                EligibilityReason::BelowMinimumPositive,
                EligibilityReason::AboveMaximumBuyPrice,
            ])
        );
    }
}
