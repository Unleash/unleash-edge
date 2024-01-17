use actix_http::header::{HeaderName, HeaderValue, TryIntoHeaderPair};
use actix_web::http::header::{ETag, EntityTag};
use base64::Engine;
use unleash_types::client_features::ClientFeatures;
use xxhash_rust::xxh3;

use crate::error::EdgeError;

pub fn bytes_to_etag(bytes: &[u8]) -> EntityTag {
    let hash = xxh3::xxh3_128(bytes);
    let base64 = base64::prelude::BASE64_URL_SAFE.encode(hash.to_le_bytes());
    let hash = format!("{:x}-{}", bytes.len(), base64);
    EntityTag::new_weak(hash)
}

pub fn bytes_to_etag_header(bytes: &[u8]) -> Result<(HeaderName, HeaderValue), EdgeError> {
    let etag = bytes_to_etag(bytes);
    entity_tag_to_etag_header(&etag)
}

pub fn entity_tag_to_etag_header(etag: &EntityTag) -> Result<(HeaderName, HeaderValue), EdgeError> {
    ETag(etag.clone())
        .try_into_pair()
        .map_err(|_| EdgeError::EdgeTokenError)
}

pub fn client_features_to_etag(features: &ClientFeatures) -> EntityTag {
    let bytes = serde_json::to_vec(features).unwrap();
    bytes_to_etag(&bytes)
}
