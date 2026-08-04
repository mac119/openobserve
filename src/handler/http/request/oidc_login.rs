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

//! Open-source (AGPL) standard-OIDC SSO login endpoints.
//!
//! These are the non-enterprise counterparts of the Dex-based
//! `/config/dex_login` and `/config/redirect` handlers. They implement a
//! self-contained OIDC authorization-code + PKCE flow (see
//! [`crate::service::oidc`]) so SSO works without the closed-source
//! `o2_dex` / `o2_enterprise` crates.
//!
//! Registered only when the "enterprise" feature is NOT enabled.

#![cfg(not(feature = "enterprise"))]

use std::str::FromStr;

use axum::{
    body::Body,
    extract::Query,
    http::{StatusCode, header},
    response::Response,
};
use axum_extra::extract::cookie::{Cookie, SameSite};
use bytes::Bytes;
use config::{
    get_config, ider,
    meta::user::{DBUser, UserOrg, UserRole},
    utils::{base64, json, rand::generate_random_string},
};

use crate::{
    common::meta::user::AuthTokens,
    handler::http::auth::validator::PKCE_STATE_ORG,
    service::{db, oidc, session::set_session, users},
};

/// `GET /config/dex_login` (open-source): start the OIDC login flow.
///
/// Builds the provider authorization URL, persists the CSRF `state` and PKCE
/// `code_verifier` (keyed by state) so the callback can complete the exchange,
/// and returns the URL as a JSON string (matching the enterprise contract the
/// frontend expects).
pub async fn dex_login() -> Response {
    let cfg = get_config();
    if !cfg.auth.oidc_enabled {
        return json_response(StatusCode::BAD_REQUEST, "\"SSO (OIDC) is not enabled\"");
    }

    match oidc::authorize_url().await {
        Ok(start) => {
            // Persist code_verifier keyed by state. Reused (and deleted) in callback.
            if let Err(e) =
                crate::service::kv::set(PKCE_STATE_ORG, &start.state, Bytes::from(start.code_verifier))
                    .await
            {
                log::error!("[oidc] failed to persist PKCE state: {e}");
                return json_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "\"failed to start SSO login\"",
                );
            }
            crate::common::meta::http::HttpResponse::json(start.url)
        }
        Err(e) => {
            log::error!("[oidc] authorize_url failed: {e}");
            json_response(StatusCode::INTERNAL_SERVER_ERROR, "\"failed to start SSO login\"")
        }
    }
}

