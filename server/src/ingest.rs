//! Turns one tracker request into a stored event, or drops it.
//!
//! The wire format is Liwan's (`EventRequest` in its `routes/event.rs`), so
//! sites load Liwan's tracker unchanged. The filtering follows Liwan too:
//! unknown sites, disallowed hosts, bots and local referrers are dropped
//! silently, so a tracked page never sees an error it cannot act on.

use crate::db::Event;
use crate::hosts;
use serde::Deserialize;
use std::collections::BTreeMap;
use url::Url;

const MAX_NAME: usize = 255;
const MAX_URL: usize = 2048;
const MAX_REFERRER: usize = 256;
const MAX_USER_AGENT: usize = 1024;
const MAX_PROPERTIES: usize = 30;
const MAX_PROPERTY_KEY: usize = 64;
const MAX_PROPERTY_VALUE: usize = 255;
const MAX_QUERY_VALUE: usize = 255;

/// Substrings of a lowercased User-Agent that mark a crawler or a tool.
/// Liwan's `crawlers.txt` tokens plus the generic ones its UA parser covers.
const BOT_TOKENS: &[&str] = &[
    "bot",
    "crawl",
    "spider",
    "slurp",
    "headless",
    "lighthouse",
    "pagespeed",
    "preview",
    "monitor",
    "curl/",
    "wget/",
    "python-",
    "go-http-client",
    "okhttp",
    "java/",
    "bytespider",
    "applebot",
    "googleother",
    "yisouspider",
];

#[derive(Debug, Deserialize)]
pub struct Request {
    pub entity_id: String,
    pub name: String,
    pub url: String,
    pub referrer: Option<String>,
    pub screen_width: Option<String>,
    pub properties: Option<BTreeMap<String, serde_json::Value>>,
    #[serde(default)]
    pub exit: bool,
}

/// What the handler knows about the request besides its body.
pub struct Context {
    pub now: i64,
    /// The visitor ID, computed by the caller (it owns the salt).
    pub visitor: String,
    pub country: Option<String>,
}

/// Why a well-formed request was not stored. Only for tests and logs.
#[derive(Debug, PartialEq, Eq)]
pub enum Drop {
    Exit,
    Bot,
    LocalReferrer,
    UnknownSite,
    HostNotAllowed,
}

/// Parses the body. The tracker sends `text/plain` to avoid a CORS
/// preflight, so this ignores the content type.
pub fn parse(body: &[u8]) -> Result<Request, String> {
    let r: Request = serde_json::from_slice(body).map_err(|e| format!("invalid json: {e}"))?;
    if r.entity_id.trim().is_empty() || r.entity_id.len() > MAX_NAME {
        return Err("invalid entity_id".into());
    }
    if r.name.trim().is_empty() || r.name.len() > MAX_NAME {
        return Err("invalid name".into());
    }
    if r.url.len() > MAX_URL {
        return Err("url too long".into());
    }
    if r.referrer
        .as_deref()
        .is_some_and(|v| v.len() > MAX_REFERRER)
    {
        return Err("referrer too long".into());
    }
    Ok(r)
}

/// Checks done before touching the database: cheap and site-independent.
pub fn precheck(r: &Request, user_agent: Option<&str>) -> Result<(), Drop> {
    if r.exit {
        // Time on page is not tracked (yet): exit signals carry nothing else.
        return Err(Drop::Exit);
    }
    if user_agent.is_some_and(is_bot) {
        return Err(Drop::Bot);
    }
    Ok(())
}

pub fn is_bot(user_agent: &str) -> bool {
    if user_agent.len() > MAX_USER_AGENT {
        return true;
    }
    // Cubot is a phone brand, not a bot.
    let ua = user_agent.to_ascii_lowercase().replace("cubot", "");
    BOT_TOKENS.iter().any(|t| ua.contains(t))
}

