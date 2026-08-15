use serde::Serialize;

use crate::{
    AmountMode, CalculationError, EligibilityFilters, ExactDecimal, MarketPair, PaymentMethod,
    RequestedAmount, StableId, UserIntent, ValidatedAd, evaluate_eligibility,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct CostInput(Option<ExactDecimal>);

impl CostInput {
    pub const UNKNOWN: Self = Self(None);

    pub fn known(value: ExactDecimal) -> Result<Self, CalculationError> {
        if value.is_negative() {
            return Err(CalculationError::InvalidCostProfile);
        }
        Ok(Self(Some(value)))
    }

    pub fn value(self) -> Option<ExactDecimal> {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LegCostTerms {
    fixed_fiat: CostInput,
    percent_fiat: CostInput,
    fixed_asset: CostInput,
    minimum_fiat: Option<ExactDecimal>,
    maximum_fiat: Option<ExactDecimal>,
    buffer_fixed_fiat: CostInput,
    buffer_percent_fiat: CostInput,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LegCostTermsInput {
    pub fixed_fiat: CostInput,
    pub percent_fiat: CostInput,
    pub fixed_asset: CostInput,
    pub minimum_fiat: Option<ExactDecimal>,
    pub maximum_fiat: Option<ExactDecimal>,
    pub buffer_fixed_fiat: CostInput,
    pub buffer_percent_fiat: CostInput,
}

impl LegCostTerms {
    pub fn new(input: LegCostTermsInput) -> Result<Self, CalculationError> {
        let percentages_valid = [input.percent_fiat, input.buffer_percent_fiat]
            .into_iter()
            .all(|value| {
                value
                    .value()
                    .is_none_or(|value| value <= ExactDecimal::HUNDRED)
            });
        let bounds_valid = input.minimum_fiat.is_none_or(|value| !value.is_negative())
            && input.maximum_fiat.is_none_or(|value| !value.is_negative())
            && match (input.minimum_fiat, input.maximum_fiat) {
                (Some(minimum), Some(maximum)) => minimum <= maximum,
                _ => true,
            };
        if !percentages_valid || !bounds_valid {
            return Err(CalculationError::InvalidCostProfile);
        }
        Ok(Self {
            fixed_fiat: input.fixed_fiat,
            percent_fiat: input.percent_fiat,
            fixed_asset: input.fixed_asset,
            minimum_fiat: input.minimum_fiat,
            maximum_fiat: input.maximum_fiat,
            buffer_fixed_fiat: input.buffer_fixed_fiat,
            buffer_percent_fiat: input.buffer_percent_fiat,
        })
    }

    pub fn explicit_zero() -> Self {
        let zero = CostInput(Some(ExactDecimal::ZERO));
        Self {
            fixed_fiat: zero,
            percent_fiat: zero,
            fixed_asset: zero,
            minimum_fiat: None,
            maximum_fiat: None,
            buffer_fixed_fiat: zero,
            buffer_percent_fiat: zero,
        }
    }

    pub fn unknown() -> Self {
        Self {
            fixed_fiat: CostInput::UNKNOWN,
            percent_fiat: CostInput::UNKNOWN,
            fixed_asset: CostInput::UNKNOWN,
            minimum_fiat: None,
            maximum_fiat: None,
            buffer_fixed_fiat: CostInput::UNKNOWN,
            buffer_percent_fiat: CostInput::UNKNOWN,
        }
    }

    fn known_values(&self) -> Option<KnownLegCostTerms> {
        Some(KnownLegCostTerms {
            fixed_fiat: self.fixed_fiat.value()?,
            percent_fiat: self.percent_fiat.value()?,
            fixed_asset: self.fixed_asset.value()?,
            minimum_fiat: self.minimum_fiat,
            maximum_fiat: self.maximum_fiat,
            buffer_fixed_fiat: self.buffer_fixed_fiat.value()?,
            buffer_percent_fiat: self.buffer_percent_fiat.value()?,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CostProfile {
    version_id: StableId,
    pair: MarketPair,
    route: PaymentMethod,
    effective_from_ms: i64,
    buy_leg: LegCostTerms,
    sell_leg: LegCostTerms,
}

impl CostProfile {
    pub fn new(
        version_id: StableId,
        pair: MarketPair,
        route: PaymentMethod,
        effective_from_ms: i64,
        buy_leg: LegCostTerms,
        sell_leg: LegCostTerms,
    ) -> Self {
        Self {
            version_id,
            pair,
            route,
            effective_from_ms,
            buy_leg,
            sell_leg,
        }
    }

    pub fn version_id(&self) -> &StableId {
        &self.version_id
    }

    pub fn pair(&self) -> &MarketPair {
        &self.pair
    }

    pub fn route(&self) -> &PaymentMethod {
        &self.route
    }

    pub fn effective_from_ms(&self) -> i64 {
        self.effective_from_ms
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CostBreakdown {
    pub base_cost_fiat: ExactDecimal,
    pub buffer_fiat: ExactDecimal,
    pub total_fiat: ExactDecimal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetSpread {
    pub buy_costs: CostBreakdown,
    pub sell_costs: CostBreakdown,
    pub total_buy_fiat: ExactDecimal,
    pub net_sell_fiat: ExactDecimal,
    pub net_difference_fiat: ExactDecimal,
    pub net_percent_of_buy_cost: ExactDecimal,
    pub break_even_sell_price: Option<ExactDecimal>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum NetAvailability {
    NotConfigured,
    UnknownCosts,
    ProfileMismatch,
    Available,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompatibleSpread {
    pub route: PaymentMethod,
    pub asset_quantity: ExactDecimal,
    pub gross_buy_fiat: ExactDecimal,
    pub gross_sell_fiat: ExactDecimal,
    pub gross_difference_fiat: ExactDecimal,
    pub gross_percent_of_buy_cost: ExactDecimal,
    pub net_availability: NetAvailability,
    pub net: Option<NetSpread>,
}

pub fn compatible_spread(
    pair: &MarketPair,
    asset_quantity: ExactDecimal,
    route: &PaymentMethod,
    filters: &EligibilityFilters,
    buy_ad: &ValidatedAd,
    sell_ad: &ValidatedAd,
    costs: Option<&CostProfile>,
) -> Result<CompatibleSpread, CalculationError> {
    if buy_ad.advertiser_side() != UserIntent::BuyAsset.expected_advertiser_side()
        || sell_ad.advertiser_side() != UserIntent::SellAsset.expected_advertiser_side()
    {
        return Err(CalculationError::IncompatibleSides);
    }
    if !buy_ad.payments().contains(route)
        || !sell_ad.payments().contains(route)
        || (!filters.selected_payments().is_empty() && !filters.selected_payments().contains(route))
    {
        return Err(CalculationError::IncompatibleRoute);
    }
    let amount = RequestedAmount::new(asset_quantity, AmountMode::Asset)?;
    let buy = evaluate_eligibility(UserIntent::BuyAsset, amount, filters, buy_ad)?;
    let sell = evaluate_eligibility(UserIntent::SellAsset, amount, filters, sell_ad)?;
    if !buy.eligible() || !sell.eligible() {
        return Err(CalculationError::IneligibleLeg);
    }

    let gross_buy_fiat = buy.fiat_amount();
    let gross_sell_fiat = sell.fiat_amount();
    let gross_difference_fiat = gross_sell_fiat.checked_sub(gross_buy_fiat)?;
    let gross_percent_of_buy_cost = gross_difference_fiat
        .checked_div(gross_buy_fiat)?
        .checked_mul(ExactDecimal::HUNDRED)?;

    let (net_availability, net) = match costs {
        None => (NetAvailability::NotConfigured, None),
        Some(profile) if profile.pair() != pair || profile.route() != route => {
            (NetAvailability::ProfileMismatch, None)
        }
        Some(profile) => {
            let net = calculate_net(
                asset_quantity,
                buy_ad.price(),
                sell_ad.price(),
                gross_buy_fiat,
                gross_sell_fiat,
                &profile.buy_leg,
                &profile.sell_leg,
            )?;
            (
                if net.is_some() {
                    NetAvailability::Available
                } else {
                    NetAvailability::UnknownCosts
                },
                net,
            )
        }
    };

    Ok(CompatibleSpread {
        route: route.clone(),
        asset_quantity,
        gross_buy_fiat,
        gross_sell_fiat,
        gross_difference_fiat,
        gross_percent_of_buy_cost,
        net_availability,
        net,
    })
}

fn calculate_net(
    asset_quantity: ExactDecimal,
    buy_price: ExactDecimal,
    sell_price: ExactDecimal,
    gross_buy_fiat: ExactDecimal,
    gross_sell_fiat: ExactDecimal,
    buy_terms: &LegCostTerms,
    sell_terms: &LegCostTerms,
) -> Result<Option<NetSpread>, CalculationError> {
    let (Some(buy_terms), Some(sell_terms)) = (buy_terms.known_values(), sell_terms.known_values())
    else {
        return Ok(None);
    };
    let buy_costs = buy_terms.calculate(gross_buy_fiat, buy_price)?;
    let sell_costs = sell_terms.calculate(gross_sell_fiat, sell_price)?;
    let total_buy_fiat = gross_buy_fiat.checked_add(buy_costs.total_fiat)?;
    let net_sell_fiat = gross_sell_fiat.checked_sub(sell_costs.total_fiat)?;
    let net_difference_fiat = net_sell_fiat.checked_sub(total_buy_fiat)?;
    let net_percent_of_buy_cost = net_difference_fiat
        .checked_div(total_buy_fiat)?
        .checked_mul(ExactDecimal::HUNDRED)?;
    let break_even_sell_price = sell_terms.break_even_sell_price(asset_quantity, total_buy_fiat)?;
    Ok(Some(NetSpread {
        buy_costs,
        sell_costs,
        total_buy_fiat,
        net_sell_fiat,
        net_difference_fiat,
        net_percent_of_buy_cost,
        break_even_sell_price,
    }))
}

#[derive(Clone, Copy)]
struct KnownLegCostTerms {
    fixed_fiat: ExactDecimal,
    percent_fiat: ExactDecimal,
    fixed_asset: ExactDecimal,
    minimum_fiat: Option<ExactDecimal>,
    maximum_fiat: Option<ExactDecimal>,
    buffer_fixed_fiat: ExactDecimal,
    buffer_percent_fiat: ExactDecimal,
}

impl KnownLegCostTerms {
    fn raw_base_cost(
        self,
        gross_fiat: ExactDecimal,
        price: ExactDecimal,
    ) -> Result<ExactDecimal, CalculationError> {
        let percent = gross_fiat
            .checked_mul(self.percent_fiat)?
            .checked_div(ExactDecimal::HUNDRED)?;
        let asset = self.fixed_asset.checked_mul(price)?;
        Ok(self.fixed_fiat.checked_add(percent)?.checked_add(asset)?)
    }

    fn clamped_base_cost(self, raw: ExactDecimal) -> ExactDecimal {
        let lower = self.minimum_fiat.map_or(raw, |minimum| raw.max(minimum));
        self.maximum_fiat
            .map_or(lower, |maximum| lower.min(maximum))
    }

    fn calculate(
        self,
        gross_fiat: ExactDecimal,
        price: ExactDecimal,
    ) -> Result<CostBreakdown, CalculationError> {
        let raw = self.raw_base_cost(gross_fiat, price)?;
        let base_cost_fiat = self.clamped_base_cost(raw);
        let variable_buffer = gross_fiat
            .checked_mul(self.buffer_percent_fiat)?
            .checked_div(ExactDecimal::HUNDRED)?;
        let buffer_fiat = self.buffer_fixed_fiat.checked_add(variable_buffer)?;
        let total_fiat = base_cost_fiat.checked_add(buffer_fiat)?;
        Ok(CostBreakdown {
            base_cost_fiat,
            buffer_fiat,
            total_fiat,
        })
    }

    fn break_even_sell_price(
        self,
        asset_quantity: ExactDecimal,
        target_net_fiat: ExactDecimal,
    ) -> Result<Option<ExactDecimal>, CalculationError> {
        let hundred = ExactDecimal::HUNDRED;
        let buffer_rate_asset = asset_quantity
            .checked_mul(self.buffer_percent_fiat)?
            .checked_div(hundred)?;
        let percent_rate_asset = asset_quantity
            .checked_mul(self.percent_fiat)?
            .checked_div(hundred)?;
        let variable_base_rate = self.fixed_asset.checked_add(percent_rate_asset)?;

        // Check the lower-clamped, ordinary, then upper-clamped linear regions.
        if let Some(minimum) = self.minimum_fiat {
            let numerator = target_net_fiat
                .checked_add(self.buffer_fixed_fiat)?
                .checked_add(minimum)?;
            let denominator = asset_quantity.checked_sub(buffer_rate_asset)?;
            if denominator.is_positive() {
                let candidate = numerator.checked_div(denominator)?;
                let gross = asset_quantity.checked_mul(candidate)?;
                if self.raw_base_cost(gross, candidate)? <= minimum {
                    return Ok(Some(candidate));
                }
            }
        }

        let numerator = target_net_fiat
            .checked_add(self.buffer_fixed_fiat)?
            .checked_add(self.fixed_fiat)?;
        let denominator = asset_quantity
            .checked_sub(buffer_rate_asset)?
            .checked_sub(variable_base_rate)?;
        if denominator.is_positive() {
            let candidate = numerator.checked_div(denominator)?;
            let gross = asset_quantity.checked_mul(candidate)?;
            let raw = self.raw_base_cost(gross, candidate)?;
            let at_or_above_min = self.minimum_fiat.is_none_or(|minimum| raw >= minimum);
            let at_or_below_max = self.maximum_fiat.is_none_or(|maximum| raw <= maximum);
            if at_or_above_min && at_or_below_max {
                return Ok(Some(candidate));
            }
        }

        if let Some(maximum) = self.maximum_fiat {
            let numerator = target_net_fiat
                .checked_add(self.buffer_fixed_fiat)?
                .checked_add(maximum)?;
            let denominator = asset_quantity.checked_sub(buffer_rate_asset)?;
            if denominator.is_positive() {
                let candidate = numerator.checked_div(denominator)?;
                let gross = asset_quantity.checked_mul(candidate)?;
                if self.raw_base_cost(gross, candidate)? >= maximum {
                    return Ok(Some(candidate));
                }
            }
        }
        Ok(None)
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

    fn ad(id: &str, side: AdvertiserSide, price: &str, routes: &[&str]) -> ValidatedAd {
        ValidatedAd::new(ValidatedAdInput {
            stable_id: StableId::new(id).expect("ad id"),
            advertiser_side: side,
            price: decimal(price),
            min_fiat: decimal("1"),
            max_fiat: decimal("100000"),
            available_asset: decimal("1000"),
            payments: routes
                .iter()
                .map(|route| PaymentMethod::new(*route).expect("payment"))
                .collect::<BTreeSet<_>>(),
            merchant: MerchantFacts::new(
                StableId::new(format!("merchant-{id}")).expect("merchant id"),
                100,
                decimal("99"),
                decimal("99"),
                true,
            )
            .expect("merchant"),
            observed_at_ms: 100,
        })
        .expect("ad")
    }

    fn pair() -> MarketPair {
        MarketPair::new(
            crate::Symbol::new("USDT").expect("asset"),
            crate::Symbol::new("EGP").expect("fiat"),
        )
        .expect("pair")
    }

    #[test]
    fn compatible_spread_requires_the_exact_same_payment_route() {
        let bank = PaymentMethod::new("BANK").expect("route");
        let result = compatible_spread(
            &pair(),
            decimal("10"),
            &bank,
            &EligibilityFilters::neutral(),
            &ad("buy", AdvertiserSide::Sell, "50", &["BANK"]),
            &ad("sell", AdvertiserSide::Buy, "51", &["CASH"]),
            None,
        );
        assert_eq!(result, Err(CalculationError::IncompatibleRoute));
    }

    #[test]
    fn unknown_cost_is_distinct_from_explicit_zero_and_suppresses_net() {
        let bank = PaymentMethod::new("BANK").expect("route");
        let buy = ad("buy", AdvertiserSide::Sell, "50", &["BANK"]);
        let sell = ad("sell", AdvertiserSide::Buy, "51", &["BANK"]);
        let unknown = CostProfile::new(
            StableId::new("v-unknown").expect("id"),
            pair(),
            bank.clone(),
            0,
            LegCostTerms::unknown(),
            LegCostTerms::unknown(),
        );
        let hidden = compatible_spread(
            &pair(),
            decimal("10"),
            &bank,
            &EligibilityFilters::neutral(),
            &buy,
            &sell,
            Some(&unknown),
        )
        .expect("gross spread");
        assert_eq!(hidden.net_availability, NetAvailability::UnknownCosts);
        assert!(hidden.net.is_none());

        let zero = CostProfile::new(
            StableId::new("v-zero").expect("id"),
            pair(),
            bank.clone(),
            0,
            LegCostTerms::explicit_zero(),
            LegCostTerms::explicit_zero(),
        );
        let shown = compatible_spread(
            &pair(),
            decimal("10"),
            &bank,
            &EligibilityFilters::neutral(),
            &buy,
            &sell,
            Some(&zero),
        )
        .expect("spread");
        assert_eq!(shown.net_availability, NetAvailability::Available);
        let net = shown.net.expect("explicit zero enables net");
        assert_eq!(net.net_difference_fiat.canonical(), "10");
        assert_eq!(
            net.break_even_sell_price.expect("break even").canonical(),
            "50"
        );
    }

    #[test]
    fn profile_pair_or_route_mismatch_is_explicit_and_never_applied() {
        let bank = PaymentMethod::new("BANK").expect("route");
        let other_pair = MarketPair::new(
            crate::Symbol::new("BTC").expect("asset"),
            crate::Symbol::new("EGP").expect("fiat"),
        )
        .expect("pair");
        let profile = CostProfile::new(
            StableId::new("v-other-pair").expect("id"),
            other_pair,
            bank.clone(),
            0,
            LegCostTerms::explicit_zero(),
            LegCostTerms::explicit_zero(),
        );
        let spread = compatible_spread(
            &pair(),
            decimal("10"),
            &bank,
            &EligibilityFilters::neutral(),
            &ad("buy", AdvertiserSide::Sell, "50", &["BANK"]),
            &ad("sell", AdvertiserSide::Buy, "51", &["BANK"]),
            Some(&profile),
        )
        .expect("gross spread");
        assert_eq!(spread.net_availability, NetAvailability::ProfileMismatch);
        assert!(spread.net.is_none());
    }

    #[test]
    fn known_costs_and_buffers_are_applied_to_the_correct_leg_direction() {
        let fixed = |amount: &str| {
            LegCostTerms::new(LegCostTermsInput {
                fixed_fiat: CostInput::known(decimal(amount)).expect("known"),
                percent_fiat: CostInput::known(ExactDecimal::ZERO).expect("known"),
                fixed_asset: CostInput::known(ExactDecimal::ZERO).expect("known"),
                minimum_fiat: None,
                maximum_fiat: None,
                buffer_fixed_fiat: CostInput::known(decimal("1")).expect("known"),
                buffer_percent_fiat: CostInput::known(ExactDecimal::ZERO).expect("known"),
            })
            .expect("terms")
        };
        let bank = PaymentMethod::new("BANK").expect("route");
        let profile = CostProfile::new(
            StableId::new("v-cost").expect("id"),
            pair(),
            bank.clone(),
            0,
            fixed("2"),
            fixed("3"),
        );
        let spread = compatible_spread(
            &pair(),
            decimal("10"),
            &bank,
            &EligibilityFilters::neutral(),
            &ad("buy", AdvertiserSide::Sell, "50", &["BANK"]),
            &ad("sell", AdvertiserSide::Buy, "51", &["BANK"]),
            Some(&profile),
        )
        .expect("spread");
        let net = spread.net.expect("known costs");
        assert_eq!(net.total_buy_fiat.canonical(), "503");
        assert_eq!(net.net_sell_fiat.canonical(), "506");
        assert_eq!(net.net_difference_fiat.canonical(), "3");
    }
}
