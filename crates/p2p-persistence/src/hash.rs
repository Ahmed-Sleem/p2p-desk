use hmac::{Hmac, KeyInit, Mac};
use p2p_domain::{AdvertiserSide, AmountMode, PaymentLogic, UserIntent, ValidatedAd};
use p2p_provider::NormalizedAd;
use sha2::{Digest, Sha256};

use crate::error::{PersistenceError, Result};
use crate::model::{CostProfileInput, SnapshotContext};
use crate::schema::hex_lower;

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone, Debug)]
pub(crate) struct AdPersistenceRecord {
    pub content_hash: String,
    pub ad_key: String,
    pub merchant_key: String,
    pub advertiser_side: &'static str,
    pub price_text: String,
    pub min_fiat_text: String,
    pub max_fiat_text: String,
    pub available_asset_text: String,
    pub monthly_orders: u64,
    pub completion_percent_text: String,
    pub positive_percent_text: String,
    pub is_pro: bool,
    pub merchant_active_seconds: u64,
    pub payment_methods: Vec<String>,
}

pub(crate) fn random_identifier(prefix: &str) -> Result<String> {
    if prefix.len() != 2 || !prefix.bytes().all(|byte| byte.is_ascii_lowercase()) {
        return Err(PersistenceError::InvalidInput(
            "identifier prefix must contain two lowercase ASCII letters".to_owned(),
        ));
    }
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|error| PersistenceError::Entropy(error.to_string()))?;
    Ok(format!("{prefix}{}", hex_lower(&bytes)))
}

pub(crate) fn sha256_bytes(bytes: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(bytes);
    hex_lower(&digest.finalize())
}

pub(crate) fn pseudonym(identity_key: &[u8; 32], namespace: &str, source_id: &str) -> String {
    let mut mac = <HmacSha256 as KeyInit>::new_from_slice(identity_key)
        .expect("HMAC-SHA256 accepts a 32-byte key");
    update_framed_mac(&mut mac, b"p2p-desk-pseudonym-v1");
    update_framed_mac(&mut mac, namespace.as_bytes());
    update_framed_mac(&mut mac, source_id.as_bytes());
    hex_lower(&mac.finalize().into_bytes())
}

pub(crate) fn context_hash(context: &SnapshotContext) -> String {
    let mut digest = Sha256::new();
    update_framed(&mut digest, b"p2p-desk-context-v1");
    update_framed(&mut digest, context.pair.asset().as_str().as_bytes());
    update_framed(&mut digest, context.pair.fiat().as_str().as_bytes());
    update_framed(&mut digest, context.amount.value().canonical().as_bytes());
    update_framed(
        &mut digest,
        match context.amount.mode() {
            AmountMode::Fiat => b"fiat",
            AmountMode::Asset => b"asset",
        },
    );
    update_framed(
        &mut digest,
        match context.filters.payment_logic() {
            PaymentLogic::Any => b"ANY",
            PaymentLogic::All => b"ALL",
        },
    );
    for payment in context.filters.selected_payments() {
        update_framed(&mut digest, payment.as_str().as_bytes());
    }
    update_framed(
        &mut digest,
        context.filters.minimum_orders().to_string().as_bytes(),
    );
    update_framed(
        &mut digest,
        context
            .filters
            .minimum_completion_percent()
            .canonical()
            .as_bytes(),
    );
    update_framed(
        &mut digest,
        context
            .filters
            .minimum_positive_percent()
            .canonical()
            .as_bytes(),
    );
    update_framed(
        &mut digest,
        if context.filters.pro_only() {
            b"1"
        } else {
            b"0"
        },
    );
    update_optional_decimal(
        &mut digest,
        context
            .filters
            .maximum_buy_price()
            .map(|value| value.canonical()),
    );
    update_optional_decimal(
        &mut digest,
        context
            .filters
            .minimum_sell_price()
            .map(|value| value.canonical()),
    );
    update_framed(
        &mut digest,
        context.result_target.value().to_string().as_bytes(),
    );
    hex_lower(&digest.finalize())
}

