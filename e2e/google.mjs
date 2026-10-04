// A fake of Google's token and Search Console endpoints for e2e. The server
// is pointed here by ANALYTICS_GOOGLE_TOKEN_URL and ANALYTICS_GOOGLE_API
// (playwright.config.ts). The Rust tests check JWT signatures; this only
// checks that one arrived.

import { createServer } from "node:http";

const port = Number(process.env.GOOGLE_PORT ?? 3903);
const PROPERTIES = ["sc-domain:blog.example", "https://shop.example/"];
const QUERIES = [
  ["realm self hosted", 41, 520, 3.1],
  ["cookieless analytics", 23, 910, 7.4],
  ["plausible alternative rust", 17, 640, 9.8],
  ["self hosted web analytics", 12, 1300, 14.2],
  ["sqlite analytics", 6, 210, 6.3],
];

const json = (res, status, body) => {
  res.writeHead(status, { "content-type": "application/json" });
  res.end(JSON.stringify(body));
};

createServer(async (req, res) => {
  let body = "";
  for await (const chunk of req) body += chunk;
  const url = new URL(req.url ?? "/", "http://localhost");
  if (url.pathname === "/healthz") return res.end("ok");

  if (url.pathname === "/token") {
    const assertion = new URLSearchParams(body).get("assertion") ?? "";
    return assertion.split(".").length === 3
      ? json(res, 200, { access_token: "fake-token", expires_in: 3600 })
      : json(res, 400, { error: "invalid_grant", error_description: "Invalid JWT." });
  }
  if (req.headers.authorization !== "Bearer fake-token") {
    return json(res, 401, { error: { message: "Request had invalid authentication credentials." } });
  }
  if (url.pathname === "/webmasters/v3/sites") {
    return json(res, 200, {
      siteEntry: PROPERTIES.map((siteUrl) => ({ siteUrl, permissionLevel: "siteRestrictedUser" })),
    });
  }
  const m = url.pathname.match(/^\/webmasters\/v3\/sites\/([^/]+)\/searchAnalytics\/query$/);
  if (m) {
    const q = JSON.parse(body);
    if (!q.dimensions?.length) {
      const clicks = QUERIES.reduce((n, r) => n + r[1], 0);
      const impressions = QUERIES.reduce((n, r) => n + r[2], 0);
      return json(res, 200, { rows: [{ clicks, impressions, ctr: clicks / impressions, position: 8.6 }] });
    }
    return json(res, 200, {
      rows: QUERIES.map(([query, clicks, impressions, position]) => ({
        keys: [query],
        clicks,
        impressions,
        ctr: clicks / impressions,
        position,
      })),
    });
  }
  json(res, 404, { error: { message: "not found" } });
}).listen(port, "127.0.0.1");
