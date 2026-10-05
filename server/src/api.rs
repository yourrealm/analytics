//! HTTP routes.
//!
//! Three audiences:
//! - Tracked pages, anonymous and cross-origin: `GET /script.js` and
//!   `POST /api/event`. The gate in realm.tsx admits exactly these on shape;
//!   the checks that matter (site exists, host allowed) happen here.
//! - The owner, through Home: everything else under `/api/`. The gate injects
//!   `X-Analytics-User` for a Home session and strips any client copy, so its
//!   presence is the identity. Each user sees only their own sites.
//! - The owner's browser: the React app from `web/`, served from disk with a
//!   fallback to `index.html` for client-side routes. The gate puts it behind
//!   the same Home session, so the app's API calls carry the header too.

use crate::crypto::Sealer;
use crate::db::{self, User};
use crate::geo::Geo;
use crate::google;
use crate::stats::{self, Period};
use crate::{hosts, ingest, visitor};
use axum::body::Bytes;
use axum::extract::rejection::JsonRejection;
use axum::extract::{ConnectInfo, FromRequest, FromRequestParts, Path, Query, Request, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post, put};
use axum::{Extension, Json, Router};
use jiff::tz::TimeZone;
use rusqlite::Connection;
use serde::Deserialize;
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::Path as FsPath;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tower_http::services::{ServeDir, ServeFile};
use tower_http::set_header::SetResponseHeaderLayer;

/// Must match `getTrustedHeaders` in realm.tsx.
const USER_HEADER: &str = "x-analytics-user";
const MAX_USERNAME: usize = 64;
const MAX_SITE_NAME: usize = 100;
const MAX_HOSTNAMES: usize = 20;
const MAX_SITES: i64 = 50;
const MAX_EVENT_NAME: usize = 255;
const SITE_ID_LEN: usize = 12;

/// Liwan's tracker, built (Apache-2.0, see NOTICE).
const TRACKER: &str = include_str!("../assets/tracker.js");

type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;
/// (property, from, to) to (fetched at, result).
type SearchCache = HashMap<(String, String, String), (i64, google::Search)>;

#[derive(Clone)]
pub struct AppState {
    db: Arc<Mutex<Connection>>,
    geo: Arc<Geo>,
    now: Clock,
    sealer: Arc<Sealer>,
    /// `None` when the operator did not grant internet access.
    google: Option<google::Client>,
    /// Access tokens by user, until they expire.
    tokens: Arc<Mutex<HashMap<i64, (String, i64)>>>,
    /// Search results by (property, from, to), for an hour: the dashboard
    /// refreshes every minute and Google's data moves daily.
    searches: Arc<Mutex<SearchCache>>,
}

fn system_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl AppState {
    pub fn new(conn: Connection, geo: Geo, sealer: Sealer, google: Option<google::Client>) -> Self {
        let mut state = Self::with_clock(conn, geo, Arc::new(system_now));
        state.sealer = Arc::new(sealer);
        state.google = google;
        state
    }

    fn with_clock(conn: Connection, geo: Geo, now: Clock) -> Self {
        Self {
            db: Arc::new(Mutex::new(conn)),
            geo: Arc::new(geo),
            now,
            sealer: Arc::new(Sealer::new("test")),
            google: None,
            tokens: Arc::default(),
            searches: Arc::default(),
        }
    }

    fn now(&self) -> i64 {
        (self.now)()
    }

    /// Runs a query off the async runtime. One connection, so calls serialize.
    async fn db<T, F>(&self, f: F) -> Result<T, ApiError>
    where
        F: FnOnce(&Connection) -> rusqlite::Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let db = Arc::clone(&self.db);
        tokio::task::spawn_blocking(move || {
            let conn = db.lock().unwrap_or_else(|e| e.into_inner());
            f(&conn)
        })
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .map_err(ApiError::from)
    }
}

