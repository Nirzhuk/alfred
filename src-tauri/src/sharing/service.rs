use super::links::{ShareLink, TrustedShareOrigin};
use super::owner::{ensure_owner_capability, OwnerCapabilityStore};
use crate::templates::contract::{canonical_bytes, digest, TemplateEnvelope, TemplateError};
use futures_util::StreamExt;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum SharingError {
    #[error("invalid template: {0}")]
    Template(#[from] TemplateError),
    #[error("sharing request failed: {0}")]
    Request(String),
    #[error("sharing service returned {0}")]
    Http(StatusCode),
    #[error("sharing response was invalid")]
    Response,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicationRequest<'a> {
    pub request_id: &'a str,
    pub template: &'a TemplateEnvelope,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Publication {
    pub share_id: String,
    pub url: String,
    pub digest: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedTemplate {
    pub format: String,
    pub digest: String,
    pub template: TemplateEnvelope,
}

pub struct SharingClient {
    client: Client,
    origin: TrustedShareOrigin,
    owner: Box<dyn OwnerCapabilityStore>,
}
// The template itself is capped at 256 KiB. API responses add the envelope,
// digest, and JSON punctuation, so allow a small fixed envelope budget while
// retaining a hard bound on what the desktop buffers.
const MAX_RESPONSE_BYTES: usize = crate::templates::contract::MAX_BYTES + 4096;

async fn bounded_response(response: reqwest::Response) -> Result<Vec<u8>, SharingError> {
    if response
        .content_length()
        .is_some_and(|length| length as usize > MAX_RESPONSE_BYTES)
    {
        return Err(SharingError::Response);
    }
    let mut stream = response.bytes_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| SharingError::Request(error.to_string()))?;
        if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            return Err(SharingError::Response);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
impl SharingClient {
    pub fn origin(&self) -> &TrustedShareOrigin {
        &self.origin
    }
    pub fn new(
        origin: TrustedShareOrigin,
        owner: Box<dyn OwnerCapabilityStore>,
    ) -> Result<Self, SharingError> {
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(20))
            .build()
            .map_err(|error| SharingError::Request(error.to_string()))?;
        Ok(Self {
            client,
            origin,
            owner,
        })
    }
    pub async fn publish(
        &self,
        template: &TemplateEnvelope,
        request_id: Option<&str>,
    ) -> Result<Publication, SharingError> {
        let bytes = canonical_bytes(template)?;
        let expected = digest(template)?;
        let request_id = request_id.unwrap_or_else(|| "");
        let request_id = if request_id.is_empty() {
            return Err(SharingError::Request(
                "publication request ID is required".into(),
            ));
        } else {
            request_id
        };
        let capability =
            ensure_owner_capability(self.owner.as_ref()).map_err(SharingError::Request)?;
        let url = self.origin.create_url().map_err(SharingError::Request)?;
        let response = self
            .client
            .post(url)
            .bearer_auth(capability)
            .header("x-publication-request-id", request_id)
            .header("content-type", "application/json")
            .body(bytes)
            .send()
            .await
            .map_err(|error| SharingError::Request(error.to_string()))?;
        if !response.status().is_success() {
            return Err(SharingError::Http(response.status()));
        }
        let publication: Publication = serde_json::from_slice(&bounded_response(response).await?)
            .map_err(|_| SharingError::Response)?;
        if publication.digest != expected {
            return Err(SharingError::Request(
                "service returned a mismatched template digest".into(),
            ));
        }
        self.origin
            .validate_publication_url(&publication.url, &publication.share_id)
            .map_err(SharingError::Request)?;
        Ok(publication)
    }
    pub async fn fetch(&self, link: &ShareLink) -> Result<SharedTemplate, SharingError> {
        let url = self
            .origin
            .api_url(&link.share_id)
            .map_err(SharingError::Request)?;
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|error| SharingError::Request(error.to_string()))?;
        if !response.status().is_success() {
            return Err(SharingError::Http(response.status()));
        }
        let shared: SharedTemplate = serde_json::from_slice(&bounded_response(response).await?)
            .map_err(|_| SharingError::Response)?;
        if shared.format != crate::templates::contract::FORMAT {
            return Err(SharingError::Response);
        }
        if shared.digest != digest(&shared.template)? {
            return Err(SharingError::Request(
                "shared template digest mismatch".into(),
            ));
        }
        Ok(shared)
    }
    pub async fn revoke(&self, share_id: &str) -> Result<(), SharingError> {
        let url = self
            .origin
            .api_url(share_id)
            .map_err(SharingError::Request)?;
        let capability =
            ensure_owner_capability(self.owner.as_ref()).map_err(SharingError::Request)?;
        let response = self
            .client
            .delete(url)
            .bearer_auth(capability)
            .send()
            .await
            .map_err(|error| SharingError::Request(error.to_string()))?;
        if response.status() == StatusCode::NOT_FOUND
            || response.status() == StatusCode::GONE
            || response.status().is_success()
        {
            Ok(())
        } else {
            Err(SharingError::Http(response.status()))
        }
    }
    pub fn new_request_id() -> String {
        Uuid::new_v4().to_string()
    }
}
