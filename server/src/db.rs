//! SQLite storage. One connection, migrations by `PRAGMA user_version`.

use rusqlite::{Connection, OptionalExtension, Row, params};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub timezone: String,
    /// The Search Console service account key, sealed (see crypto.rs).
    pub google_key: Option<String>,
    /// Its `client_email`, shown so the user knows whom to add in Search Console.
    pub google_email: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Site {
    /// Public: it is the `data-entity` on the tracked pages.
    pub id: String,
    pub name: String,
    /// Exact hosts or `*.example.com`. Empty means any host.
    pub hostnames: Vec<String>,
    pub created_at: i64,
    /// The Search Console property, e.g. `sc-domain:example.com`.
    pub search_console: Option<String>,
}

/// One stored event, after filtering. Strings are already trimmed and capped.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Event {
    pub site_id: String,
    pub ts: i64,
    pub name: String,
    pub visitor: String,
    pub host: String,
    pub path: String,
    pub referrer: Option<String>,
    pub utm_source: Option<String>,
    pub utm_medium: Option<String>,
    pub utm_campaign: Option<String>,
    pub country: Option<String>,
    pub device: Option<&'static str>,
    /// JSON object of string values; `None` when empty.
    pub props: Option<String>,
}

const MIGRATIONS: &[&str] = &[
    // v1: users, their sites, the visitor salt and raw events.
    "CREATE TABLE users (
        id INTEGER PRIMARY KEY,
        username TEXT NOT NULL UNIQUE,
        timezone TEXT NOT NULL DEFAULT 'UTC',
        created_at INTEGER NOT NULL
    );
    CREATE TABLE sites (
        id TEXT PRIMARY KEY,
        user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
        name TEXT NOT NULL,
        hostnames TEXT NOT NULL DEFAULT '[]',
        created_at INTEGER NOT NULL
    );
    CREATE INDEX sites_user ON sites(user_id);
    CREATE TABLE salt (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        salt TEXT NOT NULL,
        updated_at INTEGER NOT NULL
    );
    CREATE TABLE events (
        id INTEGER PRIMARY KEY,
        site_id TEXT NOT NULL REFERENCES sites(id) ON DELETE CASCADE,
        ts INTEGER NOT NULL,
        name TEXT NOT NULL,
        visitor TEXT NOT NULL,
        host TEXT NOT NULL,
        path TEXT NOT NULL,
        referrer TEXT,
        utm_source TEXT,
        utm_medium TEXT,
        utm_campaign TEXT,
        country TEXT,
        device TEXT,
        props TEXT
    );
    CREATE INDEX events_site_ts ON events(site_id, ts);",
    // v2: Google Search Console, a key per user and a property per site.
    "ALTER TABLE users ADD COLUMN google_key TEXT;
    ALTER TABLE users ADD COLUMN google_email TEXT;
    ALTER TABLE sites ADD COLUMN search_console TEXT;",
];

pub fn open(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    setup(conn)
}

#[cfg(test)]
pub fn open_memory() -> Connection {
    setup(Connection::open_in_memory().unwrap()).unwrap()
}

fn setup(mut conn: Connection) -> rusqlite::Result<Connection> {
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    migrate(&mut conn)?;
    Ok(conn)
}

fn migrate(conn: &mut Connection) -> rusqlite::Result<()> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(version as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", i as i64 + 1)?;
        tx.commit()?;
    }
    Ok(())
}

fn user_from(row: &Row) -> rusqlite::Result<User> {
    Ok(User {
        id: row.get("id")?,
        username: row.get("username")?,
        timezone: row.get("timezone")?,
        google_key: row.get("google_key")?,
        google_email: row.get("google_email")?,
    })
}

const USER_COLUMNS: &str = "id, username, timezone, google_key, google_email";
const SITE_COLUMNS: &str = "id, name, hostnames, created_at, search_console";

/// The user by name, created on first sight.
pub fn user_by_name(conn: &Connection, username: &str, now: i64) -> rusqlite::Result<User> {
    conn.execute(
        "INSERT INTO users (username, created_at) VALUES (?1, ?2)
         ON CONFLICT(username) DO NOTHING",
        params![username, now],
    )?;
    conn.query_row(
        &format!("SELECT {USER_COLUMNS} FROM users WHERE username = ?1"),
        [username],
        user_from,
    )
}

/// Stores (or with `None`, removes) the user's sealed key. Removing it also
/// unlinks their sites from Search Console.
pub fn set_google_key(
    conn: &Connection,
    user_id: i64,
    key: Option<(&str, &str)>,
) -> rusqlite::Result<()> {
    let (sealed, email) = key.unzip();
    conn.execute(
        "UPDATE users SET google_key = ?1, google_email = ?2 WHERE id = ?3",
        params![sealed, email, user_id],
    )?;
    if sealed.is_none() {
        conn.execute(
            "UPDATE sites SET search_console = NULL WHERE user_id = ?1",
            [user_id],
        )?;
    }
    Ok(())
}