pub(crate) fn normalized_ad_record(
    normalized: &NormalizedAd,
    identity_key: &[u8; 32],
) -> AdPersistenceRecord {
    let ad = &normalized.ad;
    let ad_key = pseudonym(identity_key, "advertisement", ad.stable_id().as_str());
    let merchant_key = pseudonym(identity_key, "merchant", ad.merchant().stable_id().as_str());
    let advertiser_side = match ad.advertiser_side() {
        AdvertiserSide::Buy => "BUY",
        AdvertiserSide::Sell => "SELL",
    };
    let price_text = ad.price().canonical();
    let min_fiat_text = ad.min_fiat().canonical();
    let max_fiat_text = ad.max_fiat().canonical();
    let available_asset_text = ad.available_asset().canonical();
    let completion_percent_text = ad.merchant().completion_percent().canonical();
    let positive_percent_text = ad.merchant().positive_percent().canonical();
    let payment_methods = ad
        .payments()
        .iter()
        .map(|payment| payment.as_str().to_owned())
        .collect::<Vec<_>>();

    let mut digest = Sha256::new();
    update_framed(&mut digest, b"p2p-desk-normalized-ad-v1");
    for value in [
        ad_key.as_str(),
        merchant_key.as_str(),
        advertiser_side,
        price_text.as_str(),
        min_fiat_text.as_str(),
        max_fiat_text.as_str(),
        available_asset_text.as_str(),
        completion_percent_text.as_str(),
        positive_percent_text.as_str(),
    ] {
        update_framed(&mut digest, value.as_bytes());
    }
    update_framed(
        &mut digest,
        ad.merchant().monthly_orders().to_string().as_bytes(),
    );
    update_framed(
        &mut digest,
        if ad.merchant().is_pro() { b"1" } else { b"0" },
    );
    update_framed(
        &mut digest,
        normalized.merchant_active_seconds.to_string().as_bytes(),
    );
    for payment in &payment_methods {
        update_framed(&mut digest, payment.as_bytes());
    }

    AdPersistenceRecord {
        content_hash: hex_lower(&digest.finalize()),
        ad_key,
        merchant_key,
        advertiser_side,
        price_text,
        min_fiat_text,
        max_fiat_text,
        available_asset_text,
        monthly_orders: ad.merchant().monthly_orders(),
        completion_percent_text,
        positive_percent_text,
        is_pro: ad.merchant().is_pro(),
        merchant_active_seconds: normalized.merchant_active_seconds,
        payment_methods,
    }
}

pub(crate) fn cost_version_hash(profile_id: &str, input: &CostProfileInput) -> String {
    let mut digest = Sha256::new();
    update_framed(&mut digest, b"p2p-desk-cost-version-v1");
    update_framed(&mut digest, profile_id.as_bytes());
    update_framed(&mut digest, input.effective_from_ms.to_string().as_bytes());
    update_framed(
        &mut digest,
        input
            .effective_to_ms
            .map_or_else(|| "unknown".to_owned(), |value| value.to_string())
            .as_bytes(),
    );
    for value in [
        input.fixed_fiat,
        input.percent_fiat,
        input.fixed_asset,
        input.minimum_charge,
        input.maximum_charge,
        input.fixed_buffer,
        input.percent_buffer,
    ] {
        update_optional_decimal(&mut digest, value.map(|decimal| decimal.canonical()));
    }
    update_framed(&mut digest, input.label.as_bytes());
    for value in [&input.source_label, &input.note] {
        update_framed(
            &mut digest,
            value.as_deref().unwrap_or("unknown").as_bytes(),
        );
    }
    hex_lower(&digest.finalize())
}

pub(crate) const fn intent_text(intent: UserIntent) -> &'static str {
    match intent {
        UserIntent::BuyAsset => "buy-asset",
        UserIntent::SellAsset => "sell-asset",
    }
}

pub(crate) fn expected_side(intent: UserIntent) -> AdvertiserSide {
    intent.expected_advertiser_side()
}

fn update_optional_decimal(digest: &mut Sha256, value: Option<String>) {
    match value {
        Some(value) => {
            update_framed(digest, b"known");
            update_framed(digest, value.as_bytes());
        }
        None => update_framed(digest, b"unknown"),
    }
}

fn update_framed(digest: &mut Sha256, value: &[u8]) {
    digest.update((value.len() as u64).to_be_bytes());
    digest.update(value);
}

fn update_framed_mac(mac: &mut HmacSha256, value: &[u8]) {
    mac.update(&(value.len() as u64).to_be_bytes());
    mac.update(value);
}

pub(crate) fn ad_matches_intent(ad: &ValidatedAd, intent: UserIntent) -> bool {
    ad.advertiser_side() == expected_side(intent)
}