/// `web` is the built frontend (`web/dist`); `None` serves no UI.
pub fn router(state: AppState, web: Option<&FsPath>) -> Router {
    let api = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/script.js", get(script))
        .route("/api/event", post(event).options(preflight))
        .route("/api/me", get(me))
        .route("/api/settings", put(put_settings))
        .route("/api/sites", get(list_sites).post(create_site))
        .route("/api/sites/{id}", put(update_site).delete(delete_site))
        .route("/api/sites/{id}/stats", get(site_stats))
        .route("/api/sites/{id}/props", get(site_props))
        .route("/api/overview", get(overview))
        .route(
            "/api/google",
            get(google_status)
                .put(connect_google)
                .delete(disconnect_google),
        )
        .route("/api/google/properties", get(google_properties))
        .route("/api/sites/{id}/search-console", put(set_search_console))
        .route("/api/sites/{id}/search", get(site_search))
        .route("/api/{*rest}", any(|| async { ApiError::NotFound }))
        .with_state(state);
    match web {
        Some(dir) => api
            // Vite fingerprints everything under assets/, so it never changes.
            .nest_service(
                "/assets",
                tower::ServiceBuilder::new()
                    .layer(SetResponseHeaderLayer::overriding(
                        header::CACHE_CONTROL,
                        HeaderValue::from_static("public, max-age=31536000, immutable"),
                    ))
                    .service(ServeDir::new(dir.join("assets"))),
            )
            .fallback_service(
                tower::ServiceBuilder::new()
                    .layer(SetResponseHeaderLayer::if_not_present(
                        header::CACHE_CONTROL,
                        HeaderValue::from_static("no-cache"),
                    ))
                    .service(ServeDir::new(dir).fallback(ServeFile::new(dir.join("index.html")))),
            ),
        None => api.fallback(|| async { ApiError::NotFound }),
    }
}

enum ApiError {
    Unauthorized,
    NotFound,
    BadRequest(String),
    /// Google answered with an error, or could not be reached.
    Upstream(String),
    Internal(String),
}

fn bad(msg: impl Into<String>) -> ApiError {
    ApiError::BadRequest(msg.into())
}

impl From<rusqlite::Error> for ApiError {
    fn from(e: rusqlite::Error) -> Self {
        ApiError::Internal(e.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            ApiError::Unauthorized => (StatusCode::UNAUTHORIZED, "sign in required".to_string()),
            ApiError::NotFound => (StatusCode::NOT_FOUND, "not found".to_string()),
            ApiError::BadRequest(m) => (StatusCode::BAD_REQUEST, m),
            ApiError::Upstream(m) => (StatusCode::BAD_GATEWAY, m),
            ApiError::Internal(m) => {
                eprintln!("internal error: {m}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal error".to_string(),
                )
            }
        };
        (status, Json(serde_json::json!({ "error": message }))).into_response()
    }
}

/// `Json`, but a malformed body is a 400 with the same `{ "error" }` shape
/// as every other failure, instead of axum's plain-text 422.
struct ApiJson<T>(T);

impl<T, S> FromRequest<S> for ApiJson<T>
where
    Json<T>: FromRequest<S, Rejection = JsonRejection>,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, ApiError> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(value)) => Ok(ApiJson(value)),
            Err(rejection) => Err(ApiError::BadRequest(rejection.body_text())),
        }
    }
}

/// The calling user, created on first sight.
struct Identity(User);

impl FromRequestParts<AppState> for Identity {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let username = parts
            .headers
            .get(USER_HEADER)
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .filter(|s| !s.is_empty() && s.len() <= MAX_USERNAME)
            .map(str::to_string)
            .ok_or(ApiError::Unauthorized)?;
        let now = state.now();
        let user = state
            .db(move |c| db::user_by_name(c, &username, now))
            .await?;
        Ok(Identity(user))
    }
}

// ---- tracked pages ------------------------------------------------------------

fn cors(headers: &mut HeaderMap) {
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
}

async fn script() -> Response {
    let mut res = (
        [
            (header::CONTENT_TYPE, "text/javascript; charset=utf-8"),
            (
                header::CACHE_CONTROL,
                "public, max-age=3600, stale-while-revalidate=86400",
            ),
        ],
        TRACKER,
    )
        .into_response();
    cors(res.headers_mut());
    res
}

/// The tracker sends `text/plain`, which needs no preflight. A custom
/// `event()` call with other headers might, so answer it anyway.
async fn preflight() -> Response {
    let mut res = (
        StatusCode::NO_CONTENT,
        [
            (header::ACCESS_CONTROL_ALLOW_METHODS, "POST"),
            (header::ACCESS_CONTROL_ALLOW_HEADERS, "content-type"),
            (header::ACCESS_CONTROL_MAX_AGE, "86400"),
        ],
    )
        .into_response();
    cors(res.headers_mut());
    res
}

/// The visitor's address. The ingress overwrites `X-Real-IP` with the one
/// address it resolved, and only the ingress can reach this port from
/// outside, so the header is trusted. The peer address is a fallback for
/// running without the ingress (`pnpm dev:server`).
fn client_ip(headers: &HeaderMap, peer: Option<IpAddr>) -> Option<IpAddr> {
    let header = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(',').next())
            .and_then(|v| v.trim().parse().ok())
    };
    header("x-real-ip")
        .or_else(|| header("x-forwarded-for"))
        .or(peer)
}

async fn event(
    State(state): State<AppState>,
    peer: Option<Extension<ConnectInfo<SocketAddr>>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let mut res = match ingest_event(
        &state,
        peer.map(|Extension(ConnectInfo(addr))| addr.ip()),
        &headers,
        &body,
    )
    .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => e.into_response(),
    };
    cors(res.headers_mut());
    res
}

