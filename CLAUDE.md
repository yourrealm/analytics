# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with
code in this repository.

## What this is

A self-hosted Plausible replacement, deployed as a Realm app. It counts
pageviews, referrers, countries, devices and custom events for each user's own
sites, with its own React dashboard behind Realm's login.

- `server/`: Rust (axum + rusqlite) over one SQLite file. It serves the tracker,
  the JSON API and the built dashboard. With DB-IP's country database it is the
  whole Docker image (`ghcr.io/yourrealm/analytics`, distroless/cc, amd64 and
  arm64). It idles near 2 MiB.
- `web/`: React 19 + Vite + Tailwind 4 + Recharts. `/` is the overview of all
  sites, `/?site=<id>` one site's dashboard, plus `/sites` and `/settings`, with
  filters in the URL. `web/src/api.ts` mirrors the server's response shapes, so
  change both together.
- `realm.tsx`: the Realm manifest (service, gate, trusted header, tile). It has
  no Realm UI surfaces and never runs in the container. We dropped the SDK pages
  for a real frontend.
- `e2e/`: Playwright runs Liwan's real tracker and the dashboard in Chromium
  against the server.

## Commands

```sh
pnpm install                 # SDK, Vite, React, Playwright (JSR registry via .npmrc)
pnpm dev:server              # server on :3000 (ANALYTICS_PORT overrides), data in server/data
pnpm dev                     # Vite on :5173, proxies /api to :3000 as user "dev"
pnpm typecheck               # deno check + tsc -p web + cargo clippy -D warnings
pnpm test                    # realm-sdk test (realm.test.ts) + cargo test
pnpm test:e2e                # Playwright; needs `pnpm exec playwright install chromium` once
pnpm screenshots             # every view with seeded data, into e2e/screenshots/
pnpm fmt / pnpm fmt:check    # deno fmt + cargo fmt
pnpm validate                # realm-sdk validate realm.tsx
pnpm build:web               # web/dist, which dev:server and the image serve
pnpm build:pack              # dist/analytics.realmpack
pnpm geoip                   # DB-IP country database into server/data/geo.mmdb
```

UI work needs neither Realm nor Docker: run `dev:server` and `dev` side by side.
The Vite proxy adds the `X-Analytics-User` header the gate would inject
(`ANALYTICS_DEV_USER` overrides the name). `realm-sdk dev` is not used: there
are no surfaces, and on Docker Desktop its loader proxy cannot reach containers.

Single tests:

```sh
cargo test --manifest-path server/Cargo.toml tracked_events_show_up   # by name substring
pnpm exec playwright test -g "not allow"                              # by title
ANALYTICS_TEST_MMDB=server/data/geo.mmdb cargo test --manifest-path server/Cargo.toml geo
```

CI (`.github/workflows/ci.yml`) runs `container`, `web` and `e2e`. On main it
pushes a multi-arch image to `ghcr.io/yourrealm/analytics` with the workflow's
own `GITHUB_TOKEN` (`latest` and the commit sha). The GHCR package must stay
public, or Realm cannot pull it. A monthly schedule rebuilds the image to pick
up new DB-IP data.

## Identity and auth (spans realm.tsx and server/src/api.rs)

- **The dashboard and owner routes** (everything except the two tracker paths
  below): the Realm gate injects `X-Analytics-User: <username>`
  (`getTrustedHeaders`) for a Home session and strips any client copy. The
  server trusts it and creates the user row on first sight. `USER_HEADER` in
  `api.rs` must match realm.tsx. Every site query is scoped by `user_id`, and a
  site that isn't yours is a 404. The dashboard is a same-origin page on the
  gated subdomain, so its `fetch` calls carry the header without any client-side
  auth. A browser with no session gets Home's login redirect.
- **Tracked pages** are anonymous and cross-origin. `verifyAnonymousRequest`
  admits exactly `POST`/`OPTIONS /api/event` and `GET`/`HEAD /script.js`. The
  server does the real checks: the site exists and the page's host is allowed.
  `realm.test.ts` asserts the gate's exact surface.
- `router.rateLimit` (600 per 10s) is per client IP and covers every request
  through the ingress, the dashboard's included. Keep it generous: many visitors
  can share one IP (offices, carrier NAT, a CDN not declared to Realm), and over
  the limit their pageviews are dropped with a 429.

