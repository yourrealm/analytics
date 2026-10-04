//! Google Search Console through a service account: one per analytics user.
//!
//! The user pastes the service account's JSON key; the server signs a JWT
//! with it, trades that for an hour-long access token, and calls the
//! Search Analytics API. Blocking (ureq): callers run it off the runtime.
//!
//! The token endpoint is fixed, never the key file's `token_uri`: a pasted
//! file must not choose where the server sends requests. Tests and e2e point
//! both URLs at a fake via `ANALYTICS_GOOGLE_API` / `ANALYTICS_GOOGLE_TOKEN_URL`.

use serde::{Deserialize, Serialize};
use std::time::Duration;

const SCOPE: &str = "https://www.googleapis.com/auth/webmasters.readonly";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
/// The Search Console API v1 root (its discovery document); the paths are
/// still `webmasters/v3/...`.
const API_BASE: &str = "https://searchconsole.googleapis.com";
/// Rows per breakdown, as on the rest of the dashboard.
const TOP: u32 = 10;

#[derive(Debug, Clone, Deserialize)]
pub struct ServiceAccount {
    pub client_email: String,
    pub private_key: String,
    pub private_key_id: Option<String>,
}

impl ServiceAccount {
    pub fn parse(json: &str) -> Result<Self, String> {
        let sa: ServiceAccount = serde_json::from_str(json)
            .map_err(|_| "not a service account key: expected the JSON file from Google Cloud")?;
        if !sa.client_email.contains('@') {
            return Err("client_email is missing".into());
        }
        Ok(sa)
    }
}

#[derive(Clone)]
pub struct Client {
    agent: ureq::Agent,
    token_url: String,
    api_base: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Totals {
    pub clicks: f64,
    pub impressions: f64,
    pub ctr: f64,
    pub position: f64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct QueryRow {
    pub query: String,
    pub clicks: f64,
    pub impressions: f64,
    pub ctr: f64,
    pub position: f64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Search {
    pub totals: Option<Totals>,
    pub queries: Vec<QueryRow>,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: Option<i64>,
}

#[derive(Deserialize)]
struct Row {
    #[serde(default)]
    keys: Vec<String>,
    clicks: f64,
    impressions: f64,
    ctr: f64,
    position: f64,
}

#[derive(Deserialize)]
struct Rows {
    #[serde(default)]
    rows: Vec<Row>,
}

impl Client {
    pub fn from_env() -> Self {
        let var = |name: &str, default: &str| {
            std::env::var(name)
                .ok()
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| default.to_string())
        };
        Self::new(
            &var("ANALYTICS_GOOGLE_TOKEN_URL", TOKEN_URL),
            &var("ANALYTICS_GOOGLE_API", API_BASE),
        )
    }

    pub fn new(token_url: &str, api_base: &str) -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(15)))
            .http_status_as_error(false)
            .build()
            .new_agent();
        Self {
            agent,
            token_url: token_url.to_string(),
            api_base: api_base.trim_end_matches('/').to_string(),
        }
    }

    /// An access token and its expiry (Unix seconds).
    pub fn token(&self, sa: &ServiceAccount, now: i64) -> Result<(String, i64), String> {
        let claims = serde_json::json!({
            "iss": sa.client_email,
            "scope": SCOPE,
            "aud": self.token_url,
            "iat": now,
            "exp": now + 3600,
        });
        let assertion = crate::crypto::jwt(&sa.private_key, sa.private_key_id.as_deref(), &claims)?;
        let mut res = self
            .agent
            .post(&self.token_url)
            .send_form([
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", assertion.as_str()),
            ])
            .map_err(|e| format!("Google sign-in: {e}"))?;
        let status = res.status().as_u16();
        let body: serde_json::Value = res
            .body_mut()
            .read_json()
            .map_err(|e| format!("Google sign-in: {e}"))?;
        if status != 200 {
            let why = body["error_description"]
                .as_str()
                .or(body["error"].as_str())
                .unwrap_or("unknown error");
            return Err(format!("Google refused the key: {why}"));
        }
        let t: TokenResponse =
            serde_json::from_value(body).map_err(|e| format!("Google sign-in: {e}"))?;
        Ok((t.access_token, now + t.expires_in.unwrap_or(3600) - 60))
    }

    /// The Search Console properties this service account was added to.
    pub fn properties(&self, token: &str) -> Result<Vec<String>, String> {
        let body = self.call(
            self.agent
                .get(format!("{}/webmasters/v3/sites", self.api_base))
                .header("authorization", format!("Bearer {token}"))
                .call(),
        )?;
        let mut sites: Vec<String> = body["siteEntry"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|e| e["permissionLevel"] != "siteUnverifiedUser")
            .filter_map(|e| e["siteUrl"].as_str().map(str::to_string))
            .collect();
        sites.sort();
        Ok(sites)
    }

    /// Totals and top queries for a property between two `YYYY-MM-DD` dates
    /// (Google reads them in Pacific time).
    pub fn search(
        &self,
        token: &str,
        property: &str,
        from: &str,
        to: &str,
    ) -> Result<Search, String> {
        let url = format!(
            "{}/webmasters/v3/sites/{}/searchAnalytics/query",
            self.api_base,
            url::form_urlencoded::byte_serialize(property.as_bytes()).collect::<String>(),
        );
        let query = |dimensions: &[&str], limit: u32| {
            let body = serde_json::json!({
                "startDate": from,
                "endDate": to,
                "dimensions": dimensions,
                "rowLimit": limit,
                // Include the last, still-changing days: the lag is long enough.
                "dataState": "ALL",
            });
            self.call(
                self.agent
                    .post(&url)
                    .header("authorization", format!("Bearer {token}"))
                    .send_json(&body),
            )
            .and_then(|v| serde_json::from_value::<Rows>(v).map_err(|e| format!("Google: {e}")))
        };
        let totals = query(&[], 1)?.rows.into_iter().next().map(|r| Totals {
            clicks: r.clicks,
            impressions: r.impressions,
            ctr: r.ctr,
            position: r.position,
        });
        let queries = query(&["query"], TOP)?
            .rows
            .into_iter()
            .filter_map(|r| {
                Some(QueryRow {
                    query: r.keys.into_iter().next()?,
                    clicks: r.clicks,
                    impressions: r.impressions,
                    ctr: r.ctr,
                    position: r.position,
                })
            })
            .collect();
        Ok(Search { totals, queries })
    }

    fn call(
        &self,
        res: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
    ) -> Result<serde_json::Value, String> {
        let mut res = res.map_err(|e| format!("Google: {e}"))?;
        let status = res.status().as_u16();
        let body: serde_json::Value = res
            .body_mut()
            .read_json()
            .unwrap_or(serde_json::Value::Null);
        if status != 200 {
            let why = body["error"]["message"].as_str().unwrap_or("unknown error");
            return Err(format!("Google ({status}): {why}"));
        }
        Ok(body)
    }
}