async fn ingest_event(
    state: &AppState,
    peer: Option<IpAddr>,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<(), ApiError> {
    let req = ingest::parse(body).map_err(bad)?;
    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    if ingest::precheck(&req, user_agent.as_deref()).is_err() {
        return Ok(());
    }
    let ip = client_ip(headers, peer);
    let country = ip.and_then(|ip| state.geo.country(ip));
    let now = state.now();
    let site_id = req.entity_id.clone();

    let built = state
        .db(move |c| {
            let Some(hostnames) = db::site_hostnames(c, &site_id)? else {
                return Ok(Ok(None));
            };
            let visitor = match (ip, user_agent.as_deref()) {
                (Some(ip), Some(ua)) => visitor::id(&ip, ua, &salt(c, now)?, &site_id),
                _ => visitor::random_id(),
            };
            let ctx = ingest::Context {
                now,
                visitor,
                country,
            };
            match ingest::build(req, Some(&hostnames), ctx) {
                Ok(Ok(event)) => {
                    db::insert_event(c, &event)?;
                    Ok(Ok(Some(())))
                }
                Ok(Err(_dropped)) => Ok(Ok(None)),
                Err(msg) => Ok(Err(msg)),
            }
        })
        .await?;
    built.map(|_| ()).map_err(bad)
}

/// Today's salt, rotated at UTC midnight. Called inside the db lock, so two
/// requests never rotate at once.
fn salt(c: &Connection, now: i64) -> rusqlite::Result<String> {
    if let Some((salt, updated_at)) = db::get_salt(c)?
        && !visitor::should_rotate(updated_at, now, &TimeZone::UTC)
    {
        return Ok(salt);
    }
    let salt = visitor::new_salt();
    db::put_salt(c, &salt, now)?;
    Ok(salt)
}

// ---- the owner ------------------------------------------------------------------

async fn me(Identity(user): Identity) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "username": user.username,
        "timezone": user.timezone,
    }))
}

#[derive(Deserialize)]
struct SettingsBody {
    timezone: String,
}

async fn put_settings(
    State(state): State<AppState>,
    Identity(user): Identity,
    ApiJson(body): ApiJson<SettingsBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let tz = body.timezone.trim().to_string();
    if TimeZone::get(&tz).is_err() {
        return Err(bad(format!("unknown time zone: {tz}")));
    }
    let saved = tz.clone();
    state
        .db(move |c| db::set_timezone(c, user.id, &saved))
        .await?;
    Ok(Json(serde_json::json!({ "timezone": tz })))
}

async fn list_sites(
    State(state): State<AppState>,
    Identity(user): Identity,
) -> Result<Json<Vec<db::SiteActivity>>, ApiError> {
    Ok(Json(
        state
            .db(move |c| db::list_sites_activity(c, user.id))
            .await?,
    ))
}

#[derive(Deserialize)]
struct SiteBody {
    name: String,
    #[serde(default)]
    hostnames: Vec<String>,
}

fn validate_site(body: SiteBody) -> Result<(String, Vec<String>), ApiError> {
    let name = body.name.trim().to_string();
    if name.is_empty() || name.chars().count() > MAX_SITE_NAME {
        return Err(bad(format!("name must be 1 to {MAX_SITE_NAME} characters")));
    }
    if body.hostnames.len() > MAX_HOSTNAMES {
        return Err(bad(format!("at most {MAX_HOSTNAMES} hostnames")));
    }
    let mut hostnames = Vec::new();
    for h in &body.hostnames {
        if let Some(h) = hosts::normalize(h).map_err(bad)?
            && !hostnames.contains(&h)
        {
            hostnames.push(h);
        }
    }
    Ok((name, hostnames))
}

async fn create_site(
    State(state): State<AppState>,
    Identity(user): Identity,
    ApiJson(body): ApiJson<SiteBody>,
) -> Result<(StatusCode, Json<db::Site>), ApiError> {
    let (name, hostnames) = validate_site(body)?;
    let now = state.now();
    let site = state
        .db(move |c| {
            if db::count_sites(c, user.id)? >= MAX_SITES {
                return Ok(None);
            }
            let id = site_id();
            db::insert_site(c, user.id, &id, &name, &hostnames, now).map(Some)
        })
        .await?
        .ok_or_else(|| bad(format!("at most {MAX_SITES} sites")))?;
    Ok((StatusCode::CREATED, Json(site)))
}

/// Lowercase letters and digits: it goes into HTML attributes and URLs.
fn site_id() -> String {
    const CHARS: &[u8; 36] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    visitor::random_bytes::<SITE_ID_LEN>()
        .iter()
        .map(|b| CHARS[(b % 36) as usize] as char)
        .collect()
}

