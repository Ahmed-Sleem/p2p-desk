use std::str::FromStr;

use p2p_domain::{
    AdvertiserSide, AllocationCandidate, ExactDecimal, FORMULA_METADATA, OutlierStatus,
    RequestSide, StableId, UserIntent, WeightedValue, allocate_across_ads,
    herfindahl_hirschman_index, inverse_weighted_ecdf_quantile, modified_z_outliers, r7_quantile,
};
use serde_json::json;

fn decimal(value: &str) -> ExactDecimal {
    ExactDecimal::from_str(value).expect("golden decimal")
}

#[test]
fn gate_02_versioned_golden_fixture_replays_exactly() {
    let values = [decimal("1"), decimal("2"), decimal("3"), decimal("4")];
    let weighted = [
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
    let outliers = modified_z_outliers(&[
        decimal("1"),
        decimal("2"),
        decimal("3"),
        decimal("4"),
        decimal("100"),
    ])
    .expect("outliers");
    let allocation = allocate_across_ads(
        decimal("12"),
        &[
            AllocationCandidate::new(
                StableId::new("b").expect("id"),
                decimal("2"),
                ExactDecimal::ZERO,
                decimal("10"),
                ExactDecimal::ZERO,
            )
            .expect("candidate"),
            AllocationCandidate::new(
                StableId::new("a").expect("id"),
                decimal("3"),
                ExactDecimal::ZERO,
                decimal("5"),
                ExactDecimal::ZERO,
            )
            .expect("candidate"),
        ],
    )
    .expect("allocation");

    let actual = json!({
        "calculationVersion": p2p_domain::CALCULATION_VERSION,
        "domainSchemaVersion": p2p_domain::DOMAIN_SCHEMA_VERSION,
        "sideMapping": {
            "buyRequest": RequestSide::Buy,
            "buyAdvertiser": AdvertiserSide::Sell,
            "sellRequest": RequestSide::Sell,
            "sellAdvertiser": AdvertiserSide::Buy,
        },
        "halfEvenIntegers": [
            decimal("6.5").quantize(0).expect("round"),
            decimal("7.5").quantize(0).expect("round"),
        ],
        "r7Quartile": r7_quantile(&values, decimal("0.25")).expect("r7"),
        "weightedInverseEcdf": inverse_weighted_ecdf_quantile(&weighted, decimal("0.5"))
            .expect("weighted quantile"),
        "outlierStatuses": outliers.iter().map(|item| item.status).collect::<Vec<OutlierStatus>>(),
        "hhi": herfindahl_hirschman_index(&[decimal("50"), decimal("30"), decimal("20")])
            .expect("hhi"),
        "multiAd": {
            "method": allocation.method,
            "optimal": allocation.optimal,
            "filledInput": allocation.filled_input,
            "remainderInput": allocation.remainder_input,
            "totalOutput": allocation.total_output,
            "stableIds": allocation.legs.iter().map(|leg| leg.stable_id.as_str()).collect::<Vec<_>>(),
        },
        "formulaMetadata": FORMULA_METADATA,
    });
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/gate_02_golden.json")).expect("fixture JSON");
    assert_eq!(actual, expected);

    assert_eq!(
        UserIntent::BuyAsset.expected_advertiser_side(),
        AdvertiserSide::Sell
    );
}