/// A fake of the token and Search Console endpoints, for tests here and in
/// api.rs. It checks the JWT's signature against the test key.
#[cfg(test)]
pub mod fake {
    use axum::extract::{Path, Request};
    use axum::http::StatusCode;
    use axum::routing::{get, post};
    use axum::{Json, Router};
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use ring::signature::{RSA_PKCS1_2048_8192_SHA256, UnparsedPublicKey};

    pub const KEY_JSON: &str = include_str!("testdata/service-account.json");
    const PUB: &[u8] = include_bytes!("testdata/test-key.pub.der");
    pub const PROPERTY: &str = "sc-domain:blog.example";

    fn valid(assertion: &str) -> bool {
        let parts: Vec<&str> = assertion.split('.').collect();
        let [header, claims, sig] = parts[..] else {
            return false;
        };
        let Ok(sig) = URL_SAFE_NO_PAD.decode(sig) else {
            return false;
        };
        UnparsedPublicKey::new(&RSA_PKCS1_2048_8192_SHA256, PUB)
            .verify(format!("{header}.{claims}").as_bytes(), &sig)
            .is_ok()
    }

    fn authorized(req: &Request) -> bool {
        req.headers()
            .get("authorization")
            .is_some_and(|v| v == "Bearer fake-token")
    }

