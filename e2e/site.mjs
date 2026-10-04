// The "tracked site" for e2e: every path serves a page that loads the tracker
// for the site ID in `?site=`. Chromium resolves *.test to 127.0.0.1 (see
// playwright.config.ts), so the page and the server share the loopback
// address space and Private Network Access stays out of the way.

import { createServer } from "node:http";

const port = Number(process.env.SITE_PORT ?? 3902);
const server = process.env.ANALYTICS_URL ?? "http://127.0.0.1:3901";

createServer((req, res) => {
  const url = new URL(req.url ?? "/", "http://localhost");
  if (url.pathname === "/healthz") return res.end("ok");
  const site = url.searchParams.get("site") ?? "";
  res.setHeader("content-type", "text/html; charset=utf-8");
  res.end(`<!doctype html><html><head><title>${url.pathname}</title>
<script type="module" src="${server}/script.js" data-entity="${site}" data-exit="false"></script>
<script type="module">
  import { event } from "${server}/script.js";
  document.querySelector("#signup").addEventListener("click", () =>
    event("signup", { properties: { plan: "pro", seats: 3 } }));
</script></head>
<body><h1>${url.pathname}</h1><button id="signup">Sign up</button>
<a href="/about?site=${site}">About</a></body></html>`);
}).listen(port, "127.0.0.1");
