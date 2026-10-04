//! Country lookup in DB-IP Lite (CC BY 4.0, https://db-ip.com). The `.mmdb`
//! ships in the image and is memory-mapped, so it costs page cache, not heap.
//! Without the file, lookups return `None` and everything else works.

use maxminddb::{Mmap, PathElement, Reader};
use std::net::IpAddr;
use std::path::Path;

pub struct Geo(Option<Reader<Mmap>>);

impl Geo {
    pub fn open(path: &Path) -> Self {
        if !path.exists() {
            eprintln!("no GeoIP database at {}, countries off", path.display());
            return Self(None);
        }
        // SAFETY: the file is read-only in the image and never rewritten
        // while the server runs.
        match unsafe { Reader::open_mmap(path) } {
            Ok(r) => Self(Some(r)),
            Err(e) => {
                eprintln!("GeoIP database {}: {e}, countries off", path.display());
                Self(None)
            }
        }
    }

    #[cfg(test)]
    pub fn none() -> Self {
        Self(None)
    }

    /// ISO 3166-1 alpha-2 code, e.g. `SE`.
    pub fn country(&self, ip: IpAddr) -> Option<String> {
        let reader = self.0.as_ref()?;
        let path = [PathElement::Key("country"), PathElement::Key("iso_code")];
        reader
            .lookup(ip)
            .ok()?
            .decode_path::<String>(&path)
            .ok()
            .flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_database_means_no_countries() {
        let g = Geo::open(Path::new("/nonexistent/geo.mmdb"));
        assert_eq!(g.country("1.1.1.1".parse().unwrap()), None);
    }

    /// Runs against a real file when `ANALYTICS_TEST_MMDB` points at one.
    #[test]
    fn real_database_resolves_a_known_address() {
        let Some(path) = std::env::var_os("ANALYTICS_TEST_MMDB") else {
            return;
        };
        let g = Geo::open(Path::new(&path));
        assert_eq!(g.country("8.8.8.8".parse().unwrap()).as_deref(), Some("US"));
        assert_eq!(g.country("10.0.0.1".parse().unwrap()), None);
    }
}
