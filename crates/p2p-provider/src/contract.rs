use std::collections::BTreeSet;
use std::str::FromStr;

use p2p_domain::{
    AdvertiserSide, ExactDecimal, MarketPair, MerchantFacts, PaymentMethod, RequestSide, StableId,
    UserIntent, ValidatedAd, ValidatedAdInput,
};
use serde::{Deserialize, Serialize};
use serde_json::{Number, Value};
use thiserror::Error;

use crate::policy::{MAX_PROVIDER_TOTAL, PAGE_SIZE};

const SUCCESS_CODE: &str = "000000";
const MAX_DISPLAY_TEXT_CHARS: usize = 80;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebSearchPageRequest {
    fiat: String,
    page: u8,
    rows: u8,
    #[serde(rename = "tradeType")]
    trade_type: RequestSide,
    asset: String,
    countries: Vec<String>,
    #[serde(rename = "proMerchantAds")]
    pro_merchant_ads: bool,
    #[serde(rename = "publisherType")]
    publisher_type: Option<String>,
    #[serde(rename = "payTypes")]
    pay_types: Vec<String>,
    #[serde(rename = "transAmount", skip_serializing_if = "Option::is_none")]
    transaction_amount: Option<String>,
}

impl WebSearchPageRequest {
    pub fn new(
        pair: &MarketPair,
        intent: UserIntent,
        page: u8,
        transaction_amount: Option<ExactDecimal>,
        payment_methods: &BTreeSet<PaymentMethod>,
    ) -> Result<Self, ContractError> {
        if page == 0 || page > 50 {
            return Err(ContractError::InvalidPage);
        }
        Ok(Self {
            fiat: pair.fiat().as_str().to_owned(),
            page,
            rows: PAGE_SIZE,
            trade_type: intent.request_side(),
            asset: pair.asset().as_str().to_owned(),
            countries: Vec::new(),
            pro_merchant_ads: false,
            publisher_type: None,
            pay_types: payment_methods
                .iter()
                .map(|method| method.as_str().to_owned())
                .collect(),
            transaction_amount: transaction_amount.map(ExactDecimal::canonical),
        })
    }

    pub fn page(&self) -> u8 {
        self.page
    }

