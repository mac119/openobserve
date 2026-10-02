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

//! Open-source (AGPL) SAML 2.0 SP SSO login endpoints (e.g. Tencent Taihu).
//!
//! Three endpoints, mounted under `/config/saml`:
//!   GET  /config/saml/metadata  — SP metadata XML (register this SP at the IdP)
//!   GET  /config/saml/login     — build AuthnRequest, 302 redirect to the IdP
//!   POST /config/saml/acs       — IdP callback: verify the signed assertion,
//!                                 auto-provision the user, issue a session.
//!
//! Registered only when the "enterprise" feature is NOT enabled.

#![cfg(not(feature = "enterprise"))]

use std::str::FromStr;

use axum::{
    body::Body,
    extract::{Form, Query},
    http::{StatusCode, header},
    response::Response,
};
use axum_extra::extract::cookie::{Cookie, SameSite};
use config::{
    get_config, ider,
    meta::user::{DBUser, UserOrg, UserRole},
    utils::{base64, json, rand::generate_random_string},
};
use serde::Deserialize;

use crate::{
    common::meta::user::AuthTokens,
    service::{db, saml, session::set_session, users},
};

/// `GET /config/saml/metadata`: return the SP metadata XML.
pub async fn metadata() -> Response {
    let cfg = get_config();
    if !cfg.auth.saml_enabled {
        return plain_response(StatusCode::BAD_REQUEST, "SAML SSO is not enabled");
    }
    match saml::sp_metadata() {
        Ok(xml) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "application/xml")
            .body(Body::from(xml))
            .unwrap(),
        Err(e) => {
            log::error!("[saml] metadata error: {e}");
            plain_response(StatusCode::INTERNAL_SERVER_ERROR, "failed to build SP metadata")
        }
    }
}

