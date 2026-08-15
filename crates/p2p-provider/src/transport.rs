use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, Instant};

use reqwest::header::{ACCEPT, CONTENT_TYPE, HeaderValue, RETRY_AFTER};
use thiserror::Error;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::circuit::now_ms;
use crate::contract::WebSearchPageRequest;
use crate::policy::{
    CONNECT_TIMEOUT, MAX_RESPONSE_BYTES, MINIMUM_START_GAP, REQUEST_TIMEOUT, WEB_SEARCH_ENDPOINT,
};

#[derive(Clone, Debug)]
pub struct TransportRequest {
    pub payload: WebSearchPageRequest,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransportResponse {
    pub status: u16,
    pub body: Vec<u8>,
    pub retry_after: Option<Duration>,
    pub received_at_ms: i64,
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum TransportError {
    #[error("provider request was cancelled")]
    Cancelled,
    #[error("provider connection timed out")]
    ConnectTimeout,
    #[error("provider request timed out")]
    RequestTimeout,
    #[error("provider network request failed")]
    Network,
    #[error("provider response exceeded the bounded body limit")]
    BodyTooLarge,
    #[error("provider HTTP client could not be constructed")]
    ClientConfiguration,
}

pub type TransportFuture<'a> =
    Pin<Box<dyn Future<Output = Result<TransportResponse, TransportError>> + Send + 'a>>;

pub trait PageTransport: Send + Sync {
    fn send<'a>(
        &'a self,
        request: TransportRequest,
        cancellation: CancellationToken,
    ) -> TransportFuture<'a>;
}

#[derive(Clone)]
pub struct ReqwestPageTransport {
    client: reqwest::Client,
}

impl ReqwestPageTransport {
    pub fn new() -> Result<Self, TransportError> {
        Ok(Self {
            client: build_http_client()?,
        })
    }
}

pub(crate) fn build_http_client() -> Result<reqwest::Client, TransportError> {
    reqwest::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .referer(false)
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .user_agent(concat!(
            "P2PDesk/",
            env!("CARGO_PKG_VERSION"),
            " experimental-read-only"
        ))
        .build()
        .map_err(|_| TransportError::ClientConfiguration)
}

impl PageTransport for ReqwestPageTransport {
    fn send<'a>(
        &'a self,
        request: TransportRequest,
        cancellation: CancellationToken,
    ) -> TransportFuture<'a> {
        Box::pin(async move {
            let response = tokio::select! {
                () = cancellation.cancelled() => return Err(TransportError::Cancelled),
                result = self.client
                    .post(WEB_SEARCH_ENDPOINT)
                    .header(ACCEPT, HeaderValue::from_static("application/json"))
                    .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
                    .header("clienttype", HeaderValue::from_static("web"))
                    .header("lang", HeaderValue::from_static("en"))
                    .json(&request.payload)
                    .send() => result.map_err(classify_reqwest_error)?,
            };

            collect_bounded_response(response, &cancellation).await
        })
    }
}

pub(crate) async fn collect_bounded_response(
    mut response: reqwest::Response,
    cancellation: &CancellationToken,
) -> Result<TransportResponse, TransportError> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err(TransportError::BodyTooLarge);
    }
    let status = response.status().as_u16();
    let retry_after = response
        .headers()
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_secs);
    let mut body = Vec::new();
    loop {
        let chunk = tokio::select! {
            () = cancellation.cancelled() => return Err(TransportError::Cancelled),
            result = response.chunk() => result.map_err(classify_reqwest_error)?,
        };
        let Some(chunk) = chunk else {
            break;
        };
        if body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            return Err(TransportError::BodyTooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(TransportResponse {
        status,
        body,
        retry_after,
        received_at_ms: now_ms(),
    })
}

fn classify_reqwest_error(error: reqwest::Error) -> TransportError {
    if error.is_connect() && error.is_timeout() {
        TransportError::ConnectTimeout
    } else if error.is_timeout() {
        TransportError::RequestTimeout
    } else {
        TransportError::Network
    }
}

#[derive(Clone, Debug)]
pub struct GlobalRequestGate {
    last_start: Arc<Mutex<Option<Instant>>>,
    minimum_gap: Duration,
}

impl Default for GlobalRequestGate {
    fn default() -> Self {
        Self::production()
    }
}

impl GlobalRequestGate {
    pub fn production() -> Self {
        Self {
            last_start: Arc::new(Mutex::new(None)),
            minimum_gap: MINIMUM_START_GAP,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_gap(minimum_gap: Duration) -> Self {
        Self {
            last_start: Arc::new(Mutex::new(None)),
            minimum_gap,
        }
    }

    pub async fn run<F, T>(
        &self,
        cancellation: &CancellationToken,
        request: F,
    ) -> Result<T, TransportError>
    where
        F: Future<Output = Result<T, TransportError>>,
    {
        let mut last_start = tokio::select! {
            () = cancellation.cancelled() => return Err(TransportError::Cancelled),
            guard = self.last_start.lock() => guard,
        };
        if let Some(previous) = *last_start {
            let elapsed = previous.elapsed();
            if elapsed < self.minimum_gap {
                tokio::select! {
                    () = cancellation.cancelled() => return Err(TransportError::Cancelled),
                    () = tokio::time::sleep(self.minimum_gap - elapsed) => {}
                }
            }
        }
        *last_start = Some(Instant::now());
        tokio::select! {
            () = cancellation.cancelled() => Err(TransportError::Cancelled),
            result = request => result,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_after_accepts_only_documented_nonnegative_integer_seconds() {
        fn parse(value: &str) -> Option<Duration> {
            value.parse::<u64>().ok().map(Duration::from_secs)
        }
        assert_eq!(parse("60"), Some(Duration::from_secs(60)));
        assert_eq!(parse("0"), Some(Duration::ZERO));
        assert_eq!(parse("-1"), None);
        assert_eq!(parse("tomorrow"), None);
    }

    #[tokio::test]
    async fn request_gate_holds_one_in_flight_and_spaces_starts() {
        let gate = GlobalRequestGate::with_gap(Duration::from_millis(30));
        let cancellation = CancellationToken::new();
        let first = gate.run(&cancellation, async {
            tokio::time::sleep(Duration::from_millis(25)).await;
            Ok::<_, TransportError>(Instant::now())
        });
        let second = gate.run(&cancellation, async {
            Ok::<_, TransportError>(Instant::now())
        });
        let started = Instant::now();
        let (_, second_start) = tokio::join!(first, second);
        assert!(started.elapsed() >= Duration::from_millis(30));
        assert!(second_start.is_ok());
    }
}
