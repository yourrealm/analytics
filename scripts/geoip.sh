#!/bin/sh
# Downloads DB-IP's free country database (CC BY 4.0, https://db-ip.com) to
# $1. A month's file appears early in that month, so fall back to last month.
set -eu
out="$1"
mkdir -p "$(dirname "$out")"
this=$(date -u +%Y-%m)
last=$(date -u -d '1 month ago' +%Y-%m 2>/dev/null || date -u -v-1m +%Y-%m)
for month in "$this" "$last"; do
  url="https://download.db-ip.com/free/dbip-country-lite-$month.mmdb.gz"
  if curl -fsSL "$url" | gunzip > "$out.tmp" 2>/dev/null && [ -s "$out.tmp" ]; then
    mv "$out.tmp" "$out"
    echo "GeoIP: $url -> $out"
    exit 0
  fi
done
rm -f "$out.tmp"
echo "GeoIP: no DB-IP database found for $this or $last" >&2
exit 1
