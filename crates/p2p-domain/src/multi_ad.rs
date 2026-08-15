use std::collections::BTreeSet;

use serde::Serialize;

use crate::{CalculationError, ExactDecimal, PaymentMethod, StableId, UserIntent, ValidatedAd};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "operation")]
enum OutputRate {
    Multiply { output_per_input: ExactDecimal },
    Divide { input_per_output: ExactDecimal },
}

impl OutputRate {
    fn is_positive(self) -> bool {
        match self {
            Self::Multiply { output_per_input } => output_per_input.is_positive(),
            Self::Divide { input_per_output } => input_per_output.is_positive(),
        }
    }

    fn output(self, input: ExactDecimal) -> Result<ExactDecimal, CalculationError> {
        Ok(match self {
            Self::Multiply { output_per_input } => input.checked_mul(output_per_input)?,
            Self::Divide { input_per_output } => input.checked_div(input_per_output)?,
        })
    }

    fn cmp(self, other: Self) -> Result<std::cmp::Ordering, CalculationError> {
        Ok(match (self, other) {
            (
                Self::Multiply {
                    output_per_input: left,
                },
                Self::Multiply {
                    output_per_input: right,
                },
            ) => left.cmp(&right),
            (
                Self::Divide {
                    input_per_output: left,
                },
                Self::Divide {
                    input_per_output: right,
                },
            ) => right.cmp(&left),
            (
                Self::Multiply {
                    output_per_input: multiplier,
                },
                Self::Divide {
                    input_per_output: divisor,
                },
            ) => multiplier.checked_mul(divisor)?.cmp(&ExactDecimal::ONE),
            (Self::Divide { .. }, Self::Multiply { .. }) => other.cmp(self)?.reverse(),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AllocationCandidate {
    stable_id: StableId,
    rate: OutputRate,
    minimum_input: ExactDecimal,
    maximum_input: ExactDecimal,
    fixed_output_cost: ExactDecimal,
}

impl AllocationCandidate {
    pub fn new(
        stable_id: StableId,
        output_per_input: ExactDecimal,
        minimum_input: ExactDecimal,
        maximum_input: ExactDecimal,
        fixed_output_cost: ExactDecimal,
    ) -> Result<Self, CalculationError> {
        Self::new_with_rate(
            stable_id,
            OutputRate::Multiply { output_per_input },
            minimum_input,
            maximum_input,
            fixed_output_cost,
        )
    }

    fn new_with_rate(
        stable_id: StableId,
        rate: OutputRate,
        minimum_input: ExactDecimal,
        maximum_input: ExactDecimal,
        fixed_output_cost: ExactDecimal,
    ) -> Result<Self, CalculationError> {
        if !rate.is_positive()
            || minimum_input.is_negative()
            || maximum_input.is_negative()
            || fixed_output_cost.is_negative()
            || minimum_input > maximum_input
        {
            return Err(CalculationError::InvalidAllocationRange);
        }
        Ok(Self {
            stable_id,
            rate,
            minimum_input,
            maximum_input,
            fixed_output_cost,
        })
    }

    pub fn stable_id(&self) -> &StableId {
        &self.stable_id
    }

    pub fn minimum_input(&self) -> ExactDecimal {
        self.minimum_input
    }

    pub fn maximum_input(&self) -> ExactDecimal {
        self.maximum_input
    }

    pub fn fixed_output_cost(&self) -> ExactDecimal {
        self.fixed_output_cost
    }
}

pub fn allocation_candidate_from_ad(
    intent: UserIntent,
    route: &PaymentMethod,
    ad: &ValidatedAd,
) -> Result<Option<AllocationCandidate>, CalculationError> {
    if ad.advertiser_side() != intent.expected_advertiser_side() {
        return Err(CalculationError::IncompatibleSides);
    }
    if !ad.payments().contains(route) {
        return Ok(None);
    }
    let (rate, minimum_input, maximum_input) = match intent {
        UserIntent::BuyAsset => (
            OutputRate::Divide {
                input_per_output: ad.price(),
            },
            ad.min_fiat(),
            ad.max_fiat()
                .min(ad.available_asset().checked_mul(ad.price())?),
        ),
        UserIntent::SellAsset => (
            OutputRate::Multiply {
                output_per_input: ad.price(),
            },
            ad.min_fiat().checked_div(ad.price())?,
            ad.max_fiat()
                .checked_div(ad.price())?
                .min(ad.available_asset()),
        ),
    };
    Ok(Some(AllocationCandidate::new_with_rate(
        ad.stable_id().clone(),
        rate,
        minimum_input,
        maximum_input,
        ExactDecimal::ZERO,
    )?))
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AllocationLeg {
    pub stable_id: StableId,
    pub input: ExactDecimal,
    pub gross_output: ExactDecimal,
    pub fixed_output_cost: ExactDecimal,
    pub net_output: ExactDecimal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AllocationMethod {
    CertifiedContinuousSortedGreedy,
    DeterministicMinimumFixedCostRepairEstimate,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OptimalityCertificate {
    pub sorted_stable_ids: Vec<StableId>,
    pub all_minimums_zero: bool,
    pub all_fixed_costs_zero: bool,
    pub prior_legs_filled_to_capacity: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AllocationOutcome {
    pub requested_input: ExactDecimal,
    pub filled_input: ExactDecimal,
    pub remainder_input: ExactDecimal,
    pub total_output: ExactDecimal,
    pub legs: Vec<AllocationLeg>,
    pub method: AllocationMethod,
    pub optimal: bool,
    pub certificate: Option<OptimalityCertificate>,
}

pub fn allocation_frontier(
    requested_inputs: &[ExactDecimal],
    candidates: &[AllocationCandidate],
) -> Result<Vec<AllocationOutcome>, CalculationError> {
    if requested_inputs.is_empty()
        || requested_inputs.iter().any(|amount| !amount.is_positive())
        || requested_inputs.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err(CalculationError::InvalidSensitivityAmounts);
    }
    requested_inputs
        .iter()
        .copied()
        .map(|requested| allocate_across_ads(requested, candidates))
        .collect()
}

pub fn allocate_across_ads(
    requested_input: ExactDecimal,
    candidates: &[AllocationCandidate],
) -> Result<AllocationOutcome, CalculationError> {
    if !requested_input.is_positive() {
        return Err(CalculationError::InvalidAllocationRange);
    }
    let unique = candidates
        .iter()
        .map(|candidate| candidate.stable_id())
        .collect::<BTreeSet<_>>();
    if unique.len() != candidates.len() {
        return Err(CalculationError::DuplicateAllocationId);
    }

    let safe_case = candidates.iter().all(|candidate| {
        candidate.minimum_input().is_zero() && candidate.fixed_output_cost().is_zero()
    });
    let mut sorted = candidates.to_vec();
    sort_candidates(&mut sorted)?;

    let (mut legs, _) = greedy_allocation(requested_input, &sorted)?;
    if !safe_case {
        // Deterministic bounded repair: retry each feasible ad as the first leg,
        // then retain the lexicographically best fill/output/leg/key outcome.
        // This fixes common greedy minimum traps without claiming a global proof.
        for seed in 0..sorted.len() {
            let reordered = std::iter::once(sorted[seed].clone())
                .chain(
                    sorted
                        .iter()
                        .enumerate()
                        .filter(|(index, _)| *index != seed)
                        .map(|(_, candidate)| candidate.clone()),
                )
                .collect::<Vec<_>>();
            let (alternative, _) = greedy_allocation(requested_input, &reordered)?;
            if allocation_is_better(&alternative, &legs)? {
                legs = alternative;
            }
        }
    }

    let filled_input = ExactDecimal::checked_sum(legs.iter().map(|leg| leg.input))?;
    let remainder_input = requested_input.checked_sub(filled_input)?;
    let total_output = ExactDecimal::checked_sum(legs.iter().map(|leg| leg.net_output))?;
    let (method, optimal, certificate) = if safe_case {
        let prior_legs_filled_to_capacity = legs.iter().enumerate().all(|(index, leg)| {
            index + 1 == legs.len()
                || sorted
                    .iter()
                    .find(|candidate| candidate.stable_id() == &leg.stable_id)
                    .is_some_and(|candidate| leg.input == candidate.maximum_input())
        });
        (
            AllocationMethod::CertifiedContinuousSortedGreedy,
            true,
            Some(OptimalityCertificate {
                sorted_stable_ids: sorted
                    .iter()
                    .map(|candidate| candidate.stable_id().clone())
                    .collect(),
                all_minimums_zero: true,
                all_fixed_costs_zero: true,
                prior_legs_filled_to_capacity,
            }),
        )
    } else {
        (
            AllocationMethod::DeterministicMinimumFixedCostRepairEstimate,
            false,
            None,
        )
    };

    Ok(AllocationOutcome {
        requested_input,
        filled_input,
        remainder_input,
        total_output,
        legs,
        method,
        optimal,
        certificate,
    })
}

fn sort_candidates(candidates: &mut [AllocationCandidate]) -> Result<(), CalculationError> {
    for index in 1..candidates.len() {
        let mut cursor = index;
        while cursor > 0 && candidate_precedes(&candidates[cursor], &candidates[cursor - 1])? {
            candidates.swap(cursor, cursor - 1);
            cursor -= 1;
        }
    }
    Ok(())
}

fn candidate_precedes(
    left: &AllocationCandidate,
    right: &AllocationCandidate,
) -> Result<bool, CalculationError> {
    Ok(match left.rate.cmp(right.rate)? {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => left
            .fixed_output_cost()
            .cmp(&right.fixed_output_cost())
            .then_with(|| left.stable_id().cmp(right.stable_id()))
            .is_lt(),
    })
}

fn greedy_allocation(
    requested_input: ExactDecimal,
    candidates: &[AllocationCandidate],
) -> Result<(Vec<AllocationLeg>, ExactDecimal), CalculationError> {
    let mut remaining = requested_input;
    let mut legs = Vec::new();
    for candidate in candidates {
        if remaining.is_zero() {
            break;
        }
        let allocation = remaining.min(candidate.maximum_input());
        if allocation.is_zero() || allocation < candidate.minimum_input() {
            continue;
        }
        let gross_output = candidate.rate.output(allocation)?;
        let net_output = gross_output.checked_sub(candidate.fixed_output_cost())?;
        legs.push(AllocationLeg {
            stable_id: candidate.stable_id().clone(),
            input: allocation,
            gross_output,
            fixed_output_cost: candidate.fixed_output_cost(),
            net_output,
        });
        remaining = remaining.checked_sub(allocation)?;
    }
    Ok((legs, remaining))
}

fn allocation_is_better(
    candidate: &[AllocationLeg],
    incumbent: &[AllocationLeg],
) -> Result<bool, CalculationError> {
    let candidate_fill = ExactDecimal::checked_sum(candidate.iter().map(|leg| leg.input))?;
    let incumbent_fill = ExactDecimal::checked_sum(incumbent.iter().map(|leg| leg.input))?;
    if candidate_fill != incumbent_fill {
        return Ok(candidate_fill > incumbent_fill);
    }
    let candidate_output = ExactDecimal::checked_sum(candidate.iter().map(|leg| leg.net_output))?;
    let incumbent_output = ExactDecimal::checked_sum(incumbent.iter().map(|leg| leg.net_output))?;
    if candidate_output != incumbent_output {
        return Ok(candidate_output > incumbent_output);
    }
    if candidate.len() != incumbent.len() {
        return Ok(candidate.len() < incumbent.len());
    }
    Ok(candidate
        .iter()
        .map(|leg| &leg.stable_id)
        .cmp(incumbent.iter().map(|leg| &leg.stable_id))
        .is_lt())
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

    fn candidate(
        id: &str,
        rate: &str,
        minimum: &str,
        maximum: &str,
        fixed: &str,
    ) -> AllocationCandidate {
        AllocationCandidate::new(
            StableId::new(id).expect("id"),
            decimal(rate),
            decimal(minimum),
            decimal(maximum),
            decimal(fixed),
        )
        .expect("candidate")
    }

    #[test]
    fn continuous_safe_case_has_a_verifiable_optimal_certificate() {
        let outcome = allocate_across_ads(
            decimal("12"),
            &[
                candidate("b", "2", "0", "10", "0"),
                candidate("a", "3", "0", "5", "0"),
            ],
        )
        .expect("allocation");
        assert!(outcome.optimal);
        assert_eq!(
            outcome.method,
            AllocationMethod::CertifiedContinuousSortedGreedy
        );
        assert_eq!(outcome.filled_input.canonical(), "12");
        assert_eq!(outcome.total_output.canonical(), "29");
        assert_eq!(
            outcome
                .legs
                .iter()
                .map(|leg| leg.stable_id.as_str())
                .collect::<Vec<_>>(),
            ["a", "b"]
        );
        assert!(
            outcome
                .certificate
                .expect("certificate")
                .prior_legs_filled_to_capacity
        );
    }

    #[test]
    fn binding_minimum_or_fixed_cost_is_never_labeled_optimal() {
        let outcome = allocate_across_ads(
            decimal("8"),
            &[
                candidate("a", "3", "5", "6", "1"),
                candidate("b", "2", "5", "10", "0"),
            ],
        )
        .expect("allocation");
        assert!(!outcome.optimal);
        assert!(outcome.certificate.is_none());
        assert_eq!(
            outcome.method,
            AllocationMethod::DeterministicMinimumFixedCostRepairEstimate
        );
        assert_eq!(outcome.filled_input.canonical(), "8");
        assert_eq!(outcome.remainder_input.canonical(), "0");
        assert_eq!(outcome.legs[0].stable_id.as_str(), "b");
    }

    #[test]
    fn stable_id_breaks_equal_rate_ties_deterministically() {
        let outcome = allocate_across_ads(
            decimal("1"),
            &[
                candidate("z", "2", "0", "1", "0"),
                candidate("a", "2", "0", "1", "0"),
            ],
        )
        .expect("allocation");
        assert_eq!(outcome.legs[0].stable_id.as_str(), "a");
    }

    #[test]
    fn old_unconstrained_slippage_regression_respects_route_limits_and_availability() {
        let bank = PaymentMethod::new("BANK").expect("payment");
        let old_rounded_reciprocal = ExactDecimal::ONE
            .checked_div(decimal("51"))
            .expect("reciprocal");
        assert!(
            old_rounded_reciprocal
                .checked_mul(decimal("99999999999999999999999999999999999999"))
                .is_err(),
            "precomputing a repeating reciprocal loses valid direct-division semantics"
        );
        let ad = ValidatedAd::new(ValidatedAdInput {
            stable_id: StableId::new("ad-route").expect("id"),
            advertiser_side: AdvertiserSide::Sell,
            price: decimal("51"),
            min_fiat: decimal("500"),
            max_fiat: decimal("1000"),
            available_asset: decimal("15"),
            payments: BTreeSet::from([bank.clone()]),
            merchant: MerchantFacts::new(
                StableId::new("merchant-route").expect("id"),
                100,
                decimal("99"),
                decimal("99"),
                false,
            )
            .expect("merchant"),
            observed_at_ms: 100,
        })
        .expect("ad");
        let candidate = allocation_candidate_from_ad(UserIntent::BuyAsset, &bank, &ad)
            .expect("candidate conversion")
            .expect("route supported");
        assert_eq!(candidate.minimum_input().canonical(), "500");
        assert_eq!(candidate.maximum_input().canonical(), "765");
        let too_small = allocate_across_ads(decimal("499"), std::slice::from_ref(&candidate))
            .expect("heuristic result");
        assert_eq!(too_small.filled_input.canonical(), "0");
        assert!(!too_small.optimal);
        assert!(
            allocation_candidate_from_ad(
                UserIntent::BuyAsset,
                &PaymentMethod::new("CASH").expect("payment"),
                &ad,
            )
            .expect("route filter")
            .is_none()
        );
        let frontier =
            allocation_frontier(&[decimal("500"), decimal("800")], &[candidate]).expect("frontier");
        assert_eq!(frontier[0].filled_input.canonical(), "500");
        assert_eq!(
            frontier[0]
                .total_output
                .quantize(8)
                .expect("display boundary")
                .canonical(),
            "9.80392157"
        );
        assert_eq!(frontier[1].remainder_input.canonical(), "35");

        let large_input = decimal("99999999999999999999999999999999999957");
        let large_ad = ValidatedAd::new(ValidatedAdInput {
            stable_id: StableId::new("ad-large-repeating-price").expect("id"),
            advertiser_side: AdvertiserSide::Sell,
            price: decimal("51"),
            min_fiat: decimal("500"),
            max_fiat: decimal("99999999999999999999999999999999999999"),
            available_asset: decimal("1960784313725490196078431372549019607"),
            payments: BTreeSet::from([bank.clone()]),
            merchant: MerchantFacts::new(
                StableId::new("merchant-large").expect("id"),
                100,
                decimal("99"),
                decimal("99"),
                false,
            )
            .expect("merchant"),
            observed_at_ms: 100,
        })
        .expect("large ad");
        let large_candidate = allocation_candidate_from_ad(UserIntent::BuyAsset, &bank, &large_ad)
            .expect("large candidate conversion")
            .expect("large route supported");
        let large_outcome = allocate_across_ads(large_input, &[large_candidate])
            .expect("large direct-division allocation");
        assert_eq!(large_outcome.filled_input, large_input);
        assert_eq!(
            large_outcome.total_output.canonical(),
            "1960784313725490196078431372549019607"
        );
    }

    proptest::proptest! {
        #[test]
        fn safe_case_never_overfills_requested_input(request in 1_u64..10_000, cap_a in 0_u64..10_000, cap_b in 0_u64..10_000) {
            let outcome = allocate_across_ads(
                ExactDecimal::from_u64(request),
                &[
                    AllocationCandidate::new(
                        StableId::new("a").expect("id"),
                        ExactDecimal::from_i64(2),
                        ExactDecimal::ZERO,
                        ExactDecimal::from_u64(cap_a),
                        ExactDecimal::ZERO,
                    ).expect("candidate"),
                    AllocationCandidate::new(
                        StableId::new("b").expect("id"),
                        ExactDecimal::ONE,
                        ExactDecimal::ZERO,
                        ExactDecimal::from_u64(cap_b),
                        ExactDecimal::ZERO,
                    ).expect("candidate"),
                ],
            ).expect("allocation");
            proptest::prop_assert!(outcome.filled_input <= outcome.requested_input);
            proptest::prop_assert_eq!(
                outcome.filled_input.checked_add(outcome.remainder_input).expect("sum"),
                outcome.requested_input
            );
        }
    }
}