    /// Serves on a random local port; returns the base URL.
    pub async fn start() -> String {
        let app = Router::new()
            .route(
                "/token",
                post(|body: String| async move {
                    let ok = url::form_urlencoded::parse(body.as_bytes())
                        .any(|(k, v)| k == "assertion" && valid(&v));
                    if ok {
                        (StatusCode::OK, Json(serde_json::json!({"access_token": "fake-token", "expires_in": 3600})))
                    } else {
                        (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "invalid_grant", "error_description": "Invalid JWT Signature."})))
                    }
                }),
            )
            .route(
                "/webmasters/v3/sites",
                get(|req: Request| async move {
                    if !authorized(&req) {
                        return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": {"message": "no"}})));
                    }
                    (StatusCode::OK, Json(serde_json::json!({"siteEntry": [
                        {"siteUrl": PROPERTY, "permissionLevel": "siteRestrictedUser"},
                        {"siteUrl": "https://shop.example/", "permissionLevel": "siteFullUser"},
                        {"siteUrl": "https://unverified.example/", "permissionLevel": "siteUnverifiedUser"},
                    ]})))
                }),
            )
            .route(
                "/webmasters/v3/sites/{site}/searchAnalytics/query",
                post(|Path(site): Path<String>, req: Request| async move {
                    if !authorized(&req) {
                        return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": {"message": "no"}})));
                    }
                    if site != PROPERTY {
                        return (StatusCode::FORBIDDEN, Json(serde_json::json!({"error": {"message": format!("User does not have sufficient permission for site '{site}'.")}})));
                    }
                    let body = axum::body::to_bytes(req.into_body(), usize::MAX).await.unwrap();
                    let q: serde_json::Value = serde_json::from_slice(&body).unwrap();
                    let rows = if q["dimensions"].as_array().is_some_and(|d| d.is_empty()) {
                        serde_json::json!([{"clicks": 42.0, "impressions": 1200.0, "ctr": 0.035, "position": 8.4}])
                    } else {
                        serde_json::json!([
                            {"keys": ["realm self hosted"], "clicks": 30.0, "impressions": 400.0, "ctr": 0.075, "position": 3.2},
                            {"keys": ["cookieless analytics"], "clicks": 12.0, "impressions": 800.0, "ctr": 0.015, "position": 11.0},
                        ])
                    };
                    (StatusCode::OK, Json(serde_json::json!({"rows": rows})))
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{addr}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn client() -> Client {
        let base = fake::start().await;
        Client::new(&format!("{base}/token"), &base)
    }

    #[tokio::test]
    async fn a_service_account_lists_properties_and_queries() {
        let c = client().await;
        let sa = ServiceAccount::parse(fake::KEY_JSON).unwrap();
        let result = tokio::task::spawn_blocking(move || {
            let (token, exp) = c.token(&sa, 1000).unwrap();
            assert_eq!(token, "fake-token");
            assert_eq!(exp, 1000 + 3600 - 60);
            let props = c.properties(&token).unwrap();
            let search = c
                .search(&token, fake::PROPERTY, "2026-09-01", "2026-09-30")
                .unwrap();
            let denied = c.search(&token, "https://shop.example/", "2026-09-01", "2026-09-30");
            (props, search, denied)
        })
        .await
        .unwrap();
        // Unverified entries are left out.
        assert_eq!(
            result.0,
            vec![
                "https://shop.example/".to_string(),
                fake::PROPERTY.to_string()
            ]
        );
        assert_eq!(result.1.totals.as_ref().unwrap().clicks, 42.0);
        assert_eq!(result.1.queries[0].query, "realm self hosted");
        assert!(result.2.unwrap_err().contains("sufficient permission"));
    }

    #[tokio::test]
    async fn a_key_google_rejects_is_a_readable_error() {
        let c = client().await;
        let mut sa = ServiceAccount::parse(fake::KEY_JSON).unwrap();
        // A real key, but not the one the fake trusts: the signature fails.
        sa.private_key = include_str!("testdata/other-key.pem").to_string();
        let err = tokio::task::spawn_blocking(move || c.token(&sa, 0))
            .await
            .unwrap()
            .unwrap_err();
        assert_eq!(err, "Google refused the key: Invalid JWT Signature.");
    }

    #[test]
    fn only_service_account_files_parse() {
        assert!(ServiceAccount::parse("{}").is_err());
        assert!(ServiceAccount::parse("nope").is_err());
        let sa = ServiceAccount::parse(fake::KEY_JSON).unwrap();
        assert_eq!(
            sa.client_email,
            "analytics@analytics-test.iam.gserviceaccount.com"
        );
    }
}
