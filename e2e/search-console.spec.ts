// Search Console as an owner sets it up: choose the service account key in
// Settings, pick a property for a site in Sites, see search terms on the
// dashboard. Google is e2e/google.mjs.

import { expect, type Page, test } from "@playwright/test";
import { SERVER, SERVICE_ACCOUNT } from "../playwright.config.ts";

/** Through the overview to Blog's dashboard. */
async function openDashboard(app: Page) {
  await app.getByRole("link", { name: "Overview" }).click();
  await app.getByRole("link", { name: "Blog", exact: true }).click();
  await expect(app.getByRole("tab", { name: "30 days" })).toBeVisible();
}

test("connect a service account, link a property, see search terms", async ({ browser }) => {
  const headers = { "X-Analytics-User": `e2e-${crypto.randomUUID()}` };
  const owner = await browser.newContext({ extraHTTPHeaders: headers });
  const app = await owner.newPage();
  await owner.request.post(`${SERVER}/api/sites`, { data: { name: "Blog", hostnames: [] } });

  // Not connected: the site points to Settings instead of a picker.
  await app.goto(`${SERVER}/sites`);
  await expect(app.getByLabel("Search Console property")).toHaveCount(0);
  await app.getByRole("link", { name: "Settings" }).last().click();
  await expect(app).toHaveURL(`${SERVER}/settings`);

  // The guide's steps open one at a time and link straight to Google's pages.
  await app.getByRole("button", { name: /Turn on the Search Console API/ }).click();
  await expect(app.getByRole("link", { name: "Google Search Console API" })).toHaveAttribute(
    "href",
    /apiid=searchconsole\.googleapis\.com/,
  );
  await app.getByRole("button", { name: "Done, next step" }).click();
  await expect(app.getByRole("link", { name: "Create a service account" })).toBeVisible();
  await app.getByRole("button", { name: /Choose that file here/ }).click();

  // A file that is not a service account key is refused with a reason.
  const file = app.getByLabel("Service account key file");
  await file.setInputFiles({ name: "notes.json", mimeType: "application/json", buffer: Buffer.from("{}") });
  await app.getByRole("button", { name: "Connect" }).click();
  await expect(app.getByText(/not a service account key/)).toBeVisible();

  // The downloaded key file, as a user would choose it.
  await file.setInputFiles(SERVICE_ACCOUNT);
  await expect(app.getByText("service-account.json")).toBeVisible();
  await app.getByRole("button", { name: "Connect" }).click();
  await expect(app.getByText("analytics@analytics-test.iam.gserviceaccount.com")).toBeVisible();
  await expect(app.getByText("This account can read 2 properties")).toBeVisible();
  await expect(app.getByText("No site is linked yet.", { exact: false })).toBeVisible();

  await app.getByRole("link", { name: "Sites" }).first().click();
  const picker = app.getByLabel("Search Console property");
  await expect(picker.locator("option")).toHaveText(["Not linked", "https://shop.example/", "sc-domain:blog.example"]);
  await picker.selectOption("sc-domain:blog.example");
  await expect(picker).toHaveValue("sc-domain:blog.example");

  await openDashboard(app);
  const card = app.locator("section", { hasText: "Search terms" });
  await expect(card).toContainText("realm self hosted");
  await expect(card).toContainText("99 clicks from 3,580 impressions");
  await expect(card.getByRole("link", { name: "Google Search Console" })).toHaveAttribute(
    "href",
    /resource_id=sc-domain%3Ablog\.example/,
  );

  // Settings lists the link. Disconnecting unlinks the site, and the card goes away.
  await app.getByRole("link", { name: "Settings" }).click();
  await expect(app.getByText("Linked: Blog → sc-domain:blog.example.", { exact: false })).toBeVisible();
  await app.getByRole("button", { name: "Disconnect" }).click();
  await expect(app.getByRole("button", { name: /Choose that file here/ })).toBeVisible();
  await openDashboard(app);
  await expect(app.locator("section", { hasText: "Search terms" })).toHaveCount(0);
  await owner.close();
});
