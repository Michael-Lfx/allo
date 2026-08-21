//! Flowy `/claw` billing catalog and Airwallex order APIs.

use nomifun_api_types::{
    CloudBillingAirwallexSession, CloudBillingCouponList, CloudBillingCreateOrderRequest,
    CloudBillingCreditPack, CloudBillingOrder, CloudBillingPaymentChannel, CloudBillingPlan,
};
use serde_json::Value;

use crate::error::ServerClientError;
use crate::session::ServerSession;

use super::{form_urlencode, FlowyApiClient};

pub(crate) fn billing_plans_path(channel: &str) -> String {
    let code = normalized_channel(channel);
    format!(
        "/plans?currency=USD&externalChannelCode={}",
        form_urlencode(&code)
    )
}

pub(crate) fn billing_credit_packs_path() -> &'static str {
    "/creditPacks/available?currency=USD"
}

pub(crate) fn billing_coupons_path(item_type: Option<&str>) -> String {
    let mut path = String::from("/promo/coupons?status=UNUSED");
    if let Some(item_type) = item_type.map(str::trim).filter(|value| !value.is_empty()) {
        path.push_str("&applicableItemTypes=");
        path.push_str(&form_urlencode(item_type));
    }
    path
}

pub(crate) fn billing_payment_channels_path(
    item_type: &str,
    item_id: i64,
    plan_period: Option<&str>,
) -> String {
    let mut path = format!(
        "/paymentChannels?itemType={}&itemId={}",
        form_urlencode(item_type),
        item_id
    );
    if let Some(period) = plan_period.map(str::trim).filter(|value| !value.is_empty()) {
        path.push_str("&planPeriod=");
        path.push_str(&form_urlencode(period));
    }
    path
}

pub(crate) fn billing_order_by_no_path(order_no: &str) -> String {
    format!(
        "/orders/byOrderNo?orderNo={}",
        form_urlencode(order_no)
    )
}

pub(crate) fn billing_airwallex_init_path(order_no: &str) -> String {
    format!(
        "/orders/{}/pay/airwallex/init",
        form_urlencode(order_no)
    )
}

fn normalized_channel(channel: &str) -> String {
    let trimmed = channel.trim();
    if trimmed.is_empty() {
        "flowy".into()
    } else {
        trimmed.to_string()
    }
}

pub(crate) fn parse_coupon_list(value: Value) -> Result<CloudBillingCouponList, ServerClientError> {
    unwrap_list(value)
        .map(|list| CloudBillingCouponList { list })
        .map_err(|e| ServerClientError::InvalidResponse(format!("coupon list decode failed: {e}")))
}

/// Cloud `GET /paymentChannels` returns `{ list: [...] }` (API.md still shows a bare array).
pub(crate) fn parse_payment_channels(
    value: Value,
) -> Result<Vec<CloudBillingPaymentChannel>, ServerClientError> {
    unwrap_list(value).map_err(|e| {
        ServerClientError::InvalidResponse(format!("payment channel list decode failed: {e}"))
    })
}

fn unwrap_list<T: serde::de::DeserializeOwned>(value: Value) -> Result<Vec<T>, serde_json::Error> {
    if value.is_null() {
        return Ok(Vec::new());
    }
    if let Ok(list) = serde_json::from_value::<Vec<T>>(value.clone()) {
        return Ok(list);
    }
    #[derive(serde::Deserialize)]
    struct Wrapped<T> {
        list: Option<Vec<T>>,
    }
    serde_json::from_value::<Wrapped<T>>(value).map(|wrapped| wrapped.list.unwrap_or_default())
}

impl FlowyApiClient {
    pub async fn list_billing_plans(
        &self,
        session: &ServerSession,
        channel: &str,
    ) -> Result<Vec<CloudBillingPlan>, ServerClientError> {
        self.get_data(&billing_plans_path(channel), Some(session))
            .await
    }

    pub async fn list_billing_credit_packs(
        &self,
        session: &ServerSession,
    ) -> Result<Vec<CloudBillingCreditPack>, ServerClientError> {
        self.get_data(billing_credit_packs_path(), Some(session))
            .await
    }

    pub async fn list_billing_coupons(
        &self,
        session: &ServerSession,
        item_type: Option<&str>,
    ) -> Result<CloudBillingCouponList, ServerClientError> {
        let value: Value = self
            .get_data(&billing_coupons_path(item_type), Some(session))
            .await?;
        parse_coupon_list(value)
    }

    pub async fn list_billing_payment_channels(
        &self,
        session: &ServerSession,
        item_type: &str,
        item_id: i64,
        plan_period: Option<&str>,
    ) -> Result<Vec<CloudBillingPaymentChannel>, ServerClientError> {
        let value: Value = self
            .get_data(
                &billing_payment_channels_path(item_type, item_id, plan_period),
                Some(session),
            )
            .await?;
        parse_payment_channels(value)
    }

    pub async fn create_billing_order(
        &self,
        session: &ServerSession,
        request: &CloudBillingCreateOrderRequest,
    ) -> Result<CloudBillingOrder, ServerClientError> {
        self.post_data("/orders", Some(session), request).await
    }

    pub async fn get_billing_order_by_no(
        &self,
        session: &ServerSession,
        order_no: &str,
    ) -> Result<CloudBillingOrder, ServerClientError> {
        self.get_data(&billing_order_by_no_path(order_no), Some(session))
            .await
    }

    pub async fn init_billing_airwallex(
        &self,
        session: &ServerSession,
        order_no: &str,
    ) -> Result<CloudBillingAirwallexSession, ServerClientError> {
        self.post_data(
            &billing_airwallex_init_path(order_no),
            Some(session),
            &serde_json::json!({}),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_paths_use_usd_and_channel() {
        assert_eq!(
            billing_plans_path(" flowy "),
            "/plans?currency=USD&externalChannelCode=flowy"
        );
        assert_eq!(
            billing_plans_path(""),
            "/plans?currency=USD&externalChannelCode=flowy"
        );
        assert_eq!(billing_credit_packs_path(), "/creditPacks/available?currency=USD");
        assert_eq!(
            billing_coupons_path(Some("plan")),
            "/promo/coupons?status=UNUSED&applicableItemTypes=plan"
        );
        assert_eq!(
            billing_payment_channels_path("pack", 10, None),
            "/paymentChannels?itemType=pack&itemId=10"
        );
        assert_eq!(
            billing_airwallex_init_path("OPL1"),
            "/orders/OPL1/pay/airwallex/init"
        );
    }

    #[test]
    fn coupon_list_accepts_array_or_wrapped_list() {
        let wrapped = parse_coupon_list(serde_json::json!({
            "list": [{ "id": 1, "discountCent": 100, "status": "UNUSED" }]
        }))
        .unwrap();
        assert_eq!(wrapped.list.len(), 1);

        let array = parse_coupon_list(serde_json::json!([
            { "id": 2, "discountCent": 200, "status": "UNUSED" }
        ]))
        .unwrap();
        assert_eq!(array.list[0].id, 2);
    }

    #[test]
    fn payment_channels_accept_cloud_list_wrapper() {
        let wrapped = parse_payment_channels(serde_json::json!({
            "list": [
                { "code": "airwallex", "name": "Airwallex" },
                { "code": "wechatpay", "name": "微信支付" }
            ]
        }))
        .unwrap();
        assert_eq!(wrapped.len(), 2);
        assert_eq!(wrapped[0].code, "airwallex");

        let array = parse_payment_channels(serde_json::json!([
            { "code": "airwallex" }
        ]))
        .unwrap();
        assert_eq!(array[0].code, "airwallex");
    }
}