async fn update_site(
    State(state): State<AppState>,
    Identity(user): Identity,
    Path(id): Path<String>,
    ApiJson(body): ApiJson<SiteBody>,
) -> Result<Json<db::Site>, ApiError> {
    let (name, hostnames) = validate_site(body)?;
    state
        .db(move |c| {
            if !db::update_site(c, user.id, &id, &name, &hostnames)? {
                return Ok(None);
            }
            db::site_for_user(c, user.id, &id)
        })
        .await?
        .map(Json)
        .ok_or(ApiError::NotFound)
}

async fn delete_site(
    State(state): State<AppState>,
    Identity(user): Identity,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let deleted = state.db(move |c| db::delete_site(c, user.id, &id)).await?;
    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound)
    }
}

#[derive(Deserialize)]
struct StatsQuery {
    period: Option<String>,
    event: Option<String>,
}

fn period(q: &StatsQuery) -> Result<Period, ApiError> {
    match q.period.as_deref().filter(|p| !p.is_empty()) {
        None => Ok(Period::Days(30)),
        Some(p) => Period::parse(p).ok_or_else(|| bad(format!("unknown period: {p}"))),
    }
}

async fn site_stats(
    State(state): State<AppState>,
    Identity(user): Identity,
    Path(id): Path<String>,
    Query(q): Query<StatsQuery>,
) -> Result<Json<stats::Stats>, ApiError> {
    let period = period(&q)?;
    let now = state.now();
    state
        .db(move |c| {
            if db::site_for_user(c, user.id, &id)?.is_none() {
                return Ok(None);
            }
            let tz = TimeZone::get(&user.timezone).unwrap_or(TimeZone::UTC);
            stats::stats(c, &id, &stats::range(period, now, &tz)).map(Some)
        })
        .await?
        .map(Json)
        .ok_or(ApiError::NotFound)
}

async fn site_props(
    State(state): State<AppState>,
    Identity(user): Identity,
    Path(id): Path<String>,
    Query(q): Query<StatsQuery>,
) -> Result<Json<Vec<stats::PropRow>>, ApiError> {
    let period = period(&q)?;
    let event = q
        .event
        .clone()
        .filter(|e| !e.is_empty() && e.len() <= MAX_EVENT_NAME)
        .ok_or_else(|| bad("event is required"))?;
    let now = state.now();
    state
        .db(move |c| {
            if db::site_for_user(c, user.id, &id)?.is_none() {
                return Ok(None);
            }
            let tz = TimeZone::get(&user.timezone).unwrap_or(TimeZone::UTC);
            stats::props(c, &id, &stats::range(period, now, &tz), &event).map(Some)
        })
        .await?
        .map(Json)
        .ok_or(ApiError::NotFound)
}

#[derive(serde::Serialize)]
struct SiteGlance {
    id: String,
    name: String,
    /// The first allowed hostname, to tell sites apart.
    host: Option<String>,
    #[serde(flatten)]
    glance: stats::Glance,
}

/// Every site of the user over the last 24 hours, for the overview.
async fn overview(
    State(state): State<AppState>,
    Identity(user): Identity,
) -> Result<Json<Vec<SiteGlance>>, ApiError> {
    let now = state.now();
    let rows = state
        .db(move |c| {
            db::list_sites(c, user.id)?
                .into_iter()
                .map(|s| {
                    Ok(SiteGlance {
                        glance: stats::glance(c, &s.id, now)?,
                        host: s.hostnames.into_iter().next(),
                        id: s.id,
                        name: s.name,
                    })
                })
                .collect::<rusqlite::Result<Vec<_>>>()
        })
        .await?;
    Ok(Json(rows))
}

// ---- Google Search Console ------------------------------------------------------

const NO_EGRESS: &str =
    "this app has no internet access: an admin can grant it in Realm to use Search Console";

impl AppState {
    fn google(&self) -> Result<google::Client, ApiError> {
        self.google.clone().ok_or_else(|| bad(NO_EGRESS))
    }

    /// Runs blocking Google calls off the runtime.
    async fn blocking<T: Send + 'static>(
        &self,
        f: impl FnOnce() -> Result<T, String> + Send + 'static,
    ) -> Result<T, ApiError> {
        tokio::task::spawn_blocking(f)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?
            .map_err(ApiError::Upstream)
    }

    /// A Google client and a live access token for this user's stored key.
    async fn google_token(&self, user: &User) -> Result<(google::Client, String), ApiError> {
        let client = self.google()?;
        let now = self.now();
        if let Some((token, exp)) = self
            .tokens
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&user.id)
            && *exp > now
        {
            return Ok((client, token.clone()));
        }
        let sealed = user
            .google_key
            .as_deref()
            .ok_or_else(|| bad("connect Google Search Console first"))?;
        let json = self
            .sealer
            .open(sealed)
            .ok_or_else(|| bad("the stored Google key can't be read; paste it again"))?;
        let sa = google::ServiceAccount::parse(&json).map_err(bad)?;
        let c = client.clone();
        let (token, exp) = self.blocking(move || c.token(&sa, now)).await?;
        self.tokens
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(user.id, (token.clone(), exp));
        Ok((client, token))
    }
}