    pub fn trade_type(&self) -> RequestSide {
        self.trade_type
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct SafeProviderText(String);

impl SafeProviderText {
    pub(crate) fn new(value: &str) -> Option<Self> {
        let sanitized = value
            .trim()
            .chars()
            .filter(|character| !character.is_control())
            .take(MAX_DISPLAY_TEXT_CHARS)
            .collect::<String>();
        (!sanitized.is_empty()).then_some(Self(sanitized))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedAd {
    pub ad: ValidatedAd,
    pub public_nickname: Option<SafeProviderText>,
    pub merchant_active_seconds: u64,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RecordRejectionCode {
    MissingField,
    InvalidIdentifier,
    InvalidDecimal,
    InvalidRange,
    MissingPayment,
    NotTradable,
    InvalidMerchantMetric,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordRejection {
    pub code: RecordRejectionCode,
    pub field: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ValidatedPage {
    pub provider_total: u32,
    pub fetched: u32,
    pub ads: Vec<NormalizedAd>,
    pub rejections: Vec<RecordRejection>,
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ContractError {
    #[error("provider page must be between 1 and 50")]
    InvalidPage,
    #[error("provider response is not valid JSON for the expected envelope")]
    InvalidEnvelope,
    #[error("provider rejected the request")]
    ProviderRejected,
    #[error("provider total is outside the accepted range")]
    InvalidTotal,
    #[error("provider returned a row for the wrong advertiser side")]
    WrongSide,
    #[error("provider returned a row for a different market pair")]
    CrossPair,
}

#[derive(Debug, Deserialize)]
struct RawEnvelope {
    code: String,
    success: bool,
    total: u64,
    data: Vec<Value>,
}

#[derive(Debug, Deserialize)]
struct RawRow {
    adv: RawAdvertisement,
    advertiser: RawAdvertiser,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawAdvertisement {
    adv_no: String,
    trade_type: String,
    asset: String,
    fiat_unit: String,
    price: DecimalToken,
    min_single_trans_amount: DecimalToken,
    max_single_trans_amount: Option<DecimalToken>,
    dynamic_max_single_trans_amount: Option<DecimalToken>,
    tradable_quantity: DecimalToken,
    trade_methods: Vec<RawTradeMethod>,
    is_tradable: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawAdvertiser {
    user_no: String,
    nick_name: Option<String>,
    month_order_count: u64,
    month_finish_rate: DecimalToken,
    positive_rate: DecimalToken,
    merchant_group_member: bool,
    active_time_in_second: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawTradeMethod {
    identifier: Option<String>,
    pay_type: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum DecimalToken {
    Text(String),
    Number(Number),
}

impl DecimalToken {
    pub(crate) fn parse(&self) -> Result<ExactDecimal, ()> {
        let text = match self {
            Self::Text(value) => value.clone(),
            Self::Number(value) => value.to_string(),
        };
        ExactDecimal::from_str(&text).map_err(|_| ())
    }
}

pub fn validate_web_search_page(
    bytes: &[u8],
    pair: &MarketPair,
    intent: UserIntent,
    observed_at_ms: i64,
) -> Result<ValidatedPage, ContractError> {
    let envelope: RawEnvelope =
        serde_json::from_slice(bytes).map_err(|_| ContractError::InvalidEnvelope)?;
    if !envelope.success || envelope.code != SUCCESS_CODE {
        return Err(ContractError::ProviderRejected);
    }
    let provider_total = u32::try_from(envelope.total).map_err(|_| ContractError::InvalidTotal)?;
    if provider_total > MAX_PROVIDER_TOTAL {
        return Err(ContractError::InvalidTotal);
    }
    let fetched = u32::try_from(envelope.data.len()).map_err(|_| ContractError::InvalidEnvelope)?;
    if fetched > u32::from(PAGE_SIZE) {
        return Err(ContractError::InvalidEnvelope);
    }

    let mut ads = Vec::with_capacity(envelope.data.len());
    let mut rejections = Vec::new();
    for value in envelope.data {
        let raw: RawRow = match serde_json::from_value(value) {
            Ok(row) => row,
            Err(_) => {
                rejections.push(RecordRejection {
                    code: RecordRejectionCode::MissingField,
                    field: "row",
                });
                continue;
            }
        };
        match normalize_row(raw, pair, intent, observed_at_ms) {
            Ok(ad) => ads.push(ad),
            Err(RowError::Hard(error)) => return Err(error),
            Err(RowError::Reject(rejection)) => rejections.push(rejection),
        }
    }

    Ok(ValidatedPage {
        provider_total,
        fetched,
        ads,
        rejections,
    })
}

enum RowError {
    Hard(ContractError),
    Reject(RecordRejection),
}

fn rejection(code: RecordRejectionCode, field: &'static str) -> RowError {
    RowError::Reject(RecordRejection { code, field })
}

fn normalize_row(
    raw: RawRow,
    pair: &MarketPair,
    intent: UserIntent,
    observed_at_ms: i64,
) -> Result<NormalizedAd, RowError> {
    let advertiser_side = match raw.adv.trade_type.as_str() {
        "BUY" => AdvertiserSide::Buy,
        "SELL" => AdvertiserSide::Sell,
        _ => return Err(RowError::Hard(ContractError::WrongSide)),
    };
    if advertiser_side != intent.expected_advertiser_side() {
        return Err(RowError::Hard(ContractError::WrongSide));
    }
    if raw.adv.asset != pair.asset().as_str() || raw.adv.fiat_unit != pair.fiat().as_str() {
        return Err(RowError::Hard(ContractError::CrossPair));
    }
    if !raw.adv.is_tradable {
        return Err(rejection(
            RecordRejectionCode::NotTradable,
            "adv.isTradable",
        ));
    }

    let stable_id = StableId::new(raw.adv.adv_no)
        .map_err(|_| rejection(RecordRejectionCode::InvalidIdentifier, "adv.advNo"))?;
    let merchant_id = StableId::new(raw.advertiser.user_no)
        .map_err(|_| rejection(RecordRejectionCode::InvalidIdentifier, "advertiser.userNo"))?;
    let price = parse_decimal(&raw.adv.price, "adv.price")?;
    let min_fiat = parse_decimal(&raw.adv.min_single_trans_amount, "adv.minSingleTransAmount")?;
    let available_asset = parse_decimal(&raw.adv.tradable_quantity, "adv.tradableQuantity")?;
    let max_fiat = effective_maximum(
        raw.adv.max_single_trans_amount.as_ref(),
        raw.adv.dynamic_max_single_trans_amount.as_ref(),
    )?;

    if !price.is_positive()
        || min_fiat.is_negative()
        || !max_fiat.is_positive()
        || max_fiat < min_fiat
        || !available_asset.is_positive()
    {
        return Err(rejection(RecordRejectionCode::InvalidRange, "adv.range"));
    }

    let mut payments = BTreeSet::new();
    for method in raw.adv.trade_methods {
        let Some(identifier) = method.identifier.or(method.pay_type) else {
            return Err(rejection(
                RecordRejectionCode::MissingPayment,
                "adv.tradeMethods.identifier",
            ));
        };
        let payment = PaymentMethod::new(identifier).map_err(|_| {
            rejection(
                RecordRejectionCode::InvalidIdentifier,
                "adv.tradeMethods.identifier",
            )
        })?;
        payments.insert(payment);
    }
    if payments.is_empty() {
        return Err(rejection(
            RecordRejectionCode::MissingPayment,
            "adv.tradeMethods",
        ));
    }

    let completion_percent = ratio_as_percent(
        &raw.advertiser.month_finish_rate,
        "advertiser.monthFinishRate",
    )?;
    let positive_percent =
        ratio_as_percent(&raw.advertiser.positive_rate, "advertiser.positiveRate")?;
    let merchant = MerchantFacts::new(
        merchant_id,
        raw.advertiser.month_order_count,
        completion_percent,
        positive_percent,
        raw.advertiser.merchant_group_member,
    )
    .map_err(|_| rejection(RecordRejectionCode::InvalidMerchantMetric, "advertiser"))?;
    let ad = ValidatedAd::new(ValidatedAdInput {
        stable_id,
        advertiser_side,
        price,
        min_fiat,
        max_fiat,
        available_asset,
        payments,
        merchant,
        observed_at_ms,
    })
    .map_err(|_| rejection(RecordRejectionCode::InvalidRange, "adv"))?;

    Ok(NormalizedAd {
        ad,
        public_nickname: raw
            .advertiser
            .nick_name
            .as_deref()
            .and_then(SafeProviderText::new),
        merchant_active_seconds: raw.advertiser.active_time_in_second,
    })
}

fn parse_decimal(token: &DecimalToken, field: &'static str) -> Result<ExactDecimal, RowError> {
    token
        .parse()
        .map_err(|_| rejection(RecordRejectionCode::InvalidDecimal, field))
}

fn effective_maximum(
    fixed: Option<&DecimalToken>,
    dynamic: Option<&DecimalToken>,
) -> Result<ExactDecimal, RowError> {
    match (fixed, dynamic) {
        (Some(fixed), Some(dynamic)) => Ok(parse_decimal(fixed, "adv.maxSingleTransAmount")?
            .min(parse_decimal(dynamic, "adv.dynamicMaxSingleTransAmount")?)),
        (Some(fixed), None) => parse_decimal(fixed, "adv.maxSingleTransAmount"),
        (None, Some(dynamic)) => parse_decimal(dynamic, "adv.dynamicMaxSingleTransAmount"),
        (None, None) => Err(rejection(
            RecordRejectionCode::MissingField,
            "adv.maxSingleTransAmount",
        )),
    }
}

fn ratio_as_percent(token: &DecimalToken, field: &'static str) -> Result<ExactDecimal, RowError> {
    let ratio = parse_decimal(token, field)?;
    if ratio.is_negative() || ratio > ExactDecimal::ONE {
        return Err(rejection(RecordRejectionCode::InvalidMerchantMetric, field));
    }
    ratio
        .checked_mul(ExactDecimal::HUNDRED)
        .map_err(|_| rejection(RecordRejectionCode::InvalidMerchantMetric, field))
}

#[cfg(test)]
mod tests {
    use super::*;
    use p2p_domain::Symbol;
    use serde_json::json;

    fn pair() -> MarketPair {
        MarketPair::new(
            Symbol::new("USDT").expect("asset"),
            Symbol::new("EGP").expect("fiat"),
        )
        .expect("pair")
    }

    fn synthetic_row(advertiser_side: &str) -> Value {
        json!({
            "adv": {
                "advNo": "synthetic-ad-001",
                "tradeType": advertiser_side,
                "asset": "USDT",
                "fiatUnit": "EGP",
                "price": "50.125",
                "minSingleTransAmount": "500",
                "maxSingleTransAmount": "25000",
                "dynamicMaxSingleTransAmount": "20000",
                "tradableQuantity": "900.5",
                "tradeMethods": [{"identifier": "SYNTHETIC_PAY"}],
                "isTradable": true
            },
            "advertiser": {
                "userNo": "synthetic-merchant-001",
                "nickName": "<img src=x onerror=alert(1)>\u{0000}",
                "monthOrderCount": 321,
                "monthFinishRate": 0.9875,
                "positiveRate": "0.995",
                "merchantGroupMember": true,
                "activeTimeInSecond": 42
            },
            "ignoredUnknownObject": {"never": "retained"}
        })
    }

    fn envelope(row: Value, total: u64) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "code": "000000",
            "success": true,
            "total": total,
            "data": [row],
            "message": null,
            "newUnknownField": "isolated"
        }))
        .expect("fixture")
    }

    #[test]
    fn request_uses_correct_user_intent_side_and_exact_amount_string() {
        let request = WebSearchPageRequest::new(
            &pair(),
            UserIntent::BuyAsset,
            1,
            Some(ExactDecimal::from_str("10000.00").expect("amount")),
            &BTreeSet::new(),
        )
        .expect("request");
        let value = serde_json::to_value(request).expect("serialize");
        assert_eq!(value["tradeType"], "BUY");
        assert_eq!(value["transAmount"], "10000");
        assert_eq!(value["rows"], 20);
        assert!(value.get("classifies").is_none());
    }

    #[test]
    fn valid_row_preserves_exact_decimals_and_uses_conservative_dynamic_maximum() {
        let page = validate_web_search_page(
            &envelope(synthetic_row("SELL"), 1),
            &pair(),
            UserIntent::BuyAsset,
            123,
        )
        .expect("valid page");
        assert_eq!(page.ads.len(), 1);
        let ad = &page.ads[0];
        assert_eq!(ad.ad.price().canonical(), "50.125");
        assert_eq!(ad.ad.max_fiat().canonical(), "20000");
        assert_eq!(ad.ad.merchant().completion_percent().canonical(), "98.75");
        assert_eq!(ad.ad.merchant().positive_percent().canonical(), "99.5");
        assert_eq!(
            ad.public_nickname.as_ref().expect("nickname").as_str(),
            "<img src=x onerror=alert(1)>"
        );
        assert_eq!(ad.merchant_active_seconds, 42);
    }

    #[test]
    fn wrong_side_or_cross_pair_is_a_hard_contract_failure() {
        assert_eq!(
            validate_web_search_page(
                &envelope(synthetic_row("BUY"), 1),
                &pair(),
                UserIntent::BuyAsset,
                1,
            ),
            Err(ContractError::WrongSide)
        );
        let mut row = synthetic_row("SELL");
        row["adv"]["fiatUnit"] = json!("USD");
        assert_eq!(
            validate_web_search_page(&envelope(row, 1), &pair(), UserIntent::BuyAsset, 1,),
            Err(ContractError::CrossPair)
        );
    }

    #[test]
    fn malformed_record_is_counted_without_retaining_raw_provider_data() {
        let mut row = synthetic_row("SELL");
        row["adv"]["price"] = json!("NaN");
        let page = validate_web_search_page(&envelope(row, 1), &pair(), UserIntent::BuyAsset, 1)
            .expect("record-level rejection");
        assert!(page.ads.is_empty());
        assert_eq!(
            page.rejections,
            vec![RecordRejection {
                code: RecordRejectionCode::InvalidDecimal,
                field: "adv.price"
            }]
        );
    }

    #[test]
    fn envelope_and_provider_rejection_fail_closed() {
        assert_eq!(
            validate_web_search_page(b"not-json", &pair(), UserIntent::BuyAsset, 1),
            Err(ContractError::InvalidEnvelope)
        );
        let rejected = serde_json::to_vec(&json!({
            "code": "000002", "success": false, "total": 0, "data": []
        }))
        .expect("fixture");
        assert_eq!(
            validate_web_search_page(&rejected, &pair(), UserIntent::BuyAsset, 1),
            Err(ContractError::ProviderRejected)
        );
    }
}