## Ingest (server/src/ingest.rs, api.rs)

- The wire format is Liwan's `EventRequest`, so sites load Liwan's unmodified
  tracker (`server/assets/tracker.js`, served at `/script.js`). Snippet:
  `<script type="module" src="<app>/script.js" data-entity="<site id>" data-exit="false">`.
  Custom events:
  `import { event } from "<app>/script.js"; event("name", { properties })`.
- The tracker posts `text/plain` (no CORS preflight). The server ignores the
  content type and sets `Access-Control-Allow-Origin: *` on its own responses.
  The ingress adds no CORS headers.
- `screen_width` is a bucket (`xs`, `sm`, `md`, `lg`, `xl`, `2xl`), never
  pixels. `device()` maps it to mobile, tablet, laptop or desktop.
- Unknown sites, disallowed hosts, bots (UA tokens, including `HeadlessChrome`),
  localhost referrers and exit signals are dropped with a 204. Malformed bodies
  get a 400. The tracker itself skips localhost pages.
- **Client IP**: the ingress overwrites `X-Real-IP` and `X-Forwarded-For` with
  the one address it resolved, so `client_ip` trusts `X-Real-IP`. The TCP peer
  is a fallback for `dev:server`.
- **Visitor IDs** (`visitor.rs`, after Liwan):
  `blake3(ip, user agent, salt,
  site)`, shortened to 16 characters. The salt
  rotates at UTC midnight and the old one is overwritten. No cookies.
- **Countries** (`geo.rs`): DB-IP Lite country `.mmdb` (CC BY 4.0), baked into
  the image by `scripts/geoip.sh` and memory-mapped. A missing file turns
  countries off. The dashboard must keep its "IP Geolocation by DB-IP" link,
  because the license requires it. Don't switch to MaxMind GeoLite2: its EULA
  forbids shipping the file in a public image.

## Google Search Console (server/src/google.rs, crypto.rs)

- **One service account key per user**, chosen in Settings. It covers all of
  that user's sites; each site picks a property the account can read. A single
  install-wide key was rejected: any user could then attach any property it can
  see.
- The server signs an RS256 JWT with the key (`crypto::jwt`, on ring), trades it
  for an hour-long token (cached per user), and calls `searchAnalytics.query`
  twice per view: totals, then the top 10 queries. Results are cached for an
  hour per (property, from, to).
- **Never trust the key file's `token_uri`**: the token URL is fixed, so a
  pasted file can't choose where the server sends requests. Tests override both
  URLs with `ANALYTICS_GOOGLE_TOKEN_URL` and `ANALYTICS_GOOGLE_API`.
- **Optional egress.** `r.host.egress().optional()` in realm.tsx. Only when it
  is granted does realm.tsx set `ANALYTICS_GOOGLE=1`; without it, the API
  answers with a reason the UI shows, and visit counting is unaffected.
- **Keys are sealed** with AES-256-GCM (`crypto::Sealer`). The key comes from
  `ANALYTICS_SECRET`, a non-rotatable `r.stableSecret("credentials")`, or
  without it from a random `secret` file in the data dir. A changed secret means
  users paste their key again; the API says so.
- Setting a site's property checks it against the account's list. Disconnecting
  unlinks every site of that user.
- The API is the Search Console API v1, `https://searchconsole.googleapis.com`
  (check its discovery document,
  `searchconsole.googleapis.com/$discovery/rest?version=v1`, before changing
  calls). The paths are still `webmasters/v3/...`; enum values like `dataState`
  are uppercase. The old `www.googleapis.com` discovery is gone.
- The setup guide in `web/src/SearchConsole.tsx` links straight to Google's
  pages (`LINKS`), with labels taken from Google's docs. Re-check them against
  those docs when Google changes its console, not from memory.
- Search Console dates are Pacific time and lag 2 to 3 days; the card says so.
- `server/src/testdata/` holds a throwaway RSA key wrapped as a service account
  file (its `token_uri` points elsewhere on purpose). `google::fake` checks JWT
  signatures against it in Rust tests; `e2e/google.mjs` fakes Google for
  Playwright.

## Stats (server/src/stats.rs)

- Raw events only, no rollups: `GROUP BY` over the `events_site_ts` index is
  fast enough at per-user, few-site volume.
