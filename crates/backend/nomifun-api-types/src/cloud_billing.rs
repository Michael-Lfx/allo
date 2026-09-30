//! Flowy cloud billing DTOs.
//!
//! Mirrors the official website's overseas checkout: the catalog is listed per
//! currency, and each currency has exactly one payment channel —
//! `CNY` → WeChat Pay (`wechatpay`, Native QR), `USD` → Airwallex (`airwallex`).

use serde::{Deserialize, Serialize};

/// Payment channel code for CNY SKUs (WeChat Pay Native QR).
pub const CLOUD_BILLING_CHANNEL_WECHATPAY: &str = "wechatpay";
/// Payment channel code for USD SKUs (Airwallex card drop-in).
pub const CLOUD_BILLING_CHANNEL_AIRWALLEX: &str = "airwallex";

/// Catalog currency the desktop checkout can list and pay in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CloudBillingCurrency {
    #[default]
    Usd,
    Cny,
}

impl CloudBillingCurrency {
    /// Parse a `currency` query value; anything other than `CNY` falls back to USD.
    pub fn from_query(value: Option<&str>) -> Self {
        match value.map(str::trim) {
            Some(value) if value.eq_ignore_ascii_case("CNY") => Self::Cny,
            _ => Self::Usd,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Self::Usd => "USD",
            Self::Cny => "CNY",
        }
    }

    /// The only payment channel this currency is paid with.
    pub fn pay_channel(self) -> &'static str {
        match self {
            Self::Usd => CLOUD_BILLING_CHANNEL_AIRWALLEX,
            Self::Cny => CLOUD_BILLING_CHANNEL_WECHATPAY,
        }
    }
}

