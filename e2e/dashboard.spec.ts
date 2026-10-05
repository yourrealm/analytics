// The React dashboard, as an owner would use it. The Realm gate would inject
// X-Analytics-User; here the owner's browser context sends it itself.

import { expect, test } from "@playwright/test";
import { SERVER, SITE_PORT_NUMBER } from "../playwright.config.ts";

test("add a site, get its snippet, see a visit on the dashboard", async ({ browser }) => {
  const owner = await browser.newContext({
    extraHTTPHeaders: { "X-Analytics-User": `e2e-${crypto.randomUUID()}` },
  });
  const app = await owner.newPage();

  // No sites yet: the overview points to the add form.
  await app.goto(SERVER);
  await expect(app.getByRole("heading", { name: "No sites yet" })).toBeVisible();
  await app.getByRole("link", { name: "Add a site" }).click();
  await expect(app).toHaveURL(`${SERVER}/sites?site=new`);

  await app.getByLabel("Site name").fill("Blog");
  await app.getByLabel("Allowed hostnames").fill("site.test");
  await app.getByRole("button", { name: "Create site" }).click();

  // The new site opens with its snippet, waiting for its first visit.
  await expect(app.getByRole("button", { name: /Blog/, pressed: true })).toBeVisible();
  await expect(app.getByText("Waiting for the first visit")).toBeVisible();
  const snippet = app.getByLabel("Tracking snippet");
  await expect(snippet).toContainText(`src="${SERVER}/script.js"`);
  const site = (await snippet.textContent())!.match(/data-entity="([a-z0-9]+)"/)![1]!;

  // A visitor, in a context without the owner's header.
  const visitor = await browser.newPage();
  const sent = visitor.waitForResponse((r) => r.url() === `${SERVER}/api/event`);
  await visitor.goto(`http://site.test:${SITE_PORT_NUMBER}/?site=${site}`, {
    referer: "https://news.ycombinator.com/",
  });
  expect((await sent).status()).toBe(204);

  // The Sites view sees it: the snippet works.
  await app.reload();
  await expect(app.getByText(/Last event just now/)).toBeVisible();

  // The overview has a card for it; the card opens the dashboard.
  await app.getByRole("link", { name: "Overview" }).click();
  const card = app.locator("article", { hasText: "Blog" });
  await expect(card).toContainText(/1\s*visitor in last 24h/);
  await card.getByRole("link", { name: "Blog", exact: true }).click();
  await expect(app).toHaveURL(new RegExp(`site=${site}`));
  await app.getByRole("tab", { name: "Today" }).click();
  await expect(app).toHaveURL(/period=today/);
  await expect(app.getByRole("button", { name: /Visitors\s*1/ })).toBeVisible();
  await expect(app.getByRole("button", { name: /Pageviews\s*1/ })).toBeVisible();
  await expect(app.locator("section", { hasText: "Referrers" })).toContainText("news.ycombinator.com");

  // The filter survives a reload: it lives in the URL.
  await app.reload();
  await expect(app.getByRole("tab", { name: "Today" })).toHaveAttribute("aria-selected", "true");

  await owner.close();
});

test("without the identity header the API refuses", async ({ request }) => {
  const res = await request.get(`${SERVER}/api/sites`);
  expect(res.status()).toBe(401);
});
