//! Cookieless visitor IDs, after Liwan (Apache-2.0, see NOTICE).
//!
//! A visitor is `blake3(ip, user agent, salt, site)`. The salt is random and
//! replaced once a day, overwriting the old one, so yesterday's IDs can never
//! be recomputed. The site is part of the hash, so one person on two sites
//! cannot be linked.

use jiff::Timestamp;
use jiff::civil::Time;
use jiff::tz::TimeZone;
use std::io::Read;
use std::net::IpAddr;

/// Local hour (in the server's zone) at which the salt rotates.
const ROTATION_HOUR: i8 = 0;
const ID_CHARS: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

pub fn id(ip: &IpAddr, user_agent: &str, salt: &str, site_id: &str) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(ip.to_string().as_bytes());
    hasher.update(user_agent.as_bytes());
    hasher.update(salt.as_bytes());
    hasher.update(site_id.as_bytes());
    encode(hasher.finalize().as_bytes())
}

/// Random ID for a request with nothing to hash: counts once, links to nothing.
pub fn random_id() -> String {
    encode(&random_bytes::<16>())
}

pub fn new_salt() -> String {
    encode(&random_bytes::<16>())
}

/// Whether a salt set at `updated_at` predates the latest rotation time.
pub fn should_rotate(updated_at: i64, now: i64, tz: &TimeZone) -> bool {
    let now = Timestamp::from_second(now)
        .unwrap_or(Timestamp::UNIX_EPOCH)
        .to_zoned(tz.clone());
    let Ok(today) = now
        .date()
        .to_datetime(Time::constant(ROTATION_HOUR, 0, 0, 0))
        .to_zoned(tz.clone())
    else {
        return true;
    };
    let latest = if now < today {
        today.yesterday().unwrap_or(today)
    } else {
        today
    };
    updated_at < latest.timestamp().as_second()
}

fn encode(bytes: &[u8]) -> String {
    bytes
        .iter()
        .take(16)
        .map(|b| ID_CHARS[(b % 62) as usize] as char)
        .collect()
}

pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut buf = [0u8; N];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut buf))
        .expect("read /dev/urandom");
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_depend_on_every_part() {
        let ip: IpAddr = "203.0.113.7".parse().unwrap();
        let other: IpAddr = "203.0.113.8".parse().unwrap();
        let a = id(&ip, "ua", "salt", "site");
        assert_eq!(a.len(), 16);
        assert_eq!(a, id(&ip, "ua", "salt", "site"));
        assert_ne!(a, id(&other, "ua", "salt", "site"));
        assert_ne!(a, id(&ip, "ua2", "salt", "site"));
        assert_ne!(a, id(&ip, "ua", "salt2", "site"));
        assert_ne!(a, id(&ip, "ua", "salt", "site2"));
        assert_ne!(random_id(), random_id());
        assert_ne!(new_salt(), new_salt());
    }

    #[test]
    fn salt_rotates_after_local_midnight() {
        let tz = TimeZone::get("Europe/Stockholm").unwrap();
        // 2026-08-30T22:30:00Z is 00:30 on the 31st in Stockholm.
        let now = 1_788_129_000;
        // Set at 23:59 on the 30th, local: before midnight, so rotate.
        assert!(should_rotate(now - 31 * 60, now, &tz));
        // Set at 00:10 on the 31st, local: after midnight, so keep.
        assert!(!should_rotate(now - 20 * 60, now, &tz));
        assert!(should_rotate(0, now, &TimeZone::UTC));
    }
}
