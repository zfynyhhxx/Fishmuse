use std::{fmt, pin::Pin, time::Duration};

use async_stream::stream;
use futures_core::Stream;
use futures_util::StreamExt;
use reqwest::{
    StatusCode,
    header::{AUTHORIZATION, HeaderValue, RETRY_AFTER},
};
use secrecy::{ExposeSecret, SecretString};
use url::Url;

use super::{events::DeepSeekEventDecoder, request::DeepSeekRequest};
use crate::{AIEvent, AIProvider, AIProviderError, AIRequest};

const OFFICIAL_HOST: &str = "api.deepseek.com";
const DEFAULT_ENDPOINT: &str = "https://api.deepseek.com/responses";
const DEFAULT_MODEL: &str = "deepseek-flash";
const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
const MAX_BUFFER_BYTES: usize = 512 * 1024;

#[derive(Clone, Debug)]
pub struct DeepSeekConfig {
    pub endpoint: Url,
    pub model: String,
    pub request_timeout: Duration,
    pub retry_limit: u8,
}

impl Default for DeepSeekConfig {
    fn default() -> Self {
        Self {
            endpoint: Url::parse(DEFAULT_ENDPOINT).expect("static endpoint is valid"),
            model: DEFAULT_MODEL.to_owned(),
            request_timeout: Duration::from_secs(60),
            retry_limit: 1,
        }
    }
}

pub struct DeepSeekClient {
    http: reqwest::Client,
    api_key: SecretString,
    config: DeepSeekConfig,
}

impl fmt::Debug for DeepSeekClient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DeepSeekClient")
            .field("api_key", &"[REDACTED]")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl DeepSeekClient {
    pub fn new(api_key: SecretString, config: DeepSeekConfig) -> Result<Self, AIProviderError> {
        validate_endpoint(&config.endpoint)?;
        if config.model != DEFAULT_MODEL || api_key.expose_secret().is_empty() {
            return Err(AIProviderError::InvalidConfiguration);
        }
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(config.request_timeout)
            .https_only(true)
            .build()
            .map_err(|_| AIProviderError::InvalidConfiguration)?;
        Ok(Self {
            http,
            api_key,
            config,
        })
    }
}

