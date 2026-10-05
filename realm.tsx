// analytics: pageviews, referrers, countries and custom events for your own
// sites, per user. One container: the Rust server, which also serves the
// React dashboard from web/.
//
// Identity. The gate injects X-Analytics-User on every request that carries a
// Home session, and strips any copy the client sent. The backend trusts that
// header and creates its local user row on first sight. Each user sees only
// the sites they created. The dashboard is an ordinary page on the app's
// gated subdomain, so its API calls carry the header like any other request.
//
// Tracked pages are anonymous and cross-origin: the gate admits exactly the
// tracker script and the event endpoint on shape. The backend checks that the
// site exists and the page's host is allowed, because the gate cannot (it
// never sees the body).
//
// Google Search Console is optional, and so is internet access: without the
// egress grant the app still counts visits, and the dashboard explains why
// search terms are off. Users' service account keys are sealed with a stable
// secret before they are stored.

import { createApp, r } from "@yourrealm/sdk";

export default createApp({
  name: "analytics",
  description:
    "Privacy-friendly web analytics: pageviews, referrers, countries and custom events, per user.",

  configSchema: {
    egress: r.host
      .egress()
      .reason("Read search terms from Google Search Console")
      .optional(),
  },

  getTrustedHeaders: (user) => ({
    "X-Analytics-User": user.username,
  }),

  verifyAnonymousRequest: (request) => {
    const { pathname } = new URL(request.url);
    if (pathname === "/api/event") {
      // OPTIONS: a custom event() call may trigger a CORS preflight, and a
      // denied preflight is an opaque failure on the tracked page.
      return request.method === "POST" || request.method === "OPTIONS";
    }
    if (pathname === "/script.js") {
      return request.method === "GET" || request.method === "HEAD";
    }
    return false;
  },

  info: ({ services }) => ({
    scriptUrl: {
      label: "Tracker script",
      value: services.api ? `${services.api}/script.js` : null,
      description:
        "Open the app and add a site to get the snippet for your pages.",
    },
  }),

  getDesiredState: ({ config }) => ({
    egress: config.egress,
    services: {
      api: {
        image: "ghcr.io/yourrealm/analytics:latest",
        env: {
          // Not rotatable: it seals stored keys, and a new value would make
          // every stored key unreadable (users would paste them again).
          ANALYTICS_SECRET: r.stableSecret("credentials"),
          ...(config.egress ? { ANALYTICS_GOOGLE: "1" } : {}),
        },
        router: {
          containerPort: 3000,
          // The tile opens the dashboard, under a capitalized label (the
          // app's own name stays lowercase, as an identifier).
          tile: true,
          name: "Analytics",
          // Home fetches the icon itself, once, and needs a public https URL
          // with an image content type; jsDelivr serves the file in this repo
          // that way. Made with realm-icons: ph/chart-bar, grain=0.
          icon:
            "https://cdn.jsdelivr.net/gh/yourrealm/analytics@main/web/public/logo.svg",
          // Per client IP, and tracking and the dashboard share it. Generous,
          // because many visitors can share one IP (offices, carrier NAT, an
          // undeclared CDN) and their pageviews would be dropped with a 429.
          rateLimit: { requests: 600, window: "10s" },
        },
        volumes: [
          {
            hostPath: "./data",
            containerPath: "/data",
            backup: { kind: "sqlite", file: "analytics.db" },
          },
        ],
        // Exec form: the distroless runtime has no shell, so the binary
        // self-checks via its `healthcheck` arg.
        healthcheck: {
          test: ["CMD", "/app/analytics", "healthcheck"],
          interval: 5,
          timeout: 3,
          retries: 12,
          startPeriod: 2,
        },
      },
    },
  }),
});