/// False when the site is not this user's.
pub fn set_search_console(
    conn: &Connection,
    user_id: i64,
    site_id: &str,
    property: Option<&str>,
) -> rusqlite::Result<bool> {
    let n = conn.execute(
        "UPDATE sites SET search_console = ?1 WHERE id = ?2 AND user_id = ?3",
        params![property, site_id, user_id],
    )?;
    Ok(n == 1)
}

pub fn set_timezone(conn: &Connection, user_id: i64, timezone: &str) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE users SET timezone = ?1 WHERE id = ?2",
        params![timezone, user_id],
    )?;
    Ok(())
}

fn site_from(row: &Row) -> rusqlite::Result<Site> {
    let hostnames: String = row.get("hostnames")?;
    Ok(Site {
        id: row.get("id")?,
        name: row.get("name")?,
        hostnames: serde_json::from_str(&hostnames).unwrap_or_default(),
        created_at: row.get("created_at")?,
        search_console: row.get("search_console")?,
    })
}

pub fn list_sites(conn: &Connection, user_id: i64) -> rusqlite::Result<Vec<Site>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {SITE_COLUMNS} FROM sites WHERE user_id = ?1 ORDER BY created_at, id"
    ))?;
    stmt.query_map([user_id], site_from)?.collect()
}

/// A site with what it has seen lately, for the Sites list: whether the
/// snippet works yet, and how busy it is.
#[derive(Debug, serde::Serialize)]
pub struct SiteActivity {
    #[serde(flatten)]
    pub site: Site,
    /// When its last event arrived, in seconds. `None` until the first visit.
    pub last_event: Option<i64>,
    /// Distinct visitors since `since`.
    pub visitors: i64,
}

/// The user's sites, with their last event and visitors since `since`. Both
/// subqueries run on the `events_site_ts` index.
pub fn list_sites_activity(
    conn: &Connection,
    user_id: i64,
    since: i64,
) -> rusqlite::Result<Vec<SiteActivity>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {SITE_COLUMNS},
            (SELECT MAX(ts) FROM events e WHERE e.site_id = sites.id) AS last_event,
            (SELECT COUNT(DISTINCT visitor) FROM events e
             WHERE e.site_id = sites.id AND e.ts >= ?2) AS visitors
         FROM sites WHERE user_id = ?1 ORDER BY created_at, id"
    ))?;
    stmt.query_map(params![user_id, since], |r| {
        Ok(SiteActivity {
            site: site_from(r)?,
            last_event: r.get("last_event")?,
            visitors: r.get("visitors")?,
        })
    })?
    .collect()
}

/// A site, only if this user owns it.
pub fn site_for_user(conn: &Connection, user_id: i64, id: &str) -> rusqlite::Result<Option<Site>> {
    conn.query_row(
        &format!("SELECT {SITE_COLUMNS} FROM sites WHERE id = ?1 AND user_id = ?2"),
        params![id, user_id],
        site_from,
    )
    .optional()
}

/// The allowed hostnames of a site, or `None` when it does not exist.
pub fn site_hostnames(conn: &Connection, id: &str) -> rusqlite::Result<Option<Vec<String>>> {
    conn.query_row("SELECT hostnames FROM sites WHERE id = ?1", [id], |r| {
        let s: String = r.get(0)?;
        Ok(serde_json::from_str(&s).unwrap_or_default())
    })
    .optional()
}

pub fn count_sites(conn: &Connection, user_id: i64) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM sites WHERE user_id = ?1",
        [user_id],
        |r| r.get(0),
    )
}

pub fn insert_site(
    conn: &Connection,
    user_id: i64,
    id: &str,
    name: &str,
    hostnames: &[String],
    now: i64,
) -> rusqlite::Result<Site> {
    let json = serde_json::to_string(hostnames).unwrap_or_else(|_| "[]".into());
    conn.execute(
        "INSERT INTO sites (id, user_id, name, hostnames, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, user_id, name, json, now],
    )?;
    Ok(Site {
        id: id.to_string(),
        name: name.to_string(),
        hostnames: hostnames.to_vec(),
        created_at: now,
        search_console: None,
    })
}

/// Returns false when the site is not this user's.
pub fn update_site(
    conn: &Connection,
    user_id: i64,
    id: &str,
    name: &str,
    hostnames: &[String],
) -> rusqlite::Result<bool> {
    let json = serde_json::to_string(hostnames).unwrap_or_else(|_| "[]".into());
    let n = conn.execute(
        "UPDATE sites SET name = ?1, hostnames = ?2 WHERE id = ?3 AND user_id = ?4",
        params![name, json, id, user_id],
    )?;
    Ok(n == 1)
}