async fn google_status(
    State(state): State<AppState>,
    Identity(user): Identity,
) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "available": state.google.is_some(),
        "email": user.google_email,
    }))
}

#[derive(Deserialize)]
struct GoogleKeyBody {
    key: String,
}

/// Checks the key with Google (a token and the property list) before keeping it.
async fn connect_google(
    State(state): State<AppState>,
    Identity(user): Identity,
    ApiJson(body): ApiJson<GoogleKeyBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let client = state.google()?;
    let sa = google::ServiceAccount::parse(body.key.trim()).map_err(bad)?;
    let now = state.now();
    let email = sa.client_email.clone();
    let (token, exp, properties) = state
        .blocking(move || {
            let (token, exp) = client.token(&sa, now)?;
            let properties = client.properties(&token)?;
            Ok((token, exp, properties))
        })
        .await?;
    let sealed = state.sealer.seal(body.key.trim());
    let saved = email.clone();
    state
        .db(move |c| db::set_google_key(c, user.id, Some((&sealed, &saved))))
        .await?;
    state
        .tokens
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(user.id, (token, exp));
    Ok(Json(
        serde_json::json!({ "email": email, "properties": properties }),
    ))
}

async fn disconnect_google(
    State(state): State<AppState>,
    Identity(user): Identity,
) -> Result<StatusCode, ApiError> {
    state
        .db(move |c| db::set_google_key(c, user.id, None))
        .await?;
    state
        .tokens
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&user.id);
    Ok(StatusCode::NO_CONTENT)
}

async fn google_properties(
    State(state): State<AppState>,
    Identity(user): Identity,
) -> Result<Json<Vec<String>>, ApiError> {
    let (client, token) = state.google_token(&user).await?;
    Ok(Json(
        state.blocking(move || client.properties(&token)).await?,
    ))
}

#[derive(Deserialize)]
struct SearchConsoleBody {
    property: Option<String>,
}

/// Links a site to a property the user's service account can read, or
/// unlinks it with `null`.
async fn set_search_console(
    State(state): State<AppState>,
    Identity(user): Identity,
    Path(id): Path<String>,
    ApiJson(body): ApiJson<SearchConsoleBody>,
) -> Result<Json<db::Site>, ApiError> {
    let property = body
        .property
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty());
    if let Some(p) = &property {
        let (client, token) = state.google_token(&user).await?;
        let known = state.blocking(move || client.properties(&token)).await?;
        if !known.contains(p) {
            return Err(bad(format!(
                "{} can't read {p}: add it as a user of that property in Search Console",
                user.google_email
                    .as_deref()
                    .unwrap_or("the service account")
            )));
        }
    }
    state
        .db(move |c| {
            if !db::set_search_console(c, user.id, &id, property.as_deref())? {
                return Ok(None);
            }
            db::site_for_user(c, user.id, &id)
        })
        .await?
        .map(Json)
        .ok_or(ApiError::NotFound)
}

#[derive(serde::Serialize)]
struct SearchView {
    property: String,
    from: String,
    to: String,
    #[serde(flatten)]
    search: google::Search,
}

