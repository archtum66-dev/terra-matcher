use matcher_api::{base_amount, Market};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

pub const QUOTE_URL: &str = "https://1click.chaindefuser.com/v0/quote";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SwapType {
    ExactInput,
    ExactOutput,
    FlexInput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DepositType {
    OriginChain,
    Intents,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RefundType {
    OriginChain,
    Intents,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RecipientType {
    DestinationChain,
    Intents,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuoteRequest {
    dry: bool,
    pub swap_type: SwapType,
    pub slippage_tolerance: u32,
    pub origin_asset: String,
    pub deposit_type: DepositType,
    pub destination_asset: String,
    pub amount: String,
    pub refund_to: String,
    pub refund_type: RefundType,
    pub recipient: String,
    pub recipient_type: RecipientType,
    pub deadline: String,
    pub deposit_mode: Option<String>,
    pub referral: Option<String>,
    pub quote_waiting_time_ms: Option<u64>,
    pub app_fees: Option<serde_json::Value>,
}

impl QuoteRequest {
    fn with_dry_true(
        swap_type: SwapType,
        slippage_tolerance: u32,
        origin_asset: String,
        deposit_type: DepositType,
        destination_asset: String,
        amount: String,
        refund_to: String,
        refund_type: RefundType,
        recipient: String,
        recipient_type: RecipientType,
        deadline: String,
    ) -> Self {
        Self {
            dry: true,
            swap_type,
            slippage_tolerance,
            origin_asset,
            deposit_type,
            destination_asset,
            amount,
            refund_to,
            refund_type,
            recipient,
            recipient_type,
            deadline,
            deposit_mode: None,
            referral: None,
            quote_waiting_time_ms: None,
            app_fees: None,
        }
    }
}

impl Serialize for QuoteRequest {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut st = serializer.serialize_struct("QuoteRequest", 13)?;
        st.serialize_field("dry", &true)?;
        st.serialize_field("swapType", &self.swap_type)?;
        st.serialize_field("slippageTolerance", &self.slippage_tolerance)?;
        st.serialize_field("originAsset", &self.origin_asset)?;
        st.serialize_field("depositType", &self.deposit_type)?;
        st.serialize_field("destinationAsset", &self.destination_asset)?;
        st.serialize_field("amount", &self.amount)?;
        st.serialize_field("refundTo", &self.refund_to)?;
        st.serialize_field("refundType", &self.refund_type)?;
        st.serialize_field("recipient", &self.recipient)?;
        st.serialize_field("recipientType", &self.recipient_type)?;
        st.serialize_field("deadline", &self.deadline)?;
        if let Some(ref v) = self.deposit_mode {
            st.serialize_field("depositMode", v)?;
        }
        if let Some(ref v) = self.referral {
            st.serialize_field("referral", v)?;
        }
        if let Some(v) = self.quote_waiting_time_ms {
            st.serialize_field("quoteWaitingTimeMs", &v)?;
        }
        if let Some(ref v) = self.app_fees {
            st.serialize_field("appFees", v)?;
        }
        st.end()
    }
}

impl<'de> Deserialize<'de> for QuoteRequest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct QuoteRequestHelper {
            #[serde(default)]
            dry: Option<bool>,
            swap_type: SwapType,
            slippage_tolerance: u32,
            origin_asset: String,
            deposit_type: DepositType,
            destination_asset: String,
            amount: String,
            refund_to: String,
            refund_type: RefundType,
            recipient: String,
            recipient_type: RecipientType,
            deadline: String,
            #[serde(default)]
            deposit_mode: Option<String>,
            #[serde(default)]
            referral: Option<String>,
            #[serde(default)]
            quote_waiting_time_ms: Option<u64>,
            #[serde(default)]
            app_fees: Option<serde_json::Value>,
        }
        let h = QuoteRequestHelper::deserialize(deserializer)?;
        let _ = h.dry;
        Ok(Self {
            dry: true,
            swap_type: h.swap_type,
            slippage_tolerance: h.slippage_tolerance,
            origin_asset: h.origin_asset,
            deposit_type: h.deposit_type,
            destination_asset: h.destination_asset,
            amount: h.amount,
            refund_to: h.refund_to,
            refund_type: h.refund_type,
            recipient: h.recipient,
            recipient_type: h.recipient_type,
            deadline: h.deadline,
            deposit_mode: h.deposit_mode,
            referral: h.referral,
            quote_waiting_time_ms: h.quote_waiting_time_ms,
            app_fees: h.app_fees,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuoteResponse {
    pub correlation_id: Option<String>,
    pub timestamp: Option<String>,
    pub signature: Option<String>,
    pub quote_request: Option<serde_json::Value>,
    pub quote: Option<Quote>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Quote {
    pub amount_in: Option<String>,
    pub min_amount_in: Option<String>,
    pub amount_out: Option<String>,
    pub min_amount_out: Option<String>,
    pub time_estimate: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportResponse {
    pub status: u16,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    RequestFailed(String),
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RequestFailed(msg) => write!(f, "transport error: {msg}"),
        }
    }
}

impl std::error::Error for TransportError {}

pub trait Transport {
    fn post_json(
        &self,
        url: &str,
        headers: &[(String, String)],
        body: &str,
    ) -> Result<TransportResponse, TransportError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuoteError {
    InvalidInput(String),
    InvalidApiKey(String),
    Transport(TransportError),
    Parse(String),
    MissingQuote,
}

impl fmt::Display for QuoteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(msg) => write!(f, "invalid input: {msg}"),
            Self::InvalidApiKey(msg) => write!(f, "invalid api key: {msg}"),
            Self::Transport(e) => write!(f, "{e}"),
            Self::Parse(msg) => write!(f, "parse error: {msg}"),
            Self::MissingQuote => write!(f, "response missing quote"),
        }
    }
}

impl std::error::Error for QuoteError {}

pub struct FixtureTransport {
    responses: std::collections::HashMap<String, TransportResponse>,
    default: Option<TransportResponse>,
}

impl FixtureTransport {
    pub fn new() -> Self {
        Self {
            responses: std::collections::HashMap::new(),
            default: None,
        }
    }

    pub fn with_response(mut self, url: &str, status: u16, body: &str) -> Self {
        self.responses.insert(
            url.to_string(),
            TransportResponse {
                status,
                body: body.to_string(),
            },
        );
        self
    }

    pub fn with_default(mut self, status: u16, body: &str) -> Self {
        self.default = Some(TransportResponse {
            status,
            body: body.to_string(),
        });
        self
    }
}

impl Default for FixtureTransport {
    fn default() -> Self {
        Self::new()
    }
}

impl Transport for FixtureTransport {
    fn post_json(
        &self,
        url: &str,
        headers: &[(String, String)],
        body: &str,
    ) -> Result<TransportResponse, TransportError> {
        let _ = (headers, body);
        if let Some(r) = self.responses.get(url) {
            return Ok(r.clone());
        }
        if let Some(ref r) = self.default {
            return Ok(r.clone());
        }
        Err(TransportError::RequestFailed(format!("no fixture for {url}")))
    }
}

pub struct RecordingTransport {
    inner: FixtureTransport,
    pub requests: std::sync::Mutex<Vec<RecordedRequest>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedRequest {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl RecordingTransport {
    pub fn new(inner: FixtureTransport) -> Self {
        Self {
            inner,
            requests: std::sync::Mutex::new(Vec::new()),
        }
    }
}

impl Transport for RecordingTransport {
    fn post_json(
        &self,
        url: &str,
        headers: &[(String, String)],
        body: &str,
    ) -> Result<TransportResponse, TransportError> {
        if let Ok(mut guard) = self.requests.lock() {
            guard.push(RecordedRequest {
                url: url.to_string(),
                headers: headers.to_vec(),
                body: body.to_string(),
            });
        }
        self.inner.post_json(url, headers, body)
    }
}

pub struct QuoteClient<T: Transport> {
    transport: T,
    url: String,
    api_key: Option<String>,
}

impl<T: Transport> QuoteClient<T> {
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            url: QUOTE_URL.to_string(),
            api_key: None,
        }
    }

    pub fn with_api_key(mut self, key: String) -> Self {
        self.api_key = Some(key);
        self
    }

    pub fn quote(&self, req: &QuoteRequest) -> Result<QuoteResponse, QuoteError> {
        let body = serde_json::to_string(req).map_err(|e| QuoteError::Parse(e.to_string()))?;
        let mut headers: Vec<(String, String)> = Vec::new();
        if let Some(ref key) = self.api_key {
            headers.push(("X-API-Key".to_string(), key.clone()));
        }
        let resp = self
            .transport
            .post_json(&self.url, &headers, &body)
            .map_err(QuoteError::Transport)?;
        match resp.status {
            400 => return Err(QuoteError::InvalidInput(resp.body)),
            401 => return Err(QuoteError::InvalidApiKey(resp.body)),
            _ => {}
        }
        if resp.status >= 400 {
            return Err(QuoteError::Parse(format!("HTTP {}", resp.status)));
        }
        serde_json::from_str(&resp.body).map_err(|e| QuoteError::Parse(e.to_string()))
    }
}

pub fn reference_quote_request(
    market: &Market,
    fill: &matcher_api::Fill,
    account: &str,
    deadline: &str,
    slippage_bps: u32,
) -> Result<QuoteRequest, crate::error::SettlementError> {
    if fill.market_id != market.market_id {
        return Err(crate::error::SettlementError::MarketIdMismatch);
    }
    let base = base_amount(market, fill.qty_lots).ok_or(crate::error::SettlementError::AmountOverflow)?;
    Ok(QuoteRequest::with_dry_true(
        SwapType::ExactInput,
        slippage_bps,
        market.base_token.clone(),
        DepositType::Intents,
        market.quote_token.clone(),
        base.to_string(),
        account.to_string(),
        RefundType::Intents,
        account.to_string(),
        RecipientType::Intents,
        deadline.to_string(),
    ))
}

pub fn funding_quote_request(
    origin_asset: &str,
    destination_asset: &str,
    amount: &str,
    account: &str,
    refund_to: &str,
    deadline: &str,
    slippage_bps: u32,
) -> QuoteRequest {
    QuoteRequest::with_dry_true(
        SwapType::ExactInput,
        slippage_bps,
        origin_asset.to_string(),
        DepositType::OriginChain,
        destination_asset.to_string(),
        amount.to_string(),
        refund_to.to_string(),
        RefundType::OriginChain,
        account.to_string(),
        RecipientType::Intents,
        deadline.to_string(),
    )
}

#[cfg(feature = "http")]
pub struct HttpTransport;

#[cfg(feature = "http")]
impl Transport for HttpTransport {
    fn post_json(
        &self,
        url: &str,
        headers: &[(String, String)],
        body: &str,
    ) -> Result<TransportResponse, TransportError> {
        let mut req = ureq::post(url).set("Content-Type", "application/json");
        for (k, v) in headers {
            req = req.set(k, v);
        }
        match req.send_string(body) {
            Ok(resp) => {
                let status = resp.status();
                let body = resp.into_string().map_err(|e| {
                    TransportError::RequestFailed(format!("read body: {e}"))
                })?;
                Ok(TransportResponse { status, body })
            }
            Err(ureq::Error::Status(code, resp)) => {
                let body = match resp.into_string() {
                    Ok(s) => s,
                    Err(e) => return Err(TransportError::RequestFailed(format!("read body: {e}"))),
                };
                Ok(TransportResponse {
                    status: code,
                    body,
                })
            }
            Err(e) => Err(TransportError::RequestFailed(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialized_requests_always_dry_true() {
        let market = Market {
            market_id: "NEAR-USDC".to_string(),
            base_token: "nep141:wrap.near".to_string(),
            quote_token: "nep141:usdc.near".to_string(),
            lot_size_base: 1,
            quote_per_tick_lot: 1,
        };
        let fill = matcher_api::Fill {
            seq: 1,
            market_id: "NEAR-USDC".to_string(),
            taker_id: "t".to_string(),
            maker_id: "m".to_string(),
            taker_account: "alice.test".to_string(),
            maker_account: "bob.test".to_string(),
            taker_side: matcher_api::Side::Buy,
            price_ticks: 10,
            qty_lots: 2,
        };
        let ref_req = reference_quote_request(&market, &fill, "alice.test", "2030-01-01T00:00:00Z", 50)
            .expect("ref");
        let fund_req = funding_quote_request(
            "nep141:eth.bridge",
            "nep141:usdc.near",
            "1000",
            "alice.test",
            "0xdead",
            "2030-01-01T00:00:00Z",
            50,
        );
        for req in [ref_req, fund_req] {
            let json = serde_json::to_string(&req).expect("serialize");
            assert!(
                json.contains("\"dry\":true"),
                "expected dry true in {json}"
            );
        }
    }

    #[test]
    fn recording_transport_captures_dry_body() {
        let fixture = FixtureTransport::new().with_default(
            200,
            include_str!("../../tests/fixtures/quote_dry_ok.json"),
        );
        let recording = RecordingTransport::new(fixture);
        let client = QuoteClient::new(recording);
        let req = funding_quote_request(
            "nep141:eth.bridge",
            "nep141:usdc.near",
            "1000",
            "alice.test",
            "0xdead",
            "2030-01-01T00:00:00Z",
            100,
        );
        let _ = client.quote(&req);
        let guard = client.transport.requests.lock().expect("lock");
        assert_eq!(guard.len(), 1);
        assert!(guard[0].body.contains("\"dry\":true"));
    }

    #[test]
    fn quote_client_maps_400_and_401() {
        let req = funding_quote_request(
            "nep141:eth",
            "nep141:usdc.near",
            "1",
            "alice.test",
            "0x1",
            "2030-01-01T00:00:00Z",
            50,
        );
        let t400 = FixtureTransport::new()
            .with_response(QUOTE_URL, 400, include_str!("../../tests/fixtures/quote_error_400.json"));
        assert!(matches!(
            QuoteClient::new(t400).quote(&req),
            Err(QuoteError::InvalidInput(_))
        ));
        let t401 = FixtureTransport::new()
            .with_response(QUOTE_URL, 401, include_str!("../../tests/fixtures/quote_error_401.json"));
        assert!(matches!(
            QuoteClient::new(t401).quote(&req),
            Err(QuoteError::InvalidApiKey(_))
        ));
    }
}
