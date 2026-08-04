// Copyright 2026 OpenObserve Inc.
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program.  If not, see <http://www.gnu.org/licenses/>.

//! Open-source, standard OpenID Connect (OIDC) login support.
//!
//! Implements the authorization-code flow with PKCE against any standards
//! compliant OIDC provider (Google, Azure AD, Keycloak, Authing, Okta, ...).
//! This is a self-contained implementation that does NOT depend on the
//! closed-source enterprise `o2_dex` crate, so SSO works in the AGPL build.
//!
//! Flow:
//!   1. `authorize_url()` builds the provider authorization URL (state + PKCE)
//!   2. user authenticates at the IdP, which redirects back with `code`+`state`
//!   3. `exchange_code()` swaps the code for tokens at the token endpoint
//!   4. the `id_token` is verified against the provider JWKS via
//!      `crate::common::utils::jwt::verify_decode_token`

use std::collections::HashMap;

use config::{get_config, utils::rand::generate_random_string};
use serde::Deserialize;

/// Cached OIDC provider discovery document.
#[derive(Debug, Clone, Deserialize)]
pub struct OidcDiscovery {
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub jwks_uri: String,
    #[serde(default)]
    pub userinfo_endpoint: Option<String>,
}

/// Token response from the OIDC token endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct OidcTokenResponse {
    #[serde(default)]
    pub access_token: String,
    #[serde(default)]
    pub id_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
}

/// Data needed to start the login flow: the authorization URL plus the
/// `state` (used for CSRF protection) and the PKCE `code_verifier` that must
/// be replayed on the token exchange.
#[derive(Debug, Clone)]
pub struct OidcLoginStart {
    pub url: String,
    pub state: String,
    pub code_verifier: String,
}

fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap_or_default()
}

fn random_token(len: usize) -> String {
    // Alphanumeric — a valid subset of the PKCE `code_verifier` unreserved
    // character set, and safe to use for the CSRF `state` value.
    generate_random_string(len)
}

/// base64url (no padding) of the SHA-256 of the verifier — the PKCE S256 challenge.
fn pkce_challenge_s256(verifier: &str) -> String {
    // `sha256::digest` returns a lowercase hex string; decode it back to the
    // raw 32 bytes, then base64url-encode (no padding) per RFC 7636.
    let hex_digest = sha256::digest(verifier.as_bytes());
    let raw = hex::decode(hex_digest).unwrap_or_default();
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
    URL_SAFE_NO_PAD.encode(raw)
}

/// Fetch the provider discovery document from `{issuer}/.well-known/openid-configuration`.
pub async fn discovery() -> Result<OidcDiscovery, anyhow::Error> {
    let cfg = get_config();
    let issuer = cfg.auth.oidc_issuer_url.trim_end_matches('/');
    if issuer.is_empty() {
        return Err(anyhow::anyhow!("ZO_OIDC_ISSUER_URL is not set"));
    }
    let url = format!("{issuer}/.well-known/openid-configuration");
    let resp = http_client().get(&url).send().await?;
    if !resp.status().is_success() {
        return Err(anyhow::anyhow!(
            "OIDC discovery failed: {} {}",
            resp.status(),
            url
        ));
    }
    let doc: OidcDiscovery = resp.json().await?;
    Ok(doc)
}

/// Build the authorization URL to which the browser should be redirected.
pub async fn authorize_url() -> Result<OidcLoginStart, anyhow::Error> {
    let cfg = get_config();
    let disc = discovery().await?;

    let state = random_token(32);
    let code_verifier = random_token(64);
    let code_challenge = pkce_challenge_s256(&code_verifier);

    let mut url = url::Url::parse(&disc.authorization_endpoint)
        .map_err(|e| anyhow::anyhow!("invalid authorization_endpoint: {e}"))?;
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", &cfg.auth.oidc_client_id)
        .append_pair("redirect_uri", &cfg.auth.oidc_redirect_url)
        .append_pair("scope", &cfg.auth.oidc_scopes)
        .append_pair("state", &state)
        .append_pair("code_challenge", &code_challenge)
        .append_pair("code_challenge_method", "S256");
    let url = url.to_string();

    Ok(OidcLoginStart {
        url,
        state,
        code_verifier,
    })
}

/// Exchange the authorization `code` for tokens at the token endpoint.
pub async fn exchange_code(
    code: &str,
    code_verifier: &str,
) -> Result<OidcTokenResponse, anyhow::Error> {
    let cfg = get_config();
    let disc = discovery().await?;

    let mut form: HashMap<&str, &str> = HashMap::new();
    form.insert("grant_type", "authorization_code");
    form.insert("code", code);
    form.insert("redirect_uri", &cfg.auth.oidc_redirect_url);
    form.insert("client_id", &cfg.auth.oidc_client_id);
    form.insert("client_secret", &cfg.auth.oidc_client_secret);
    form.insert("code_verifier", code_verifier);

    let resp = http_client()
        .post(&disc.token_endpoint)
        .form(&form)
        .send()
        .await?;

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(anyhow::anyhow!("OIDC token exchange failed: {status} {body}"));
    }
    let tokens: OidcTokenResponse = serde_json::from_str(&body)
        .map_err(|e| anyhow::anyhow!("failed to parse token response: {e} body={body}"))?;
    Ok(tokens)
}

/// Fetch the provider JWKS (as a JSON string) for id_token verification.
pub async fn jwks() -> Result<String, anyhow::Error> {
    let disc = discovery().await?;
    let resp = http_client().get(&disc.jwks_uri).send().await?;
    if !resp.status().is_success() {
        return Err(anyhow::anyhow!("OIDC JWKS fetch failed: {}", resp.status()));
    }
    Ok(resp.text().await?)
}
