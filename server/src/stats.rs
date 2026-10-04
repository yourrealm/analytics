//! Dashboard numbers for one site and range, straight from raw events.
//!
//! Ranges and buckets are in the viewer's time zone. At per-user, few-site
//! volume a `GROUP BY` over `events_site_ts` is fast enough, so there are no
//! rollup tables to keep in sync.

use jiff::civil::Date;
use jiff::tz::TimeZone;
use jiff::{Timestamp, ToSpan, Zoned};
use rusqlite::{Connection, params};
use serde::Serialize;
use std::collections::HashSet;

/// Rows per breakdown list.
const TOP: i64 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Period {
    Today,
    Yesterday,
    Days(i64),
}

impl Period {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "today" => Period::Today,
            "yesterday" => Period::Yesterday,
            "7d" => Period::Days(7),
            "30d" => Period::Days(30),
            "90d" => Period::Days(90),
            "365d" => Period::Days(365),
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Bucket {
    Hour,
    Day,
}

/// A half-open `[start, end)` range cut into buckets, in Unix seconds.
#[derive(Debug, PartialEq, Eq)]
pub struct Range {
    pub start: i64,
    pub end: i64,
    pub bucket: Bucket,
    /// Bucket start times, ascending; the first is `start`.
    pub starts: Vec<i64>,
    pub from: String,
    pub to: String,
}

pub fn range(period: Period, now: i64, tz: &TimeZone) -> Range {
    let now = Timestamp::from_second(now)
        .unwrap_or(Timestamp::UNIX_EPOCH)
        .to_zoned(tz.clone());
    let today = now.date();
    let (first, last, bucket) = match period {
        Period::Today => (today, today, Bucket::Hour),
        Period::Yesterday => {
            let y = today.yesterday().unwrap_or(today);
            (y, y, Bucket::Hour)
        }
        Period::Days(n) => (
            today.checked_sub((n - 1).days()).unwrap_or(today),
            today,
            Bucket::Day,
        ),
    };
    let start = midnight(first, tz);
    let end = midnight(last.tomorrow().unwrap_or(last), tz);
    let step = match bucket {
        Bucket::Hour => 1.hour(),
        Bucket::Day => 1.day(),
    };
    let mut starts = Vec::new();
    let mut t = start.clone();
    while t < end {
        starts.push(t.timestamp().as_second());
        // Calendar arithmetic on a zoned time: a DST day has 23 or 25 hours.
        t = match t.checked_add(step) {
            Ok(next) => next,
            Err(_) => break,
        };
    }
    Range {
        start: start.timestamp().as_second(),
        end: end.timestamp().as_second(),
        bucket,
        starts,
        from: first.to_string(),
        to: last.to_string(),
    }
}

fn midnight(d: Date, tz: &TimeZone) -> Zoned {
    d.to_zoned(tz.clone())
        .unwrap_or_else(|_| Timestamp::UNIX_EPOCH.to_zoned(tz.clone()))
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Stats {
    pub from: String,
    pub to: String,
    pub bucket: Bucket,
    pub totals: Totals,
    pub series: Vec<Point>,
    pub pages: Vec<Row>,
    pub referrers: Vec<Row>,
    pub campaigns: Vec<Row>,
    pub countries: Vec<Row>,
    pub devices: Vec<Row>,
    pub events: Vec<EventRow>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Totals {
    pub visitors: i64,
    pub pageviews: i64,
    pub events: i64,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Point {
    /// Bucket start, Unix seconds.
    pub t: i64,
    pub visitors: i64,
    pub pageviews: i64,
}

/// One breakdown entry. `key` is `None` for "no value" (direct traffic,
/// unknown country).
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Row {
    pub key: Option<String>,
    pub visitors: i64,
    pub pageviews: i64,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct EventRow {
    pub name: String,
    pub count: i64,
    pub visitors: i64,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct PropRow {
    pub key: String,
    pub value: String,
    pub count: i64,
    pub visitors: i64,
}

pub fn totals(conn: &Connection, site: &str, r: &Range) -> rusqlite::Result<Totals> {
    conn.query_row(
        "SELECT COUNT(DISTINCT visitor),
                COALESCE(SUM(name = 'pageview'), 0),
                COALESCE(SUM(name <> 'pageview'), 0)
         FROM events WHERE site_id = ?1 AND ts >= ?2 AND ts < ?3",
        params![site, r.start, r.end],
        |row| {
            Ok(Totals {
                visitors: row.get(0)?,
                pageviews: row.get(1)?,
                events: row.get(2)?,
            })
        },
    )
}

pub fn stats(conn: &Connection, site: &str, r: &Range) -> rusqlite::Result<Stats> {
    let totals = totals(conn, site, r)?;
    Ok(Stats {
        from: r.from.clone(),
        to: r.to.clone(),
        bucket: r.bucket,
        totals,
        series: series(conn, site, r)?,
        pages: breakdown(conn, site, r, "path")?,
        referrers: breakdown(conn, site, r, "referrer")?,
        campaigns: breakdown(conn, site, r, "utm_source")?,
        countries: breakdown(conn, site, r, "country")?,
        devices: breakdown(conn, site, r, "device")?,
        events: events(conn, site, r)?,
    })
}

/// Pageviews and distinct visitors per bucket. Bucketing happens here, not
/// in SQL, because bucket edges follow the viewer's DST-aware calendar.
fn series(conn: &Connection, site: &str, r: &Range) -> rusqlite::Result<Vec<Point>> {
    let mut points: Vec<(i64, HashSet<String>)> =
        r.starts.iter().map(|_| (0, HashSet::new())).collect();
    let mut stmt = conn.prepare(
        "SELECT ts, visitor FROM events
         WHERE site_id = ?1 AND ts >= ?2 AND ts < ?3 AND name = 'pageview'",
    )?;
    let mut rows = stmt.query(params![site, r.start, r.end])?;
    while let Some(row) = rows.next()? {
        let ts: i64 = row.get(0)?;
        let i = r.starts.partition_point(|&s| s <= ts).saturating_sub(1);
        if let Some(p) = points.get_mut(i) {
            p.0 += 1;
            p.1.insert(row.get(1)?);
        }
    }
    Ok(r.starts
        .iter()
        .zip(points)
        .map(|(&t, (pageviews, visitors))| Point {
            t,
            visitors: visitors.len() as i64,
            pageviews,
        })
        .collect())
}

/// Top values of one column over pageviews. `column` is a fixed name from
/// this module, never user input.
fn breakdown(conn: &Connection, site: &str, r: &Range, column: &str) -> rusqlite::Result<Vec<Row>> {
    let sql = format!(
        "SELECT {column}, COUNT(DISTINCT visitor) AS v, COUNT(*) AS p FROM events
         WHERE site_id = ?1 AND ts >= ?2 AND ts < ?3 AND name = 'pageview'
         GROUP BY {column} ORDER BY v DESC, p DESC, {column} LIMIT ?4"
    );
    let mut stmt = conn.prepare(&sql)?;
    stmt.query_map(params![site, r.start, r.end, TOP], |row| {
        Ok(Row {
            key: row.get(0)?,
            visitors: row.get(1)?,
            pageviews: row.get(2)?,
        })
    })?
    .collect()
}

fn events(conn: &Connection, site: &str, r: &Range) -> rusqlite::Result<Vec<EventRow>> {
    let mut stmt = conn.prepare(
        "SELECT name, COUNT(*) AS c, COUNT(DISTINCT visitor) FROM events
         WHERE site_id = ?1 AND ts >= ?2 AND ts < ?3 AND name <> 'pageview'
         GROUP BY name ORDER BY c DESC, name LIMIT 50",
    )?;
    stmt.query_map(params![site, r.start, r.end], |row| {
        Ok(EventRow {
            name: row.get(0)?,
            count: row.get(1)?,
            visitors: row.get(2)?,
        })
    })?
    .collect()
}

/// Property values of one custom event, most common first.
pub fn props(
    conn: &Connection,
    site: &str,
    r: &Range,
    event: &str,
) -> rusqlite::Result<Vec<PropRow>> {
    let mut stmt = conn.prepare(
        "SELECT p.key, p.value, COUNT(*) AS c, COUNT(DISTINCT e.visitor)
         FROM events e, json_each(e.props) p
         WHERE e.site_id = ?1 AND e.ts >= ?2 AND e.ts < ?3 AND e.name = ?4
           AND e.props IS NOT NULL
         GROUP BY p.key, p.value ORDER BY p.key, c DESC LIMIT 200",
    )?;
    stmt.query_map(params![site, r.start, r.end, event], |row| {
        Ok(PropRow {
            key: row.get(0)?,
            value: row.get(1)?,
            count: row.get(2)?,
            visitors: row.get(3)?,
        })
    })?
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{self, Event};

    // 2026-08-30T22:30:00Z: 00:30 on the 31st in Stockholm.
    const NOW: i64 = 1_788_129_000;

    fn tz(name: &str) -> TimeZone {
        TimeZone::get(name).unwrap()
    }

    #[test]
    fn today_is_the_local_day_in_hours() {
        let r = range(Period::Today, NOW, &tz("Europe/Stockholm"));
        assert_eq!(r.from, "2026-08-31");
        assert_eq!(r.bucket, Bucket::Hour);
        assert_eq!(r.starts.len(), 24);
        assert_eq!(r.start, NOW - 30 * 60);
        assert_eq!(r.end - r.start, 24 * 3600);
    }

    #[test]
    fn day_ranges_end_today_and_follow_dst() {
        let r = range(Period::Days(7), NOW, &TimeZone::UTC);
        assert_eq!(
            (r.from.as_str(), r.to.as_str()),
            ("2026-08-24", "2026-08-30")
        );
        assert_eq!(r.starts.len(), 7);
        // Stockholm leaves summer time on 2026-10-25: that day has 25 hours.
        let late_oct = 1_793_534_400; // 2026-11-01T12:00:00Z
        let r = range(Period::Days(30), late_oct, &tz("Europe/Stockholm"));
        assert_eq!(r.starts.len(), 30);
        assert_eq!(r.end - r.start, 30 * 86400 + 3600);
        assert_eq!(Period::parse("30d"), Some(Period::Days(30)));
        assert_eq!(Period::parse("1y"), None);
    }

    fn event(ts: i64, name: &str, visitor: &str, path: &str, referrer: Option<&str>) -> Event {
        Event {
            site_id: "s1".into(),
            ts,
            name: name.into(),
            visitor: visitor.into(),
            host: "example.com".into(),
            path: path.into(),
            referrer: referrer.map(Into::into),
            ..Default::default()
        }
    }

    #[test]
    fn stats_count_visitors_pageviews_and_events() {
        let conn = db::open_memory();
        let u = db::user_by_name(&conn, "ann", 0).unwrap();
        db::insert_site(&conn, u.id, "s1", "Blog", &[], 0).unwrap();
        db::insert_site(&conn, u.id, "s2", "Other", &[], 0).unwrap();
        let r = range(Period::Days(7), NOW, &TimeZone::UTC);
        let day = 86400;
        for e in [
            event(r.start + 10, "pageview", "a", "/", Some("hn.com")),
            event(r.start + 20, "pageview", "a", "/post", None),
            event(r.start + day, "pageview", "b", "/", Some("hn.com")),
            event(r.start + day, "pageview", "c", "/", None),
            event(r.start + day + 5, "signup", "c", "/", None),
            // Outside the range, and another site: not counted.
            event(r.start - 1, "pageview", "z", "/", None),
            Event {
                site_id: "s2".into(),
                ..event(r.start, "pageview", "y", "/", None)
            },
        ] {
            db::insert_event(&conn, &e).unwrap();
        }
        conn.execute(
            "UPDATE events SET props = '{\"plan\":\"pro\"}' WHERE name = 'signup'",
            [],
        )
        .unwrap();

        let s = stats(&conn, "s1", &r).unwrap();
        assert_eq!(
            s.totals,
            Totals {
                visitors: 3,
                pageviews: 4,
                events: 1
            }
        );
        assert_eq!(s.series.len(), 7);
        assert_eq!((s.series[0].visitors, s.series[0].pageviews), (1, 2));
        assert_eq!((s.series[1].visitors, s.series[1].pageviews), (2, 2));
        assert_eq!(
            s.pages[0],
            Row {
                key: Some("/".into()),
                visitors: 3,
                pageviews: 3
            }
        );
        assert_eq!(s.referrers.len(), 2);
        assert_eq!(s.referrers[0].visitors, 2);
        assert_eq!(
            s.events,
            vec![EventRow {
                name: "signup".into(),
                count: 1,
                visitors: 1
            }]
        );

        let p = props(&conn, "s1", &r, "signup").unwrap();
        assert_eq!(
            p,
            vec![PropRow {
                key: "plan".into(),
                value: "pro".into(),
                count: 1,
                visitors: 1
            }]
        );
    }
}