/// Builds the event. `hostnames` is the site's allow list, `None` when the
/// site does not exist.
pub fn build(
    r: Request,
    hostnames: Option<&[String]>,
    ctx: Context,
) -> Result<Result<Event, Drop>, String> {
    let url = Url::parse(&r.url).map_err(|_| "invalid url".to_string())?;
    let host = url
        .host_str()
        .ok_or("url has no host")?
        .trim_start_matches("www.")
        .to_ascii_lowercase();
    let props = properties(r.properties)?;

    let Some(hostnames) = hostnames else {
        return Ok(Err(Drop::UnknownSite));
    };
    if !hosts::allowed(url.host_str().unwrap_or_default(), hostnames) {
        return Ok(Err(Drop::HostNotAllowed));
    }
    let referrer = match referrer_host(r.referrer.as_deref()) {
        Referrer::Local => return Ok(Err(Drop::LocalReferrer)),
        Referrer::None => None,
        // A click within the site is navigation, not a referral.
        Referrer::Host(h) if h == host => None,
        Referrer::Host(h) => Some(h),
    };

    let path = url.path();
    let path = if path.len() > 1 {
        path.trim_end_matches('/')
    } else {
        path
    };

    Ok(Ok(Event {
        site_id: r.entity_id,
        ts: ctx.now,
        name: r.name.trim().to_string(),
        visitor: ctx.visitor,
        host,
        path: path.to_string(),
        referrer,
        utm_source: query(
            &url,
            &["utm_source", "source", "ref", "referrer", "referer"],
        ),
        utm_medium: query(&url, &["utm_medium", "medium"]),
        utm_campaign: query(&url, &["utm_campaign", "campaign"]),
        country: ctx.country,
        device: device(r.screen_width.as_deref()),
        props,
    }))
}

enum Referrer {
    None,
    Local,
    Host(String),
}

/// The referrer reduced to its host, without `www.`.
fn referrer_host(referrer: Option<&str>) -> Referrer {
    let Some(raw) = referrer.map(str::trim).filter(|r| !r.is_empty()) else {
        return Referrer::None;
    };
    let with_scheme = if raw.contains("://") {
        raw.to_string()
    } else {
        format!("http://{raw}")
    };
    let Some(host) = Url::parse(&with_scheme)
        .ok()
        .and_then(|u| u.host_str().map(str::to_ascii_lowercase))
    else {
        return Referrer::None;
    };
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    if host == "localhost"
        || host.ends_with(".localhost")
        || bare.parse::<std::net::IpAddr>().is_ok()
    {
        return Referrer::Local;
    }
    let host = host.trim_start_matches("www.").to_string();
    if host.len() <= 3 {
        return Referrer::None;
    }
    Referrer::Host(host)
}

fn query(url: &Url, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|k| url.query_pairs().find(|(n, _)| n == k).map(|(_, v)| v))
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty() && v.len() <= MAX_QUERY_VALUE)
}

/// Device class from the tracker's width bucket: `xs` under 480px, `sm`
/// under 768, `md` under 1024, `lg` under 1280, `xl` under 1536, `2xl` above
/// (`screen_width` in Liwan's tracker). It never sends pixels.
fn device(screen_width: Option<&str>) -> Option<&'static str> {
    Some(match screen_width?.trim() {
        "xs" | "sm" => "mobile",
        "md" => "tablet",
        "lg" | "xl" => "laptop",
        "2xl" => "desktop",
        _ => return None,
    })
}

