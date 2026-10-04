// The tracker on a cross-origin page (served by e2e/site.mjs): pageviews, a custom event with
// properties, and a page on a host the site does not allow.

import { expect, type Page, test } from "@playwright/test";
import { SERVER, SITE_PORT_NUMBER } from "../playwright.config.ts";

// Each test gets its own user, so runs never see each other's sites.
const owner = () => ({ "X-Analytics-User": `e2e-${crypto.randomUUID()}` });

/** A page of the fake site at `host`, tracked for `site`. */
const at = (host: string, site: string) => `http://${host}:${SITE_PORT_NUMBER}/?site=${site}`;

const tracked = (page: Page) =>
  page.waitForResponse((r) => r.url() === `${SERVER}/api/event` && r.request().method() === "POST");

test("pageviews and a custom event reach the dashboard API", async ({ page, request }) => {
  const headers = owner();
  const created = await request.post(`${SERVER}/api/sites`, {
    headers,
    data: { name: "E2E", hostnames: ["site.test"] },
  });
  expect(created.status()).toBe(201);
  const site = (await created.json()).id as string;

  // First view arrives from Hacker News.
  let sent = tracked(page);
  await page.goto(at("site.test", site), { referer: "https://news.ycombinator.com/" });
  expect((await sent).status()).toBe(204);

  // A click within the site: a second pageview, not a referral.
  sent = tracked(page);
  await page.getByRole("link", { name: "About" }).click();
  expect((await sent).status()).toBe(204);

  sent = tracked(page);
  await page.getByRole("button", { name: "Sign up" }).click();
  expect((await sent).status()).toBe(204);

  const stats = await (await request.get(`${SERVER}/api/sites/${site}/stats?period=today`, { headers })).json();
  expect(stats.totals).toEqual({ visitors: 1, pageviews: 2, events: 1 });
  expect(stats.pages.map((p: { key: string }) => p.key).sort()).toEqual(["/", "/about"]);
  expect(stats.referrers).toContainEqual({ key: "news.ycombinator.com", visitors: 1, pageviews: 1 });
  expect(stats.devices[0].key).toBe("laptop");
  expect(stats.events).toEqual([{ name: "signup", count: 1, visitors: 1 }]);

  const props = await (await request.get(
    `${SERVER}/api/sites/${site}/props?period=today&event=signup`,
    { headers },
  )).json();
  expect(props).toEqual([
    { key: "plan", value: "pro", count: 1, visitors: 1 },
    { key: "seats", value: "3", count: 1, visitors: 1 },
  ]);
});

test("a host the site does not allow is not counted", async ({ page, request }) => {
  const headers = owner();
  const created = await request.post(`${SERVER}/api/sites`, {
    headers,
    data: { name: "Strict", hostnames: ["site.test"] },
  });
  const site = (await created.json()).id as string;

  const sent = tracked(page);
  await page.goto(at("elsewhere.test", site));
  // Dropped silently: the page never learns why.
  expect((await sent).status()).toBe(204);

  const stats = await (await request.get(`${SERVER}/api/sites/${site}/stats?period=today`, { headers })).json();
  expect(stats.totals.pageviews).toBe(0);
});