/// Deletes the site and, by cascade, its events. False when not this user's.
pub fn delete_site(conn: &Connection, user_id: i64, id: &str) -> rusqlite::Result<bool> {
    let n = conn.execute(
        "DELETE FROM sites WHERE id = ?1 AND user_id = ?2",
        params![id, user_id],
    )?;
    Ok(n == 1)
}

pub fn get_salt(conn: &Connection) -> rusqlite::Result<Option<(String, i64)>> {
    conn.query_row("SELECT salt, updated_at FROM salt WHERE id = 1", [], |r| {
        Ok((r.get(0)?, r.get(1)?))
    })
    .optional()
}

/// Replaces the salt. The old one is gone for good, which is the point.
pub fn put_salt(conn: &Connection, salt: &str, now: i64) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO salt (id, salt, updated_at) VALUES (1, ?1, ?2)
         ON CONFLICT(id) DO UPDATE SET salt = excluded.salt, updated_at = excluded.updated_at",
        params![salt, now],
    )?;
    Ok(())
}

pub fn insert_event(conn: &Connection, e: &Event) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO events (site_id, ts, name, visitor, host, path, referrer,
            utm_source, utm_medium, utm_campaign, country, device, props)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            e.site_id,
            e.ts,
            e.name,
            e.visitor,
            e.host,
            e.path,
            e.referrer,
            e.utm_source,
            e.utm_medium,
            e.utm_campaign,
            e.country,
            e.device,
            e.props,
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_are_idempotent() {
        let mut conn = open_memory();
        migrate(&mut conn).unwrap();
        let v: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v as usize, MIGRATIONS.len());
    }

    #[test]
    fn sites_belong_to_their_user() {
        let conn = open_memory();
        let ann = user_by_name(&conn, "ann", 1).unwrap();
        let bob = user_by_name(&conn, "bob", 1).unwrap();
        assert_eq!(user_by_name(&conn, "ann", 2).unwrap().id, ann.id);

        let hosts = vec!["example.com".to_string()];
        insert_site(&conn, ann.id, "s1", "Blog", &hosts, 1).unwrap();
        assert_eq!(list_sites(&conn, ann.id).unwrap().len(), 1);
        assert!(list_sites(&conn, bob.id).unwrap().is_empty());
        assert!(site_for_user(&conn, bob.id, "s1").unwrap().is_none());
        assert!(!update_site(&conn, bob.id, "s1", "Mine", &[]).unwrap());
        assert!(!delete_site(&conn, bob.id, "s1").unwrap());
        assert_eq!(site_hostnames(&conn, "s1").unwrap(), Some(hosts));
        assert_eq!(site_hostnames(&conn, "nope").unwrap(), None);
    }

    #[test]
    fn removing_the_google_key_unlinks_search_console() {
        let conn = open_memory();
        let ann = user_by_name(&conn, "ann", 1).unwrap();
        let bob = user_by_name(&conn, "bob", 1).unwrap();
        insert_site(&conn, ann.id, "s1", "Blog", &[], 1).unwrap();
        set_google_key(&conn, ann.id, Some(("sealed", "sa@x"))).unwrap();
        assert!(set_search_console(&conn, ann.id, "s1", Some("sc-domain:a.com")).unwrap());
        assert!(!set_search_console(&conn, bob.id, "s1", Some("sc-domain:b.com")).unwrap());
        let ann = user_by_name(&conn, "ann", 2).unwrap();
        assert_eq!(ann.google_email.as_deref(), Some("sa@x"));
        assert_eq!(
            site_for_user(&conn, ann.id, "s1")
                .unwrap()
                .unwrap()
                .search_console
                .as_deref(),
            Some("sc-domain:a.com")
        );
        set_google_key(&conn, ann.id, None).unwrap();
        assert_eq!(
            site_for_user(&conn, ann.id, "s1")
                .unwrap()
                .unwrap()
                .search_console,
            None
        );
        assert_eq!(user_by_name(&conn, "ann", 3).unwrap().google_key, None);
    }

    #[test]
    fn deleting_a_site_deletes_its_events() {
        let conn = open_memory();
        let ann = user_by_name(&conn, "ann", 1).unwrap();
        insert_site(&conn, ann.id, "s1", "Blog", &[], 1).unwrap();
        let e = Event {
            site_id: "s1".into(),
            name: "pageview".into(),
            ..Default::default()
        };
        insert_event(&conn, &e).unwrap();
        assert!(delete_site(&conn, ann.id, "s1").unwrap());
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }
}
