use std::collections::BTreeSet;

use serde::Serialize;

use crate::{
    CalculationError, ExactDecimal, OUTLIER_COEFFICIENT_TEXT, OUTLIER_MINIMUM_SAMPLE,
    OUTLIER_THRESHOLD_TEXT, StableId,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WeightedValue {
    pub value: ExactDecimal,
    pub weight: ExactDecimal,
    pub stable_key: StableId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DescriptiveSummary {
    pub count: usize,
    pub minimum: ExactDecimal,
    pub q1_r7: ExactDecimal,
    pub median_r7: ExactDecimal,
    pub q3_r7: ExactDecimal,
    pub maximum: ExactDecimal,
    pub mean: ExactDecimal,
    pub mad: ExactDecimal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OutlierStatus {
    Typical,
    Outlier,
    InsufficientSample,
    IndeterminateZeroMad,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutlierClassification {
    pub value: ExactDecimal,
    pub modified_z: Option<ExactDecimal>,
    pub status: OutlierStatus,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StabilitySummary {
    pub previous_count: usize,
    pub current_count: usize,
    pub retained_count: usize,
    pub entered_count: usize,
    pub exited_count: usize,
    pub jaccard: ExactDecimal,
    pub churn_of_union: ExactDecimal,
}

pub fn r7_quantile(
    values: &[ExactDecimal],
    probability: ExactDecimal,
) -> Result<ExactDecimal, CalculationError> {
    validate_probability(probability)?;
    if values.is_empty() {
        return Err(CalculationError::EmptySample);
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    if sorted.len() == 1 {
        return Ok(sorted[0]);
    }

    let last_index = sorted.len() - 1;
    let position = probability.checked_mul(ExactDecimal::from_usize(last_index)?)?;
    let lower = position.floor_nonnegative_to_usize()?;
    let upper = lower.saturating_add(1).min(last_index);
    if lower == upper {
        return Ok(sorted[lower]);
    }
    let fraction = position.checked_sub(ExactDecimal::from_usize(lower)?)?;
    let interval = sorted[upper].checked_sub(sorted[lower])?;
    Ok(sorted[lower].checked_add(interval.checked_mul(fraction)?)?)
}

pub fn inverse_weighted_ecdf_quantile(
    values: &[WeightedValue],
    probability: ExactDecimal,
) -> Result<ExactDecimal, CalculationError> {
    validate_probability(probability)?;
    if values.is_empty() {
        return Err(CalculationError::EmptySample);
    }
    if values.iter().any(|item| !item.weight.is_positive()) {
        return Err(CalculationError::InvalidWeights);
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|left, right| {
        left.value
            .cmp(&right.value)
            .then_with(|| left.stable_key.cmp(&right.stable_key))
    });
    let total = ExactDecimal::checked_sum(sorted.iter().map(|item| item.weight))?;
    let target = total.checked_mul(probability)?;
    let mut cumulative = ExactDecimal::ZERO;
    for item in &sorted {
        cumulative = cumulative.checked_add(item.weight)?;
        if cumulative >= target {
            return Ok(item.value);
        }
    }
    Ok(sorted.last().expect("non-empty checked above").value)
}

pub fn arithmetic_mean(values: &[ExactDecimal]) -> Result<ExactDecimal, CalculationError> {
    if values.is_empty() {
        return Err(CalculationError::EmptySample);
    }
    ExactDecimal::checked_sum(values.iter().copied())?
        .checked_div(ExactDecimal::from_usize(values.len())?)
        .map_err(Into::into)
}

pub fn weighted_mean(values: &[WeightedValue]) -> Result<ExactDecimal, CalculationError> {
    if values.is_empty() {
        return Err(CalculationError::EmptySample);
    }
    if values.iter().any(|item| !item.weight.is_positive()) {
        return Err(CalculationError::InvalidWeights);
    }
    let numerator = ExactDecimal::checked_sum(
        values
            .iter()
            .map(|item| item.value.checked_mul(item.weight))
            .collect::<Result<Vec<_>, _>>()?,
    )?;
    let denominator = ExactDecimal::checked_sum(values.iter().map(|item| item.weight))?;
    Ok(numerator.checked_div(denominator)?)
}

pub fn descriptive_summary(
    values: &[ExactDecimal],
) -> Result<DescriptiveSummary, CalculationError> {
    if values.is_empty() {
        return Err(CalculationError::EmptySample);
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let median = r7_quantile(&sorted, decimal_constant("0.5"))?;
    let deviations = sorted
        .iter()
        .map(|value| value.checked_sub(median).map(ExactDecimal::abs))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(DescriptiveSummary {
        count: sorted.len(),
        minimum: sorted[0],
        q1_r7: r7_quantile(&sorted, decimal_constant("0.25"))?,
        median_r7: median,
        q3_r7: r7_quantile(&sorted, decimal_constant("0.75"))?,
        maximum: *sorted.last().expect("non-empty checked above"),
        mean: arithmetic_mean(&sorted)?,
        mad: r7_quantile(&deviations, decimal_constant("0.5"))?,
    })
}

pub fn modified_z_outliers(
    values: &[ExactDecimal],
) -> Result<Vec<OutlierClassification>, CalculationError> {
    if values.len() < OUTLIER_MINIMUM_SAMPLE {
        return Ok(values
            .iter()
            .copied()
            .map(|value| OutlierClassification {
                value,
                modified_z: None,
                status: OutlierStatus::InsufficientSample,
            })
            .collect());
    }
    let summary = descriptive_summary(values)?;
    if summary.mad.is_zero() {
        return Ok(values
            .iter()
            .copied()
            .map(|value| OutlierClassification {
                value,
                modified_z: None,
                status: OutlierStatus::IndeterminateZeroMad,
            })
            .collect());
    }
    let coefficient = decimal_constant(OUTLIER_COEFFICIENT_TEXT);
    let threshold = decimal_constant(OUTLIER_THRESHOLD_TEXT);
    values
        .iter()
        .copied()
        .map(|value| {
            let difference = value.checked_sub(summary.median_r7)?;
            let absolute_scaled_difference = coefficient.checked_mul(difference.abs())?;
            let exact_threshold = threshold.checked_mul(summary.mad)?;
            let modified_z = coefficient
                .checked_mul(difference)?
                .checked_div(summary.mad)?;
            let status = if absolute_scaled_difference > exact_threshold {
                OutlierStatus::Outlier
            } else {
                OutlierStatus::Typical
            };
            Ok(OutlierClassification {
                value,
                modified_z: Some(modified_z),
                status,
            })
        })
        .collect()
}

pub fn herfindahl_hirschman_index(
    nonnegative_amounts: &[ExactDecimal],
) -> Result<ExactDecimal, CalculationError> {
    if nonnegative_amounts.is_empty() {
        return Err(CalculationError::EmptySample);
    }
    if nonnegative_amounts.iter().any(|value| value.is_negative()) {
        return Err(CalculationError::InvalidWeights);
    }
    let total = ExactDecimal::checked_sum(nonnegative_amounts.iter().copied())?;
    if total.is_zero() {
        return Err(CalculationError::InvalidWeights);
    }
    let squared_shares = nonnegative_amounts
        .iter()
        .map(|value| {
            let share = value.checked_div(total)?;
            share.checked_mul(share)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ExactDecimal::checked_sum(squared_shares)?)
}

pub fn top_k_share(
    nonnegative_amounts: &[ExactDecimal],
    k: usize,
) -> Result<ExactDecimal, CalculationError> {
    if nonnegative_amounts.is_empty() || k == 0 {
        return Err(CalculationError::EmptySample);
    }
    if nonnegative_amounts.iter().any(|value| value.is_negative()) {
        return Err(CalculationError::InvalidWeights);
    }
    let total = ExactDecimal::checked_sum(nonnegative_amounts.iter().copied())?;
    if total.is_zero() {
        return Err(CalculationError::InvalidWeights);
    }
    let mut sorted = nonnegative_amounts.to_vec();
    sorted.sort_unstable_by(|left, right| right.cmp(left));
    let top = ExactDecimal::checked_sum(sorted.into_iter().take(k))?;
    Ok(top.checked_div(total)?)
}

pub fn jaccard_index<T: Ord>(
    left: &BTreeSet<T>,
    right: &BTreeSet<T>,
) -> Result<ExactDecimal, CalculationError> {
    let union_count = left.union(right).count();
    if union_count == 0 {
        return Ok(ExactDecimal::ONE);
    }
    let intersection_count = left.intersection(right).count();
    Ok(ExactDecimal::from_usize(intersection_count)?
        .checked_div(ExactDecimal::from_usize(union_count)?)?)
}

pub fn stability_summary<T: Ord>(
    previous: &BTreeSet<T>,
    current: &BTreeSet<T>,
) -> Result<StabilitySummary, CalculationError> {
    let retained_count = previous.intersection(current).count();
    let entered_count = current.difference(previous).count();
    let exited_count = previous.difference(current).count();
    let union_count = retained_count + entered_count + exited_count;
    let jaccard = jaccard_index(previous, current)?;
    let churn_of_union = if union_count == 0 {
        ExactDecimal::ZERO
    } else {
        ExactDecimal::from_usize(entered_count + exited_count)?
            .checked_div(ExactDecimal::from_usize(union_count)?)?
    };
    Ok(StabilitySummary {
        previous_count: previous.len(),
        current_count: current.len(),
        retained_count,
        entered_count,
        exited_count,
        jaccard,
        churn_of_union,
    })
}

fn validate_probability(probability: ExactDecimal) -> Result<(), CalculationError> {
    if probability.is_negative() || probability > ExactDecimal::ONE {
        return Err(CalculationError::InvalidProbability);
    }
    Ok(())
}

fn decimal_constant(value: &str) -> ExactDecimal {
    value.parse().expect("audited exact decimal constant")
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    fn decimal(value: &str) -> ExactDecimal {
        ExactDecimal::from_str(value).expect("fixture decimal")
    }

    #[test]
    fn r7_matches_a_golden_linear_interpolation_fixture() {
        let values = [decimal("1"), decimal("2"), decimal("3"), decimal("4")];
        assert_eq!(
            r7_quantile(&values, decimal("0.25"))
                .expect("quantile")
                .canonical(),
            "1.75"
        );
        assert_eq!(
            r7_quantile(&values, decimal("0.5"))
                .expect("quantile")
                .canonical(),
            "2.5"
        );
    }

    #[test]
    fn weighted_quantile_is_inverse_ecdf_not_interpolated() {
        let values = [
            WeightedValue {
                value: decimal("10"),
                weight: decimal("1"),
                stable_key: StableId::new("a").expect("id"),
            },
            WeightedValue {
                value: decimal("20"),
                weight: decimal("3"),
                stable_key: StableId::new("b").expect("id"),
            },
        ];
        assert_eq!(
            inverse_weighted_ecdf_quantile(&values, decimal("0.25"))
                .expect("weighted quantile")
                .canonical(),
            "10"
        );
        assert_eq!(
            inverse_weighted_ecdf_quantile(&values, decimal("0.2500000001"))
                .expect("weighted quantile")
                .canonical(),
            "20"
        );
    }

    #[test]
    fn nonpositive_weights_fail_closed() {
        let values = [WeightedValue {
            value: decimal("10"),
            weight: ExactDecimal::ZERO,
            stable_key: StableId::new("a").expect("id"),
        }];
        assert_eq!(
            inverse_weighted_ecdf_quantile(&values, decimal("0.5")),
            Err(CalculationError::InvalidWeights)
        );
    }

    #[test]
    fn modified_z_requires_five_values_and_handles_zero_mad() {
        let short = modified_z_outliers(&[decimal("1"), decimal("100")]).expect("classification");
        assert!(
            short
                .iter()
                .all(|item| item.status == OutlierStatus::InsufficientSample)
        );

        let flat = modified_z_outliers(&[decimal("1"); 5]).expect("classification");
        assert!(
            flat.iter()
                .all(|item| item.status == OutlierStatus::IndeterminateZeroMad)
        );
    }

    #[test]
    fn modified_z_flags_only_strictly_beyond_threshold() {
        let values = [
            decimal("1"),
            decimal("2"),
            decimal("3"),
            decimal("4"),
            decimal("100"),
        ];
        let classified = modified_z_outliers(&values).expect("classification");
        assert_eq!(classified[4].status, OutlierStatus::Outlier);
        assert!(
            classified[..4]
                .iter()
                .all(|item| item.status == OutlierStatus::Typical)
        );
    }

    #[test]
    fn concentration_and_overlap_use_explicit_share_bases() {
        let shares = [decimal("50"), decimal("30"), decimal("20")];
        assert_eq!(
            top_k_share(&shares, 2).expect("top share").canonical(),
            "0.8"
        );
        assert_eq!(
            herfindahl_hirschman_index(&shares)
                .expect("hhi")
                .canonical(),
            "0.38"
        );
        let left = BTreeSet::from(["A", "B"]);
        let right = BTreeSet::from(["B", "C"]);
        assert_eq!(
            jaccard_index(&left, &right)
                .expect("jaccard")
                .quantize(6)
                .expect("round")
                .canonical(),
            "0.333333"
        );
    }

    #[test]
    fn weighted_mean_and_stability_are_deterministic() {
        let values = [
            WeightedValue {
                value: decimal("10"),
                weight: decimal("1"),
                stable_key: StableId::new("a").expect("id"),
            },
            WeightedValue {
                value: decimal("20"),
                weight: decimal("3"),
                stable_key: StableId::new("b").expect("id"),
            },
        ];
        assert_eq!(
            weighted_mean(&values).expect("weighted mean").canonical(),
            "17.5"
        );
        let previous = BTreeSet::from(["a", "b"]);
        let current = BTreeSet::from(["b", "c"]);
        let stability = stability_summary(&previous, &current).expect("stability");
        assert_eq!(stability.retained_count, 1);
        assert_eq!(stability.entered_count, 1);
        assert_eq!(stability.exited_count, 1);
        assert_eq!(
            stability
                .churn_of_union
                .quantize(6)
                .expect("round")
                .canonical(),
            "0.666667"
        );
    }

    proptest::proptest! {
        #[test]
        fn r7_quantile_always_stays_inside_the_sample_range(
            raw in proptest::collection::vec(-1_000_i64..1_000, 1..50),
            probability_basis_points in 0_u64..=10_000,
        ) {
            let values = raw.into_iter().map(ExactDecimal::from_i64).collect::<Vec<_>>();
            let probability = ExactDecimal::from_u64(probability_basis_points)
                .checked_div(ExactDecimal::from_u64(10_000)).expect("probability");
            let quantile = r7_quantile(&values, probability).expect("quantile");
            let minimum = *values.iter().min().expect("non-empty");
            let maximum = *values.iter().max().expect("non-empty");
            proptest::prop_assert!(quantile >= minimum && quantile <= maximum);
        }
    }
}
