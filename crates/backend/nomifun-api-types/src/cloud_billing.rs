//! Flowy cloud billing DTOs (USD catalog + Airwallex checkout).

use serde::{Deserialize, Serialize};

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
}
