// End-to-end: the real tracker and the real dashboard in a browser, against
// the real server. e2e/site.mjs plays the tracked site, on its own origin.

import { defineConfig, devices } from "@playwright/test";
import { existsSync } from "node:fs";

const PORT = 3901;
const SITE_PORT = 3902;
const GOOGLE_PORT = 3903;
// Countries need the DB-IP file (`pnpm geoip`); without it they show Unknown.
const GEOIP = existsSync("server/data/geo.mmdb") ? "server/data/geo.mmdb" : "server/target/e2e-data/none.mmdb";

export default defineConfig({
  testDir: "e2e",
  fullyParallel: false,
  forbidOnly: !!process.env.CI,
  reporter: process.env.CI ? "github" : "list",
  use: {
    ...devices["Desktop Chrome"],
    // Headless Chromium says "HeadlessChrome", which the bot filter drops.
    userAgent:
      "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36",
    trace: "retain-on-failure",
    // site.test and elsewhere.test are two cross-origin "sites" on loopback.
    launchOptions: {
      args: ["--host-resolver-rules=MAP *.test 127.0.0.1"],
    },
  },
  webServer: [{
    // A fresh database per run, and a fresh frontend build. No X-Real-IP
    // without the ingress, so the server falls back to the peer address.
    command:
      "pnpm build:web && rm -rf server/target/e2e-data && cargo run --manifest-path server/Cargo.toml",
    url: `http://127.0.0.1:${PORT}/healthz`,
    env: {
      PORT: String(PORT),
      ANALYTICS_DATA_DIR: "server/target/e2e-data",
      ANALYTICS_GEOIP: GEOIP,
      ANALYTICS_WEB_DIR: "web/dist",
      // Search Console against e2e/google.mjs, never Google.
      ANALYTICS_GOOGLE: "1",
      ANALYTICS_GOOGLE_API: `http://127.0.0.1:${GOOGLE_PORT}`,
      ANALYTICS_GOOGLE_TOKEN_URL: `http://127.0.0.1:${GOOGLE_PORT}/token`,
    },
    timeout: 180 * 1000,
    reuseExistingServer: false,
    stderr: "pipe",
  }, {
    command: "node e2e/site.mjs",
    url: `http://127.0.0.1:${SITE_PORT}/healthz`,
    env: { SITE_PORT: String(SITE_PORT), ANALYTICS_URL: `http://127.0.0.1:${PORT}` },
    reuseExistingServer: false,
  }, {
    command: "node e2e/google.mjs",
    url: `http://127.0.0.1:${GOOGLE_PORT}/healthz`,
    env: { GOOGLE_PORT: String(GOOGLE_PORT) },
    reuseExistingServer: false,
  }],
});

export const SERVER = `http://127.0.0.1:${PORT}`;
export const SITE_PORT_NUMBER = SITE_PORT;
/** A service account file around a throwaway test key (server/src/testdata). */
export const SERVICE_ACCOUNT = "server/src/testdata/service-account.json";
