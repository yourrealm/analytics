//! analytics server: tracker ingest and the owner's JSON API over one SQLite
//! file.
//!
//! `analytics` serves; `analytics healthcheck` probes a running server and
//! exits 0/1 (the container has no shell or curl, so the binary checks itself).

mod api;
mod crypto;
mod db;
mod geo;
mod google;
mod healthcheck;
mod hosts;
mod ingest;
mod stats;
mod visitor;

use std::net::SocketAddr;
use std::path::PathBuf;

fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        None => serve(),
        Some("healthcheck") => healthcheck::run(port()),
        Some(other) => {
            eprintln!("unknown command: {other}");
            std::process::exit(2);
        }
    }
}

fn port() -> u16 {
    std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000)
}

fn env_path(name: &str, default: &str) -> PathBuf {
    std::env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(default))
}

#[tokio::main]
async fn serve() {
    let dir = env_path("ANALYTICS_DATA_DIR", "/data");
    std::fs::create_dir_all(&dir)
        .unwrap_or_else(|e| panic!("create data dir {}: {e}", dir.display()));
    let db_path = dir.join("analytics.db");
    let conn =
        db::open(&db_path).unwrap_or_else(|e| panic!("open database {}: {e}", db_path.display()));
    let geo = geo::Geo::open(&env_path("ANALYTICS_GEOIP", "/app/geo.mmdb"));

    let web = env_path("ANALYTICS_WEB_DIR", "/app/web");
    let web = web.join("index.html").exists().then_some(web);
    if web.is_none() {
        eprintln!("no frontend build found, serving the API only");
    }
    let sealer = crypto::Sealer::from_env_or_file(&dir)
        .unwrap_or_else(|e| panic!("secret in {}: {e}", dir.display()));
    // Set by realm.tsx only when the operator granted internet access.
    let google =
        (std::env::var("ANALYTICS_GOOGLE").as_deref() == Ok("1")).then(google::Client::from_env);
    if google.is_none() {
        eprintln!("no internet access granted, Search Console off");
    }
    let app = api::router(
        api::AppState::new(conn, geo, sealer, google),
        web.as_deref(),
    );
    let addr = SocketAddr::from(([0, 0, 0, 0], port()));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap_or_else(|e| panic!("bind {addr}: {e}"));
    eprintln!("analytics listening on {addr}, data in {}", dir.display());
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown())
    .await
    .expect("server error");
}

/// Resolves on SIGINT or SIGTERM so `docker stop` ends the process promptly.
async fn shutdown() {
    let ctrl_c = tokio::signal::ctrl_c();
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("install SIGTERM handler");
    tokio::select! {
        _ = ctrl_c => {}
        _ = term.recv() => {}
    }
}