- Periods (`today`, `yesterday`, `7d`, `30d`, `90d`, `365d`) and buckets are in
  the user's time zone (set in Settings, default UTC). Series are bucketed in
  Rust, because bucket edges follow DST.
- Breakdowns count pageviews only. Totals' `visitors` counts distinct IDs across
  all events. IDs rotate daily, so multi-day visitor counts are upper bounds, as
  in Plausible.

## Frontend (web/)

- Serving: `api::router` serves `ANALYTICS_WEB_DIR` (`/app/web` in the image).
  `/assets/*` is cached as immutable (Vite fingerprints it). Any other non-API
  path falls back to `index.html` with `no-cache`, so client-side routes load.
  Unknown `/api/*` paths stay JSON 404s.
- The overview (`/` without `?site`) is a card per site, as on Plausible's home:
  `GET /api/overview` gives visitors in the last 24 hours, in the 24 before, and
  per hour (`stats::glance`). The window is rolling, so no time zone, and it
  includes the current second.
- Sites is a list and a detail panel, stacked on a phone. `?site=<id>` picks a
  site and `?site=new` opens the add form (also shown when there are none).
  `GET /api/sites` adds each site's `last_event` (`db::list_sites_activity`) for
  the status dot. Visitor counts live on the overview, not here. Settings holds
  what is per user: the Search Console account (a collapsible step guide until
  connected) and the time zone.
- No router or data library: `route.ts` (path plus query, history API) and
  `useApi` in `api.ts` (fetch, reload, refresh interval). The dashboard
  refreshes stats every minute.
- The mark in `web/public` (logo, favicon, icon-512) comes from realm-icons
  (`yourrealm/icons`): `ph/chart-bar` with `grain=0`, because the default heavy
  grain looks like noise at header size. It is that service's five-file set
  (logo, dark logo, favicon SVG and ICO, 512 px PNG). `router.icon` hotlinks
  `logo.svg` through jsDelivr from `yourrealm/analytics@main`.
- `e2e/screenshots.spec.ts` seeds a month of traffic through the real ingest
  API, backdates it in SQLite, and screenshots every view in light, dark and
  phone. With `server/data/geo.mmdb` present (`pnpm geoip`) countries resolve.
  CI uploads the folder as an artifact.
- Theme: CSS variables in `index.css`, light and dark via
  `prefers-color-scheme`, exposed to Tailwind through `@theme inline`. Charts
  use the same variables.
- In e2e, the tracked site (`e2e/site.mjs`) must sit on loopback too, through
  `--host-resolver-rules`. Otherwise Chrome's Private Network Access blocks the
  script. This can't happen in production, where both sides are public HTTPS.
  The owner's browser context sends `X-Analytics-User` itself, standing in for
  the gate.

## Server conventions

- `db::MIGRATIONS` is an append-only array keyed by `PRAGMA user_version`. Add a
  new entry at the end and never edit an existing one.
- Tests live in `#[cfg(test)]` modules. API tests build the router with
  `AppState::with_clock(db::open_memory(), Geo::none(), fixed_now)` and drive it
  with `tower::ServiceExt::oneshot`.
- The binary also runs as `analytics healthcheck`, the container healthcheck
  (exec form, because the image has no shell). Env: `PORT` (3000),
  `ANALYTICS_DATA_DIR` (`/data`), `ANALYTICS_GEOIP` (`/app/geo.mmdb`),
  `ANALYTICS_WEB_DIR` (`/app/web`; without an `index.html` it serves the API
  only), `ANALYTICS_SECRET`, `ANALYTICS_GOOGLE`.
- The project is AGPL-3.0-only (`LICENSE`), copyright Max Malm. Liwan-derived
  code (Apache-2.0, text in `LICENSES/Apache-2.0.txt`) and DB-IP data (CC BY
  4.0) are credited in `NOTICE`. Keep it current. `tracker.js` stays Apache-2.0,
  so tracked sites are not bound by the AGPL; `README.md` says so.

## Conventions (from realmdo)

- Don't commit or push unless asked.
- Relative imports carry `.ts`/`.tsx` extensions, in `web/` too. No `npm:` or
  `jsr:` prefixes: dependencies live in `package.json`.
- Short code comments. No backwards-compat shims, no dynamic imports, and don't
  export what nothing imports.
- Write docs in plain language: conclusion first, one idea per sentence, no em
  dashes.
