# analytics

Privacy-friendly web analytics for [Realm](https://yourrealm.eu). It counts
pageviews, referrers, campaigns, countries, devices and custom events for your
own sites. There are no cookies and no stored IP addresses. Realm's login is the
only login.

It is one small Rust server over one SQLite file. The Docker image is about 70
MB and idles near 2 MiB of memory.

## Install on Realm

Paste [`realm.tsx`](realm.tsx) into Home's install form, or choose the file.
That is the whole app definition, so there is nothing to build: Home bundles it,
and CI publishes the image, `ghcr.io/yourrealm/analytics`, for amd64 and arm64.
To update, use **Replace source…** on the app's Updates tab.

The install screen asks for one optional permission: internet access. It is
needed only for Google Search Console. Without it, visit counting works the
same.

Open the app's tile to reach the dashboard. Every Realm user has their own
sites, which nobody else sees.

## Track a site

1. Open **Sites**, add a site, and give it its hostnames (`example.com`,
   `*.example.com`). Only pages on those hosts count. An empty list accepts any.
2. Copy its snippet into every page's `<head>`:

   ```html
   <script type="module" src="https://<your app>/script.js" data-entity="<site id>"
     data-exit="false"></script>
   ```

3. Visit a page. The dashboard shows it within a minute. Visits from localhost
   are never counted.

Custom events take a name and optional properties:

```html
<script type="module">
import { event } from "https://<your app>/script.js";
event("signup", { properties: { plan: "pro" } });
</script>
```

The dashboard lists each event, and clicking one breaks it down by property.

The tracker is [Liwan](https://github.com/explodingcamera/liwan)'s, unmodified.
It counts a new pageview when the path changes, also in single-page apps, where
the browser supports the Navigation API.

### URLs that hold secrets

The tracker sends the page's full path. It drops the query string, except for
campaign parameters like `utm_source`. If your URLs contain something private,
like an invite token in the path, send pageviews yourself with the secret
removed. POST JSON as `text/plain` to `https://<your app>/api/event`:

```js
fetch("https://<your app>/api/event", {
  method: "POST",
  headers: { "Content-Type": "text/plain" },
  keepalive: true,
  body: JSON.stringify({
    name: "pageview", // or a custom event's name
    entity_id: "<site id>",
    url: location.href.replace(/\/invite\/[^/?#]+/, "/invite/_"),
    referrer: document.referrer || undefined,
    screen_width: "md", // optional: xs, sm, md, lg, xl or 2xl
    properties: undefined, // { key: value } for custom events
  }),
});
```

## Google Search Console

The dashboard can show the Google searches that led to a site, next to its
visits. It needs internet access, granted at install or later in the app's
settings.

Each user connects one Google service account. **Settings** walks you through it
step by step, with direct links: create the account in Google Cloud, download
its JSON key, choose the file, then add the account's email as a user on each
Search Console property. Then pick the property on each site in **Sites**. The
key is checked with Google and stored encrypted.

## Privacy

- No cookies and nothing stored in the visitor's browser.
- Each visitor's IP address is used once, to look up the country and build an
  anonymous ID. The IP itself is never stored.
- The anonymous ID is a hash of the IP, user agent, site and a salt that is
  replaced every day. Yesterday's IDs can't be recomputed, and one person on two
  sites can't be linked.
- Bots, unknown sites and pages on hosts you didn't allow are dropped.

This is how Plausible and similar tools work without a consent banner. Your
privacy policy should still say that you count visits.

## Development

UI work needs neither Realm nor Docker. Run the server and the dashboard side by
side:

```sh
pnpm install
pnpm dev:server              # the server on :3000, data in server/data
pnpm dev                     # the dashboard on :5173, signed in as "dev"
```

```sh
pnpm typecheck               # deno check, tsc and cargo clippy
pnpm test                    # realm.tsx tests and cargo test
pnpm test:e2e                # Playwright: the real tracker and the dashboard in Chromium
pnpm screenshots             # every view with a month of seeded traffic, into e2e/screenshots/
pnpm geoip                   # the DB-IP country database, so countries resolve locally
```

`CLAUDE.md` describes the architecture in more depth.

## Why not an existing tool

Plausible, GoatCounter, Umami and Liwan all keep their own user accounts, and
none of them can trust a login header from a reverse proxy. Behind Realm that
means a second login, or an admin assigning every user to their sites. Here,
Realm's login is the account and every user owns their sites. Liwan came
closest: this project uses its tracker and wire format and adapts parts of its
ingest.

## License

[AGPL-3.0](LICENSE), copyright Max Malm. If you run a modified version as a
service, you must offer its source to its users.

The tracker script (`server/assets/tracker.js`) is Liwan's and stays under the
Apache License 2.0, so loading it on your website does not bring your site under
the AGPL. [NOTICE](NOTICE) lists every third-party part, including the DB-IP
country data (CC BY 4.0).