/// `GET /config/saml/login`: 302 redirect the browser to the IdP SSO endpoint.
pub async fn login(Query(query): Query<std::collections::HashMap<String, String>>) -> Response {
    let cfg = get_config();
    if !cfg.auth.saml_enabled {
        return plain_response(StatusCode::BAD_REQUEST, "SAML SSO is not enabled");
    }
    // RelayState is echoed back on the ACS callback; default to the web home.
    let relay_state = query.get("relay_state").cloned().unwrap_or_else(|| "/web/".to_string());

    match saml::login_redirect_url(&relay_state) {
        Ok(url) => Response::builder()
            .status(StatusCode::FOUND)
            .header(header::LOCATION, url)
            .body(Body::empty())
            .unwrap(),
        Err(e) => {
            log::error!("[saml] login error: {e}");
            plain_response(StatusCode::INTERNAL_SERVER_ERROR, "failed to start SAML login")
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct AcsForm {
    #[serde(rename = "SAMLResponse")]
    pub saml_response: String,
    #[serde(rename = "RelayState", default)]
    pub relay_state: Option<String>,
}

/// `POST /config/saml/acs`: IdP assertion consumer service.
///
/// Verifies the signed assertion, auto-provisions the user on first login,
/// creates a server session and sets the auth cookie, then redirects to the
/// app (or to the provided RelayState).
pub async fn acs(Form(form): Form<AcsForm>) -> Response {
    let cfg = get_config();
    if !cfg.auth.saml_enabled {
        return plain_response(StatusCode::BAD_REQUEST, "SAML SSO is not enabled");
    }

    // Verify signature + extract identity.
    let assertion = match saml::verify_and_extract(&form.saml_response) {
        Ok(a) => a,
        Err(e) => {
            log::warn!("[saml] assertion verification failed: {e}");
            return plain_response(StatusCode::UNAUTHORIZED, "SAML authentication failed");
        }
    };

    let user_email = assertion.email.trim().to_lowercase();
    if user_email.is_empty() {
        return plain_response(StatusCode::UNAUTHORIZED, "assertion has no email");
    }

    // Auto-provision (or refresh) the user and obtain a Basic-auth credential
    // the open-source `validate_credentials` path accepts (matched against the
    // user's `password_ext`).
    let basic_auth = match ensure_user(&user_email, &assertion.name).await {
        Ok(v) => v,
        Err(e) => {
            log::error!("[saml] failed to provision user {user_email}: {e}");
            return plain_response(StatusCode::UNAUTHORIZED, "failed to provision user");
        }
    };

    // Store the Basic credential server-side, keyed by an opaque session id;
    // the browser only receives `session <id>` in the cookie.
    let session_id = ider::uuid();
    if set_session(&session_id, &basic_auth).await.is_none() {
        log::error!("[saml] failed to store session {session_id}");
    }
    let access_token = format!("session {session_id}");

    let auth_tokens = AuthTokens {
        access_token,
        refresh_token: String::new(),
    };
    let cookie_val = base64::encode(&json::to_string(&auth_tokens).unwrap());

    let mut auth_cookie = Cookie::new("auth_tokens", cookie_val);
    auth_cookie.set_expires(
        time::OffsetDateTime::now_utc() + time::Duration::seconds(cfg.auth.cookie_max_age),
    );
    auth_cookie.set_http_only(true);
    auth_cookie.set_secure(cfg.auth.cookie_secure_only);
    auth_cookie.set_path("/");
    if cfg.auth.cookie_same_site_lax {
        auth_cookie.set_same_site(SameSite::Lax);
    } else {
        auth_cookie.set_same_site(SameSite::None);
    }

    // The OpenObserve frontend establishes its SSO login state from an
    // `#id_token=<JWT>` fragment on the callback URL (see web views/Login.vue:
    // created() reads this.$route.hash -> getUserInfo). A bare redirect to
    // "/web/" leaves the SPA with no login state and it bounces back to
    // /web/login. So build a frontend-only JWT and redirect with that fragment.
    // API authentication itself rides the session cookie set above.
    let _ = &form.relay_state; // RelayState unused: target is fixed to the login hash route.
    let id_token = frontend_id_token(&user_email, &assertion.name);
    let redirect_to = format!("/web/login#id_token={id_token}");

    Response::builder()
        .status(StatusCode::FOUND)
        .header(header::LOCATION, redirect_to)
        .header(header::SET_COOKIE, auth_cookie.to_string())
        .body(Body::empty())
        .unwrap()
}

/// Ensure the SSO user exists and return `Basic <base64(email:secret)>`.
///
/// A fresh random per-session `secret` is stored as the user's `password_ext`;
/// `validate_credentials` accepts a request whose password equals
/// `password_ext`, so the session credential authenticates without the user
/// ever having a real password.
async fn ensure_user(email: &str, name: &str) -> Result<String, anyhow::Error> {
    let cfg = get_config();
    let secret = generate_random_string(40);
    let (first_name, last_name) = name.split_once(' ').unwrap_or((name, ""));

    if db::user::get_user_by_email(email).await.is_some() {
        let record = db::user::get_user_record(email).await;
        let (fn_, ln_) = match &record {
            Ok(r) => (r.first_name.clone(), r.last_name.clone()),
            Err(_) => (first_name.to_owned(), last_name.to_owned()),
        };
        let existing_password = record.map(|r| r.password).unwrap_or_default();
        db::user::update(email, &fn_, &ln_, &existing_password, Some(secret.clone())).await?;
    } else {
        let role = UserRole::from_str(&cfg.auth.saml_default_role).unwrap_or(UserRole::Admin);
        let org = cfg.auth.saml_default_org.clone();
        let _ = crate::service::organization::check_and_create_org(&org).await;

        let db_user = DBUser {
            email: email.to_owned(),
            first_name: first_name.to_owned(),
            last_name: last_name.to_owned(),
            password: String::new(),
            salt: String::new(),
            organizations: vec![UserOrg {
                name: org.clone(),
                org_name: org,
                token: Default::default(),
                rum_token: Default::default(),
                role,
            }],
            is_external: true,
            password_ext: Some(secret.clone()),
        };
        users::create_new_user(db_user).await?;
    }

    let basic = base64::encode(&format!("{email}:{secret}"));
    Ok(format!("Basic {basic}"))
}

fn plain_response(status: StatusCode, body: &'static str) -> Response {
    Response::builder()
        .status(status)
        .body(Body::from(body))
        .unwrap()
}

/// Build a frontend-only JWT for the OpenObserve web app to establish SSO login
/// state (see web `views/Login.vue`). It is NOT used for API authentication —
/// that rides the session cookie; the frontend only base64url-decodes the
/// payload. The `pgdata` marker makes the open-source login flow take the
/// `store.dispatch("login")` branch and skip its `VerifyAndCreateUser` path, as
/// the user is already provisioned server-side by `ensure_user`.
fn frontend_id_token(email: &str, name: &str) -> String {
    // `::base64` = the external crate; the module `base64` is shadowed here by
    // `use config::utils::base64` at the top of this file.
    use ::base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    let (given, family) = name.split_once(' ').unwrap_or((name, ""));
    let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none","typ":"JWT"}"#);
    let payload_json = serde_json::json!({
        "email": email,
        "sub": email,
        "name": name,
        "given_name": given,
        "family_name": family,
        "pgdata": {},
    })
    .to_string();
    let payload = URL_SAFE_NO_PAD.encode(payload_json.as_bytes());
    // Trailing empty signature keeps the 3-part "header.payload.sig" shape the
    // frontend expects (tokenString.split(".").length === 3).
    format!("{header}.{payload}.")
}