async fn site_search(
    State(state): State<AppState>,
    Identity(user): Identity,
    Path(id): Path<String>,
    Query(q): Query<StatsQuery>,
) -> Result<Json<SearchView>, ApiError> {
    let period = period(&q)?;
    let user_id = user.id;
    let site = state
        .db(move |c| db::site_for_user(c, user_id, &id))
        .await?
        .ok_or(ApiError::NotFound)?;
    let property = site
        .search_console
        .ok_or_else(|| bad("this site is not linked to Search Console"))?;
    let tz = TimeZone::get(&user.timezone).unwrap_or(TimeZone::UTC);
    let now = state.now();
    let range = stats::range(period, now, &tz);
    let key = (property.clone(), range.from.clone(), range.to.clone());

    let cached = state
        .searches
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&key)
        .filter(|(at, _)| now - at < 3600)
        .map(|(_, s)| s.clone());
    let search = match cached {
        Some(s) => s,
        None => {
            let (client, token) = state.google_token(&user).await?;
            let (p, from, to) = key.clone();
            let s = state
                .blocking(move || client.search(&token, &p, &from, &to))
                .await?;
            state
                .searches
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(key, (now, s.clone()));
            s
        }
    };
    Ok(Json(SearchView {
        property,
        from: range.from,
        to: range.to,
        search,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{Body, to_bytes};
    use axum::http::{Method, Request};
    use serde_json::{Value, json};
    use tower::ServiceExt;

    // 2026-08-30T12:00:00Z
    const NOON: i64 = 1_788_091_200;
    const ANN: (&str, &str) = ("X-Analytics-User", "ann");
    const BOB: (&str, &str) = ("X-Analytics-User", "bob");
    const CHROME: &str =
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) Chrome/140.0.0.0 Safari/537.36";

    fn app() -> Router {
        router(
            AppState::with_clock(db::open_memory(), Geo::none(), Arc::new(|| NOON)),
            None,
        )
    }

    fn request(
        method: Method,
        path: &str,
        headers: &[(&str, &str)],
        body: Option<Value>,
    ) -> Request<Body> {
        let mut req = Request::builder().method(method).uri(path);
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        req.body(body.map(|b| Body::from(b.to_string())).unwrap_or_default())
            .unwrap()
    }

    async fn call(app: &Router, req: Request<Body>) -> (StatusCode, HeaderMap, Value) {
        let res = app.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let headers = res.headers().clone();
        let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
        let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, headers, json)
    }

    async fn create_site(app: &Router, who: (&str, &str), body: Value) -> Value {
        let json_ct = (header::CONTENT_TYPE.as_str(), "application/json");
        let (status, _, site) = call(
            app,
            request(Method::POST, "/api/sites", &[who, json_ct], Some(body)),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{site}");
        site
    }

    async fn track(app: &Router, body: Value, ip: &str) -> (StatusCode, HeaderMap) {
        let req = request(
            Method::POST,
            "/api/event",
            &[
                ("content-type", "text/plain;charset=UTF-8"),
                ("user-agent", CHROME),
                ("x-real-ip", ip),
            ],
            Some(body),
        );
        let (status, headers, _) = call(app, req).await;
        (status, headers)
    }

    #[tokio::test]
    async fn owner_routes_need_the_header() {
        let app = app();
        for path in ["/api/me", "/api/sites", "/api/overview"] {
            let (status, _, _) = call(&app, request(Method::GET, path, &[], None)).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{path}");
        }
    }

    #[tokio::test]
    async fn sites_are_private_to_their_owner() {
        let app = app();
        let site = create_site(
            &app,
            ANN,
            json!({"name": "Blog", "hostnames": ["Example.com", " ", "*.example.org"]}),
        )
        .await;
        assert_eq!(site["hostnames"], json!(["example.com", "*.example.org"]));
        let id = site["id"].as_str().unwrap();
        assert_eq!(id.len(), SITE_ID_LEN);

        let (_, _, mine) = call(&app, request(Method::GET, "/api/sites", &[ANN], None)).await;
        assert_eq!(mine.as_array().unwrap().len(), 1);
        // No visit yet: the Sites list shows it waiting.
        assert_eq!(mine[0]["last_event"], Value::Null);
        let (_, _, theirs) = call(&app, request(Method::GET, "/api/sites", &[BOB], None)).await;
        assert_eq!(theirs, json!([]));

        let stats = format!("/api/sites/{id}/stats");
        let (status, _, _) = call(&app, request(Method::GET, &stats, &[BOB], None)).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let site_path = format!("/api/sites/{id}");
        let (status, _, _) = call(&app, request(Method::DELETE, &site_path, &[BOB], None)).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _, _) = call(&app, request(Method::DELETE, &site_path, &[ANN], None)).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn bad_site_input_is_a_400() {
        let app = app();
        let json_ct = ("content-type", "application/json");
        for body in [
            json!({"name": ""}),
            json!({"name": "x", "hostnames": ["https://a.com"]}),
            json!({}),
        ] {
            let (status, _, err) = call(
                &app,
                request(Method::POST, "/api/sites", &[ANN, json_ct], Some(body)),
            )
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert!(err["error"].is_string());
        }
    }

    #[tokio::test]
    async fn tracked_events_show_up_in_stats() {
        let app = app();
        let site = create_site(
            &app,
            ANN,
            json!({"name": "Blog", "hostnames": ["example.com"]}),
        )
        .await;
        let id = site["id"].as_str().unwrap();

        let pv = |path: &str| json!({"entity_id": id, "name": "pageview", "url": format!("https://example.com{path}"), "referrer": "https://news.ycombinator.com/", "screen_width": "lg"});
        let (status, headers) = track(&app, pv("/"), "203.0.113.1").await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert_eq!(headers[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");
        track(&app, pv("/post"), "203.0.113.1").await;
        track(&app, pv("/"), "203.0.113.2").await;
        let signup = json!({"entity_id": id, "name": "signup", "url": "https://example.com/", "properties": {"plan": "pro"}});
        track(&app, signup, "203.0.113.2").await;
        // Dropped silently: unknown site, other host.
        let (status, _) = track(
            &app,
            json!({"entity_id": "nope", "name": "pageview", "url": "https://example.com/"}),
            "203.0.113.3",
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        track(
            &app,
            json!({"entity_id": id, "name": "pageview", "url": "https://evil.com/"}),
            "203.0.113.3",
        )
        .await;

        let (status, _, s) = call(
            &app,
            request(
                Method::GET,
                &format!("/api/sites/{id}/stats?period=today"),
                &[ANN],
                None,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{s}");
        assert_eq!(
            s["totals"],
            json!({"visitors": 2, "pageviews": 3, "events": 1})
        );
        assert_eq!(s["bucket"], "hour");
        assert_eq!(s["series"].as_array().unwrap().len(), 24);
        assert_eq!(s["referrers"][0]["key"], "news.ycombinator.com");
        assert_eq!(s["devices"][0]["key"], "laptop");
        assert_eq!(s["events"][0]["name"], "signup");

        let (_, _, list) = call(&app, request(Method::GET, "/api/sites", &[ANN], None)).await;
        assert_eq!(list[0]["last_event"], NOON);

        let (_, _, p) = call(
            &app,
            request(
                Method::GET,
                &format!("/api/sites/{id}/props?period=today&event=signup"),
                &[ANN],
                None,
            ),
        )
        .await;
        assert_eq!(p[0]["value"], "pro");

        let (_, _, all) = call(&app, request(Method::GET, "/api/overview", &[ANN], None)).await;
        assert_eq!(all[0]["id"], id);
        assert_eq!(all[0]["visitors"], 2);
        assert_eq!(all[0]["previous"], 0);
        assert_eq!(all[0]["series"].as_array().unwrap().len(), 24);
    }

    #[tokio::test]
    async fn ingest_rejects_garbage_and_answers_preflight() {
        let app = app();
        let (status, headers) = track(&app, json!({"nope": true}), "203.0.113.1").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(headers[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");

        let (status, headers, _) =
            call(&app, request(Method::OPTIONS, "/api/event", &[], None)).await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert_eq!(headers[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");

        let (status, headers, _) = call(&app, request(Method::GET, "/script.js", &[], None)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");
    }

    #[tokio::test]
    async fn the_frontend_is_served_with_a_client_side_route_fallback() {
        let dir = std::env::temp_dir().join(format!("analytics-web-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::write(dir.join("index.html"), "<!doctype html>app").unwrap();
        std::fs::write(dir.join("assets/app-abc.js"), "js").unwrap();
        let app = router(
            AppState::with_clock(db::open_memory(), Geo::none(), Arc::new(|| NOON)),
            Some(&dir),
        );
        for path in ["/", "/sites", "/sites/abc"] {
            let res = app
                .clone()
                .oneshot(request(Method::GET, path, &[], None))
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::OK, "{path}");
            assert_eq!(res.headers()[header::CACHE_CONTROL], "no-cache");
        }
        let res = app
            .clone()
            .oneshot(request(Method::GET, "/assets/app-abc.js", &[], None))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert!(
            res.headers()[header::CACHE_CONTROL]
                .to_str()
                .unwrap()
                .contains("immutable")
        );
        // Unknown API paths stay JSON 404s, never the HTML shell.
        let (status, _, body) = call(&app, request(Method::GET, "/api/nope", &[ANN], None)).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["error"], "not found");
        std::fs::remove_dir_all(dir).ok();
    }

    async fn app_with_google() -> Router {
        let base = google::fake::start().await;
        let mut state = AppState::with_clock(db::open_memory(), Geo::none(), Arc::new(|| NOON));
        state.google = Some(google::Client::new(&format!("{base}/token"), &base));
        router(state, None)
    }

    fn put_json(path: &str, who: (&str, &str), body: Value) -> Request<Body> {
        request(
            Method::PUT,
            path,
            &[who, ("content-type", "application/json")],
            Some(body),
        )
    }

    #[tokio::test]
    async fn search_console_connects_links_and_reports() {
        let app = app_with_google().await;
        let (_, _, status) = call(&app, request(Method::GET, "/api/google", &[ANN], None)).await;
        assert_eq!(status, json!({"available": true, "email": null}));

        let (code, _, err) = call(&app, put_json("/api/google", ANN, json!({"key": "{}"}))).await;
        assert_eq!(code, StatusCode::BAD_REQUEST);
        assert!(err["error"].as_str().unwrap().contains("service account"));

        let (code, _, connected) = call(
            &app,
            put_json("/api/google", ANN, json!({"key": google::fake::KEY_JSON})),
        )
        .await;
        assert_eq!(code, StatusCode::OK, "{connected}");
        assert_eq!(
            connected["email"],
            "analytics@analytics-test.iam.gserviceaccount.com"
        );
        assert_eq!(connected["properties"][1], google::fake::PROPERTY);

        let site = create_site(&app, ANN, json!({"name": "Blog"})).await;
        let id = site["id"].as_str().unwrap();
        let search = format!("/api/sites/{id}/search?period=30d");
        let (code, _, _) = call(&app, request(Method::GET, &search, &[ANN], None)).await;
        assert_eq!(code, StatusCode::BAD_REQUEST, "not linked yet");

        // A property the service account was not added to is refused.
        let link = format!("/api/sites/{id}/search-console");
        let (code, _, err) = call(
            &app,
            put_json(&link, ANN, json!({"property": "sc-domain:other.example"})),
        )
        .await;
        assert_eq!(code, StatusCode::BAD_REQUEST);
        assert!(err["error"].as_str().unwrap().contains("add it as a user"));

        let (code, _, linked) = call(
            &app,
            put_json(&link, ANN, json!({"property": google::fake::PROPERTY})),
        )
        .await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(linked["search_console"], google::fake::PROPERTY);

        let (code, _, s) = call(&app, request(Method::GET, &search, &[ANN], None)).await;
        assert_eq!(code, StatusCode::OK, "{s}");
        assert_eq!(s["property"], google::fake::PROPERTY);
        assert_eq!(s["totals"]["clicks"], 42.0);
        assert_eq!(s["queries"][0]["query"], "realm self hosted");
        assert_eq!(
            (s["from"].as_str(), s["to"].as_str()),
            (Some("2026-08-01"), Some("2026-08-30"))
        );

        // Another user sees nothing of it.
        let (code, _, _) = call(&app, request(Method::GET, &search, &[BOB], None)).await;
        assert_eq!(code, StatusCode::NOT_FOUND);
        let (_, _, bob) = call(&app, request(Method::GET, "/api/google", &[BOB], None)).await;
        assert_eq!(bob["email"], Value::Null);

        // Disconnecting unlinks the site.
        let (code, _, _) = call(&app, request(Method::DELETE, "/api/google", &[ANN], None)).await;
        assert_eq!(code, StatusCode::NO_CONTENT);
        let (_, _, sites) = call(&app, request(Method::GET, "/api/sites", &[ANN], None)).await;
        assert_eq!(sites[0]["search_console"], Value::Null);
    }

    #[tokio::test]
    async fn without_internet_access_search_console_explains_why() {
        let app = app();
        let (_, _, status) = call(&app, request(Method::GET, "/api/google", &[ANN], None)).await;
        assert_eq!(status["available"], false);
        let (code, _, err) = call(
            &app,
            put_json("/api/google", ANN, json!({"key": google::fake::KEY_JSON})),
        )
        .await;
        assert_eq!(code, StatusCode::BAD_REQUEST);
        assert!(err["error"].as_str().unwrap().contains("internet access"));
    }

    #[tokio::test]
    async fn a_key_sealed_under_another_secret_asks_to_paste_again() {
        // Seal with a different key than the app's, as after a lost secret.
        let other = crate::crypto::Sealer::new("elsewhere").seal(google::fake::KEY_JSON);
        let mut state = AppState::with_clock(db::open_memory(), Geo::none(), Arc::new(|| NOON));
        state.google = Some(google::Client::new(
            "http://127.0.0.1:9/token",
            "http://127.0.0.1:9",
        ));
        let conn = state.db.clone();
        let app = router(state, None);
        {
            let c = conn.lock().unwrap();
            let u = db::user_by_name(&c, "ann", 0).unwrap();
            db::set_google_key(&c, u.id, Some((&other, "sa@x"))).unwrap();
        }
        let (code, _, err) = call(
            &app,
            request(Method::GET, "/api/google/properties", &[ANN], None),
        )
        .await;
        assert_eq!(code, StatusCode::BAD_REQUEST);
        assert!(err["error"].as_str().unwrap().contains("paste it again"));
    }

    #[test]
    fn client_ip_prefers_the_ingress_header() {
        let mut h = HeaderMap::new();
        let peer: IpAddr = "172.18.0.2".parse().unwrap();
        assert_eq!(client_ip(&h, Some(peer)), Some(peer));
        h.insert("x-forwarded-for", "198.51.100.9, 10.0.0.1".parse().unwrap());
        assert_eq!(client_ip(&h, Some(peer)), "198.51.100.9".parse().ok());
        h.insert("x-real-ip", "198.51.100.7".parse().unwrap());
        assert_eq!(client_ip(&h, Some(peer)), "198.51.100.7".parse().ok());
    }
}