/// Normalize a client-supplied pay channel; only WeChat Pay and Airwallex are accepted.
pub fn normalize_cloud_billing_pay_channel(value: &str) -> Option<&'static str> {
    match value.trim().to_ascii_lowercase().as_str() {
        CLOUD_BILLING_CHANNEL_WECHATPAY => Some(CLOUD_BILLING_CHANNEL_WECHATPAY),
        CLOUD_BILLING_CHANNEL_AIRWALLEX => Some(CLOUD_BILLING_CHANNEL_AIRWALLEX),
        _ => None,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudBillingPlan {
    pub id: i64,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub plan_period: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub name_en: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub description_en: Option<String>,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub current_price_cent: i64,
    #[serde(default)]
    pub original_price_cent: i64,
    #[serde(default)]
    pub grant_points: i64,
    #[serde(default)]
    pub duration_days: Option<i64>,
    #[serde(default)]
    pub duration_months: Option<i64>,
    #[serde(default)]
    pub is_hot: bool,
    #[serde(default)]
    pub is_current: bool,
    #[serde(default)]
    pub benefit_list: Vec<String>,
    #[serde(default)]
    pub benefit_list_en: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudBillingCreditPack {
    pub id: i64,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub name_en: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub description_en: Option<String>,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub price_cent: i64,
    #[serde(default)]
    pub points: i64,
    #[serde(default)]
    pub valid_days: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudBillingCoupon {
    pub id: i64,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub discount_cent: i64,
    #[serde(default)]
    pub applicable_item_types: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CloudBillingCouponList {
    #[serde(default)]
    pub list: Vec<CloudBillingCoupon>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudBillingPaymentChannel {
    pub code: String,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudBillingCreateOrderRequest {
    pub item_type: String,
    pub item_id: i64,
    pub pay_channel: String,
    pub idempotency_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coupon_id: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_period: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CloudBillingPaymentInfo {
    #[serde(default)]
    pub channel: Option<String>,
    #[serde(default, alias = "payment_intent_id")]
    pub payment_intent_id: Option<String>,
    #[serde(default, alias = "client_secret")]
    pub client_secret: Option<String>,
    #[serde(default)]
    pub intent_id: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub currency: Option<String>,
    /// WeChat Pay Native QR payload (`weixin://wxpay/bizpayurl?...`).
    #[serde(default, alias = "code_url")]
    pub code_url: Option<String>,
    #[serde(default, alias = "channel_name")]
    pub channel_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CloudBillingOrder {
    #[serde(default)]
    pub id: Option<i64>,
    #[serde(default, alias = "order_no")]
    pub order_no: Option<String>,
    #[serde(default, alias = "item_type")]
    pub item_type: Option<String>,
    #[serde(default, alias = "item_id")]
    pub item_id: Option<i64>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub title_en: Option<String>,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default, alias = "amount_cent")]
    pub amount_cent: Option<i64>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default, alias = "pay_channel")]
    pub pay_channel: Option<String>,
    #[serde(default, alias = "expires_at")]
    pub expires_at: Option<String>,
    #[serde(default, alias = "paid_at")]
    pub paid_at: Option<String>,
    #[serde(default)]
    pub payment: Option<CloudBillingPaymentInfo>,
    #[serde(default, alias = "payment_intent_id")]
    pub payment_intent_id: Option<String>,
    #[serde(default, alias = "client_secret")]
    pub client_secret: Option<String>,
    /// Legacy top-level WeChat QR payload (`pay_code_url` on order rows).
    #[serde(default, alias = "pay_code_url")]
    pub pay_code_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CloudBillingAirwallexSession {
    #[serde(default, alias = "payment_intent_id")]
    pub payment_intent_id: Option<String>,
    #[serde(default, alias = "client_secret")]
    pub client_secret: Option<String>,
    #[serde(default)]
    pub intent_id: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_order_serializes_airwallex_camel_case() {
        let req = CloudBillingCreateOrderRequest {
            item_type: "plan".into(),
            item_id: 1,
            pay_channel: "airwallex".into(),
            idempotency_key: "attempt-1".into(),
            coupon_id: Some(9),
            plan_period: Some("MONTH".into()),
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["itemType"], "plan");
        assert_eq!(json["itemId"], 1);
        assert_eq!(json["payChannel"], "airwallex");
        assert_eq!(json["idempotencyKey"], "attempt-1");
        assert_eq!(json["couponId"], 9);
        assert_eq!(json["planPeriod"], "MONTH");
    }

    #[test]
    fn order_accepts_snake_case_cloud_payload() {
        let order: CloudBillingOrder = serde_json::from_value(serde_json::json!({
            "id": 1,
            "order_no": "OPL260305034500123A1B2C3D4E5F67",
            "item_type": "plan",
            "item_id": 1,
            "title": "Pro Monthly",
            "currency": "USD",
            "amount_cent": 1990,
            "status": "CREATED",
            "expires_at": "2026-03-05T12:00:00+08:00",
            "payment": {
                "channel": "airwallex",
                "paymentIntentId": "int_1",
                "clientSecret": "secret_1"
            }
        }))
        .unwrap();
        assert_eq!(order.order_no.as_deref(), Some("OPL260305034500123A1B2C3D4E5F67"));
        assert_eq!(order.amount_cent, Some(1990));
        assert_eq!(
            order.payment.as_ref().and_then(|p| p.payment_intent_id.as_deref()),
            Some("int_1")
        );
        let wire = serde_json::to_value(&order).unwrap();
        assert_eq!(wire["orderNo"], "OPL260305034500123A1B2C3D4E5F67");
        assert_eq!(wire["amountCent"], 1990);
    }

    #[test]
    fn order_keeps_wechat_code_url_for_the_renderer() {
        let order: CloudBillingOrder = serde_json::from_value(serde_json::json!({
            "order_no": "OPK260305034500123A1B2C3D4E5F67",
            "currency": "CNY",
            "amount_cent": 12800,
            "status": "CREATED",
            "pay_channel": "wechatpay",
            "payment": {
                "channel": "wechatpay",
                "codeUrl": "weixin://wxpay/bizpayurl?pr=abc"
            }
        }))
        .unwrap();
        let wire = serde_json::to_value(&order).unwrap();
        assert_eq!(wire["payChannel"], "wechatpay");
        assert_eq!(wire["payment"]["codeUrl"], "weixin://wxpay/bizpayurl?pr=abc");

        let pay: CloudBillingPaymentInfo = serde_json::from_value(serde_json::json!({
            "channel": "wechatpay",
            "channel_name": "微信支付",
            "code_url": "weixin://wxpay/bizpayurl?pr=def"
        }))
        .unwrap();
        assert_eq!(pay.code_url.as_deref(), Some("weixin://wxpay/bizpayurl?pr=def"));
        assert_eq!(pay.channel_name.as_deref(), Some("微信支付"));
    }

    #[test]
    fn currency_maps_to_its_single_pay_channel() {
        assert_eq!(CloudBillingCurrency::from_query(None), CloudBillingCurrency::Usd);
        assert_eq!(CloudBillingCurrency::from_query(Some("jpy")), CloudBillingCurrency::Usd);
        assert_eq!(CloudBillingCurrency::from_query(Some(" cny ")), CloudBillingCurrency::Cny);
        assert_eq!(CloudBillingCurrency::Usd.pay_channel(), "airwallex");
        assert_eq!(CloudBillingCurrency::Cny.pay_channel(), "wechatpay");
        assert_eq!(CloudBillingCurrency::Cny.code(), "CNY");
    }

    #[test]
    fn pay_channel_normalization_rejects_unknown_channels() {
        assert_eq!(normalize_cloud_billing_pay_channel(" WeChatPay "), Some("wechatpay"));
        assert_eq!(normalize_cloud_billing_pay_channel("airwallex"), Some("airwallex"));
        assert_eq!(normalize_cloud_billing_pay_channel("credit_card"), None);
        assert_eq!(normalize_cloud_billing_pay_channel(""), None);
    }
}
