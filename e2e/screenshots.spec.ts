// Screenshots of every view with a believable month of traffic, written to
// e2e/screenshots/ (gitignored; CI uploads them as an artifact). Not an
// assertion suite: it exists so a person can look at the UI.
//
// Events go in through the real ingest API, with a fake visitor IP per
// request (X-Real-IP, as the ingress would send). The API stamps everything
// "now", so the spec then spreads the rows over 30 days in SQLite directly.

import { type BrowserContextOptions, expect, test } from "@playwright/test";
import { DatabaseSync } from "node:sqlite";
import { readFileSync } from "node:fs";
import { SERVER, SERVICE_ACCOUNT } from "../playwright.config.ts";

const OUT = "e2e/screenshots";
const DB = "server/target/e2e-data/analytics.db";
// Each run starts on a fresh database, so a fixed name is safe.
const USER = "demo";
const owner = { "X-Analytics-User": USER };

const PAGES = ["/", "/", "/", "/", "/posts/realm-apps", "/posts/realm-apps", "/posts/rust-footprint", "/about", "/projects", "/posts/cookieless-analytics"];
const REFERRERS = [undefined, undefined, undefined, "https://news.ycombinator.com/", "https://news.ycombinator.com/", "https://www.google.com/", "https://bsky.app/", "https://lobste.rs/", "https://github.com/"];
// Addresses in a spread of countries; the last octet varies per visitor.
const NETWORKS = ["8.8.8", "194.132.0", "81.2.69", "1.1.1", "91.198.174", "133.11.0", "200.160.2", "41.0.0", "62.4.1", "5.255.255", "151.101.1"];
const SIZES = ["xs", "xs", "sm", "md", "lg", "xl", "xl", "2xl"];

// Deterministic, so the screenshots only change when the UI does.
let seed = 7;
const rand = () => ((seed = (seed * 16807) % 2147483647) - 1) / 2147483646;
const pick = <T>(xs: T[]) => xs[Math.floor(rand() * xs.length)]!;

test.beforeAll(async ({ request }) => {
  const created = await request.post(`${SERVER}/api/sites`, {
    headers: owner,
    data: { name: "blog.example", hostnames: ["blog.example"] },
  });
  const site = (await created.json()).id as string;
  await request.post(`${SERVER}/api/sites`, { headers: owner, data: { name: "shop.example", hostnames: [] } });
  await request.put(`${SERVER}/api/settings`, { headers: owner, data: { timezone: "Europe/Stockholm" } });
  // Search Console against e2e/google.mjs, so the Search terms card shows.
  await request.put(`${SERVER}/api/google`, { headers: owner, data: { key: readFileSync(SERVICE_ACCOUNT, "utf8") } });
  await request.put(`${SERVER}/api/sites/${site}/search-console`, {
    headers: owner,
    data: { property: "sc-domain:blog.example" },
  });

  for (let i = 0; i < 260; i++) {
    const custom = rand() < 0.12;
    const body = custom
      ? {
        entity_id: site,
        name: pick(["signup", "signup", "download", "newsletter"]),
        url: "https://blog.example/",
        properties: { plan: pick(["free", "free", "pro"]) },
      }
      : {
        entity_id: site,
        name: "pageview",
        url: `https://blog.example${pick(PAGES)}${rand() < 0.08 ? "?utm_source=newsletter" : ""}`,
        referrer: pick(REFERRERS),
        screen_width: pick(SIZES),
      };
    const res = await request.post(`${SERVER}/api/event`, {
      headers: {
        "content-type": "text/plain;charset=UTF-8",
        "user-agent": `Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_${Math.floor(rand() * 9)}) Chrome/140.0`,
        "x-real-ip": `${pick(NETWORKS)}.${1 + Math.floor(rand() * 250)}`,
      },
      data: JSON.stringify(body),
    });
    expect(res.status()).toBe(204);
  }

  // Spread this site's events over the last 30 days, more of them recent.
  const db = new DatabaseSync(DB);
  // The server and the other specs write meanwhile: wait for the lock.
  db.exec("PRAGMA busy_timeout = 5000");
  const now = Math.floor(Date.now() / 1000);
  const update = db.prepare("UPDATE events SET ts = ? WHERE id = ?");
  for (const row of db.prepare("SELECT id FROM events WHERE site_id = ?").all(site)) {
    const daysAgo = Math.min(29, Math.floor(-Math.log(1 - rand()) * 9));
    update.run(now - daysAgo * 86400 - Math.floor(rand() * 40000), row.id as number);
  }
  db.close();
});

const shoot = (name: string, options: BrowserContextOptions, path: string) =>
  test(name, async ({ browser }) => {
    const ctx = await browser.newContext({ ...options, extraHTTPHeaders: owner });
    const page = await ctx.newPage();
    await page.goto(`${SERVER}${path}`);
    // Data has arrived and the chart has drawn.
    await expect(page.getByRole("heading", { name: "No sites yet" })).toHaveCount(0);
    await page.waitForLoadState("networkidle");
    await page.screenshot({ path: `${OUT}/${name}.png`, fullPage: true });
    await ctx.close();
  });

const desktop = { viewport: { width: 1280, height: 900 } };
const phone = { viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true };

shoot("dashboard-light", { ...desktop, colorScheme: "light" }, "/?event=signup");
shoot("dashboard-dark", { ...desktop, colorScheme: "dark" }, "/?event=signup");
shoot("dashboard-7d-pageviews", { ...desktop, colorScheme: "light" }, "/?period=7d&metric=pageviews");
shoot("dashboard-phone", { ...phone, colorScheme: "light" }, "/");
shoot("sites-light", { ...desktop, colorScheme: "light" }, "/sites");
shoot("sites-dark", { ...desktop, colorScheme: "dark" }, "/sites");
shoot("sites-phone", { ...phone, colorScheme: "light" }, "/sites");
shoot("settings-light", { ...desktop, colorScheme: "light" }, "/settings");

// Search Console before connecting: the setup steps, the first one open.
test("settings-search-console-setup", async ({ browser }) => {
  const ctx = await browser.newContext({ ...desktop, extraHTTPHeaders: { "X-Analytics-User": "newcomer" } });
  await ctx.request.post(`${SERVER}/api/sites`, { data: { name: "blog.example", hostnames: ["blog.example"] } });
  const page = await ctx.newPage();
  await page.goto(`${SERVER}/settings`);
  await expect(page.getByRole("button", { name: "Done, next step" })).toBeVisible();
  await page.screenshot({ path: `${OUT}/settings-search-console-setup.png`, fullPage: true });
  await ctx.close();
});