impl AIProvider for DeepSeekClient {
    fn stream(
        &self,
        request: AIRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<AIEvent, AIProviderError>> + Send>> {
        let http = self.http.clone();
        let endpoint = self.config.endpoint.clone();
        let model = self.config.model.clone();
        let retry_limit = self.config.retry_limit;
        let mut authorization =
            match HeaderValue::from_str(&format!("Bearer {}", self.api_key.expose_secret())) {
                Ok(value) => value,
                Err(_) => {
                    return Box::pin(futures_util::stream::once(async {
                        Err(AIProviderError::InvalidConfiguration)
                    }));
                }
            };
        authorization.set_sensitive(true);

        Box::pin(stream! {
            let mut attempt = 0_u8;
            let response = loop {
                let result = http
                    .post(endpoint.clone())
                    .header(AUTHORIZATION, &authorization)
                    .json(&DeepSeekRequest::new(&model, &request))
                    .send()
                    .await;
                let response = match result {
                    Ok(value) => value,
                    Err(error) if error.is_timeout() => {
                        yield Err(AIProviderError::Timeout);
                        return;
                    }
                    Err(_) => {
                        yield Err(AIProviderError::Transport);
                        return;
                    }
                };
                if response.status() == StatusCode::TOO_MANY_REQUESTS && attempt < retry_limit {
                    let retry_after = response.headers().get(RETRY_AFTER).and_then(|value| value.to_str().ok());
                    let delay = parse_retry_after(retry_after).unwrap_or(Duration::from_secs(1)).min(Duration::from_secs(30));
                    attempt = attempt.saturating_add(1);
                    tokio::time::sleep(delay).await;
                    continue;
                }
                break response;
            };

            if !response.status().is_success() {
                let error = classify_http_status(response.status().as_u16(), response.headers().get(RETRY_AFTER).and_then(|value| value.to_str().ok()));
                yield Err(error);
                return;
            }

            let mut decoder = DeepSeekEventDecoder::new();
            let mut bytes = response.bytes_stream();
            let mut buffer = Vec::new();
            let mut total = 0_usize;
            while let Some(chunk) = bytes.next().await {
                let chunk = match chunk {
                    Ok(value) => value,
                    Err(_) => {
                        yield Err(AIProviderError::StreamInterrupted);
                        return;
                    }
                };
                total = total.saturating_add(chunk.len());
                if total > MAX_RESPONSE_BYTES || buffer.len().saturating_add(chunk.len()) > MAX_BUFFER_BYTES {
                    yield Err(AIProviderError::ResponseTooLarge);
                    return;
                }
                buffer.extend_from_slice(&chunk);
                while let Some(position) = buffer.iter().position(|byte| *byte == b'\n') {
                    let line_bytes = buffer[..position].strip_suffix(b"\r").unwrap_or(&buffer[..position]);
                    let mut line = match std::str::from_utf8(line_bytes) {
                        Ok(value) => value.to_owned(),
                        Err(_) => {
                            yield Err(AIProviderError::Protocol { reason: "stream line is not UTF-8".to_owned() });
                            return;
                        }
                    };
                    buffer.drain(..=position);
                    if line.starts_with(':') || line.is_empty() || line.starts_with("event:") {
                        continue;
                    }
                    if let Some(data) = line.strip_prefix("data:") {
                        line = data.trim_start().to_owned();
                    }
                    match decoder.decode_json_line(&line) {
                        Ok(events) => for event in events { yield Ok(event); },
                        Err(error) => {
                            yield Err(error);
                            return;
                        }
                    }
                }
            }
            if buffer.iter().any(|byte| !byte.is_ascii_whitespace()) {
                yield Err(AIProviderError::StreamInterrupted);
                return;
            }
            if let Err(error) = decoder.finish() {
                yield Err(error);
            }
        })
    }
}

pub fn classify_http_status(status: u16, retry_after: Option<&str>) -> AIProviderError {
    match status {
        401 | 403 => AIProviderError::Unauthorized,
        429 => AIProviderError::RateLimited {
            retry_after: parse_retry_after(retry_after),
        },
        500..=599 => AIProviderError::Server { status },
        _ => AIProviderError::Rejected { status },
    }
}

fn parse_retry_after(value: Option<&str>) -> Option<Duration> {
    value
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_secs)
}

fn validate_endpoint(endpoint: &Url) -> Result<(), AIProviderError> {
    if endpoint.scheme() == "https"
        && endpoint.host_str() == Some(OFFICIAL_HOST)
        && endpoint.path() == "/responses"
        && endpoint.query().is_none()
        && endpoint.fragment().is_none()
    {
        Ok(())
    } else {
        Err(AIProviderError::InvalidConfiguration)
    }
}

#[cfg(test)]
mod tests {
    use secrecy::SecretString;
    use url::Url;

    use super::{DeepSeekClient, DeepSeekConfig};
    use crate::AIProviderError;

    fn secret() -> SecretString {
        SecretString::from("fixture-only".to_owned().into_boxed_str())
    }

    #[test]
    fn rejects_non_official_endpoints_and_unapproved_models() {
        let config = DeepSeekConfig {
            endpoint: Url::parse("https://example.com/responses").expect("URL"),
            ..DeepSeekConfig::default()
        };
        assert!(matches!(
            DeepSeekClient::new(secret(), config),
            Err(AIProviderError::InvalidConfiguration)
        ));

        let config = DeepSeekConfig {
            model: "unapproved-model".to_owned(),
            ..DeepSeekConfig::default()
        };
        assert!(matches!(
            DeepSeekClient::new(secret(), config),
            Err(AIProviderError::InvalidConfiguration)
        ));
    }
}