/// `GET /config/redirect` (open-source): OIDC callback.
///
/// Validates `state`, exchanges `code` for tokens, verifies the `id_token`
/// against the provider JWKS, auto-provisions the user on first login, creates
/// a server session and sets the auth cookie, then redirects to the app.
pub async fn redirect(Query(query): Query<std::collections::HashMap<String, String>>) -> Response {
    let cfg = get_config();
    if !cfg.auth.oidc_enabled {
        return json_response(StatusCode::BAD_REQUEST, "\"SSO (OIDC) is not enabled\"");
    }

    // Provider may report an error back on the redirect.
    if let Some(err) = query.get("error") {
        let desc = query.get("error_description").cloned().unwrap_or_default();
        log::warn!("[oidc] provider returned error: {err} {desc}");
        return json_response(StatusCode::UNAUTHORIZED, "\"SSO authentication failed\"");
    }

    let code = match query.get("code") {
        Some(c) => c,
        None => return plain_response(StatusCode::BAD_REQUEST, "no code in request"),
    };

    // Validate state and recover the PKCE code_verifier.
    let state = match query.get("state") {
        Some(s) => s,
        None => return plain_response(StatusCode::BAD_REQUEST, "no state in request"),
    };
    let code_verifier = match crate::service::kv::get(PKCE_STATE_ORG, state).await {
        Ok(v) => {
            let _ = crate::service::kv::delete(PKCE_STATE_ORG, state).await;
            String::from_utf8_lossy(&v).to_string()
        }
        Err(_) => return plain_response(StatusCode::BAD_REQUEST, "invalid state in request"),
    };

    // Exchange the code for tokens.
    let tokens = match oidc::exchange_code(code, &code_verifier).await {
        Ok(t) => t,
        Err(e) => {
            log::error!("[oidc] token exchange failed: {e}");
            return json_response(StatusCode::UNAUTHORIZED, "\"token exchange failed\"");
        }
    };

    if tokens.id_token.is_empty() {
        return json_response(StatusCode::UNAUTHORIZED, "\"no id_token returned by provider\"");
    }

    // Verify the id_token against the provider JWKS.
    let jwks = match oidc::jwks().await {
        Ok(j) => j,
        Err(e) => {
            log::error!("[oidc] failed to fetch JWKS: {e}");
            return json_response(StatusCode::UNAUTHORIZED, "\"failed to fetch provider keys\"");
        }
    };

    let verified = crate::common::utils::jwt::verify_decode_token(
        &tokens.id_token,
        &jwks,
        &cfg.auth.oidc_client_id,
        true,
        true,
    );
    let (validation, _decoded) = match verified {
        Ok(v) => v,
        Err(e) => {
            log::error!("[oidc] id_token verification failed: {e}");
            return json_response(StatusCode::UNAUTHORIZED, "\"invalid id_token\"");
        }
    };

    let user_email = validation.user_email.trim().to_lowercase();
    if user_email.is_empty() {
        return json_response(StatusCode::UNAUTHORIZED, "\"id_token has no email claim\"");
    }

    // Auto-provision (or refresh) the user and obtain a Basic-auth credential
    // that the open-source `validate_credentials` path accepts (matched against
    // the user's `password_ext`).
    let basic_auth = match ensure_user(&user_email, &validation.user_name).await {
        Ok(v) => v,
        Err(e) => {
            log::error!("[oidc] failed to provision user {user_email}: {e}");
            return json_response(StatusCode::UNAUTHORIZED, "\"failed to provision user\"");
        }
    };

    // Store the Basic credential server-side, keyed by an opaque session id, and
    // hand the browser only `session <id>` in the cookie.
    let session_id = ider::uuid();
    if set_session(&session_id, &basic_auth).await.is_none() {
        log::error!("[oidc] failed to store session {session_id}");
    }
    let access_token = format!("session {session_id}");

    let auth_tokens = AuthTokens {
        access_token,
        refresh_token: tokens.refresh_token.unwrap_or_default(),
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

    // Redirect to the web UI home.
    let login_url = "/web/";

    Response::builder()
        .status(StatusCode::FOUND)
        .header(header::LOCATION, login_url)
        .header(header::SET_COOKIE, auth_cookie.to_string())
        .body(Body::empty())
        .unwrap()
}

/// Ensure the SSO user exists and return a `Basic <base64(email:secret)>`
/// credential valid for the open-source auth path.
///
/// On every login we (re)generate a random per-session `secret` and store it as
/// the user's `password_ext`. `validate_credentials` accepts a request whose
/// password equals `password_ext`, so the session credential authenticates
/// without the user ever having a real password.
async fn ensure_user(email: &str, name: &str) -> Result<String, anyhow::Error> {
    let cfg = get_config();
    let secret = generate_random_string(40);
    let (first_name, last_name) = name.split_once(' ').unwrap_or((name, ""));

    if db::user::get_user_by_email(email).await.is_some() {
        // Existing user: refresh password_ext to the new session secret,
        // preserving the stored password/first/last as needed.
        let record = db::user::get_user_record(email).await;
        let (fn_, ln_) = match &record {
            Ok(r) => (r.first_name.clone(), r.last_name.clone()),
            Err(_) => (first_name.to_owned(), last_name.to_owned()),
        };
        let existing_password = record.map(|r| r.password).unwrap_or_default();
        db::user::update(email, &fn_, &ln_, &existing_password, Some(secret.clone())).await?;
    } else {
        // New user: provision into the configured default org/role.
        let role = UserRole::from_str(&cfg.auth.oidc_default_role).unwrap_or(UserRole::Admin);
        let org = cfg.auth.oidc_default_org.clone();
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

fn json_response(status: StatusCode, body: &'static str) -> Response {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .unwrap()
}

fn plain_response(status: StatusCode, body: &'static str) -> Response {
    Response::builder()
        .status(status)
        .body(Body::from(body))
        .unwrap()
}