/// Properties as a JSON object of strings, coerced the way Liwan does.
fn properties(raw: Option<BTreeMap<String, serde_json::Value>>) -> Result<Option<String>, String> {
    let mut out = BTreeMap::new();
    for (key, value) in raw.unwrap_or_default() {
        let value = match value {
            serde_json::Value::Null => continue,
            serde_json::Value::String(s) if s.is_empty() => continue,
            serde_json::Value::String(s) => s,
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::Bool(b) => b.to_string(),
            _ => return Err("property values must be strings, numbers or booleans".into()),
        };
        let key = key.trim();
        if key.is_empty() || key.chars().count() > MAX_PROPERTY_KEY {
            return Err(format!(
                "property keys must be 1 to {MAX_PROPERTY_KEY} characters"
            ));
        }
        if value.chars().count() > MAX_PROPERTY_VALUE {
            return Err(format!(
                "property values cannot exceed {MAX_PROPERTY_VALUE} characters"
            ));
        }
        out.insert(key.to_string(), value);
    }
    if out.len() > MAX_PROPERTIES {
        return Err(format!("at most {MAX_PROPERTIES} properties"));
    }
    Ok((!out.is_empty()).then(|| serde_json::to_string(&out).unwrap_or_default()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHROME: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36";

    fn ctx() -> Context {
        Context {
            now: 100,
            visitor: "v".into(),
            country: Some("SE".into()),
        }
    }

    fn req(json: &str) -> Request {
        parse(json.as_bytes()).unwrap()
    }

    #[test]
    fn a_pageview_becomes_an_event() {
        let r = req(
            r#"{"entity_id":"s1","name":"pageview","url":"https://www.example.com/blog/?utm_source=hn&x=1","referrer":"https://news.ycombinator.com/item?id=1","screen_width":"xs"}"#,
        );
        let e = build(r, Some(&[]), ctx()).unwrap().unwrap();
        assert_eq!(e.site_id, "s1");
        assert_eq!(e.host, "example.com");
        assert_eq!(e.path, "/blog");
        assert_eq!(e.referrer.as_deref(), Some("news.ycombinator.com"));
        assert_eq!(e.utm_source.as_deref(), Some("hn"));
        assert_eq!(e.device, Some("mobile"));
        assert_eq!(e.country.as_deref(), Some("SE"));
        assert_eq!(e.props, None);
    }

    #[test]
    fn custom_events_keep_their_properties_as_strings() {
        let r = req(
            r#"{"entity_id":"s1","name":"signup","url":"https://example.com/","properties":{"plan":"pro","seats":3,"trial":true,"none":null}}"#,
        );
        let e = build(r, Some(&[]), ctx()).unwrap().unwrap();
        assert_eq!(e.name, "signup");
        assert_eq!(e.path, "/");
        assert_eq!(
            e.props.as_deref(),
            Some(r#"{"plan":"pro","seats":"3","trial":"true"}"#)
        );
        let bad = req(
            r#"{"entity_id":"s1","name":"x","url":"https://example.com/","properties":{"a":[1]}}"#,
        );
        assert!(build(bad, Some(&[]), ctx()).is_err());
    }

    #[test]
    fn unwanted_requests_are_dropped() {
        let pv = r#"{"entity_id":"s1","name":"pageview","url":"https://example.com/"}"#;
        assert_eq!(
            build(req(pv), None, ctx()).unwrap().unwrap_err(),
            Drop::UnknownSite
        );
        let only = vec!["other.com".to_string()];
        assert_eq!(
            build(req(pv), Some(&only), ctx()).unwrap().unwrap_err(),
            Drop::HostNotAllowed
        );
        let local = r#"{"entity_id":"s1","name":"pageview","url":"https://example.com/","referrer":"http://localhost:3000/"}"#;
        assert_eq!(
            build(req(local), Some(&[]), ctx()).unwrap().unwrap_err(),
            Drop::LocalReferrer
        );
        let exit =
            req(r#"{"entity_id":"s1","name":"pageview","url":"https://example.com/","exit":true}"#);
        assert_eq!(precheck(&exit, Some(CHROME)), Err(Drop::Exit));
        assert_eq!(precheck(&req(pv), Some("Googlebot/2.1")), Err(Drop::Bot));
        assert_eq!(precheck(&req(pv), Some(CHROME)), Ok(()));
        assert!(!is_bot(
            "Mozilla/5.0 (Linux; Android 12; CUBOT_X30) Chrome/120"
        ));
    }

    #[test]
    fn self_referrals_are_not_referrers() {
        let r = req(
            r#"{"entity_id":"s1","name":"pageview","url":"https://example.com/b","referrer":"https://www.example.com/a"}"#,
        );
        assert_eq!(build(r, Some(&[]), ctx()).unwrap().unwrap().referrer, None);
    }

    #[test]
    fn malformed_bodies_are_rejected() {
        assert!(parse(b"nope").is_err());
        assert!(parse(br#"{"entity_id":"","name":"pageview","url":"https://a.com/"}"#).is_err());
        assert!(parse(br#"{"entity_id":"s","name":" ","url":"https://a.com/"}"#).is_err());
        let r = req(r#"{"entity_id":"s","name":"pageview","url":"not a url"}"#);
        assert!(build(r, Some(&[]), ctx()).is_err());
    }

    #[test]
    fn devices_follow_screen_width() {
        assert_eq!(device(Some("xs")), Some("mobile"));
        assert_eq!(device(Some("sm")), Some("mobile"));
        assert_eq!(device(Some("md")), Some("tablet"));
        assert_eq!(device(Some("xl")), Some("laptop"));
        assert_eq!(device(Some("2xl")), Some("desktop"));
        assert_eq!(device(Some("1280")), None);
        assert_eq!(device(None), None);
    }
}
