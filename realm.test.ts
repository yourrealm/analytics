import { assert, assertEquals } from "@std/assert";
import { type ConfigOf, type GrantsOf, Realm } from "@yourrealm/sdk/test";
import analytics from "./realm.tsx";

const config: ConfigOf<typeof analytics> = {};
const grants: GrantsOf<typeof analytics> = {};

Realm.describe(analytics, import.meta.resolve("./realm.tsx"), ({ test }) => {
  test("brokers the Home username as X-Analytics-User", async (t) => {
    const result = await t.compile({ config, grants });
    assert(
      !result.error,
      `expected success, got: ${JSON.stringify(result, null, 2)}`,
    );
    const trusted = result.app.trustedHeaders!;
    assertEquals(trusted.keys, ["X-Analytics-User"]);
    assertEquals(trusted.byRole.admin!["X-Analytics-User"], {
      field: "username",
    });
    assertEquals(trusted.byRole.member!["X-Analytics-User"], {
      field: "username",
    });
  });

  test("api service: image, sqlite backup, exec-form healthcheck, rate limit", async (t) => {
    const result = await t.compile({ config, grants });
    assert(!result.error);
    const api = result.app.services.api!;
    assertEquals(api.image, "ghcr.io/yourrealm/analytics:latest");
    assertEquals(api.router!.containerPort, 3000);
    assertEquals(api.router!.rateLimit, { requests: 600, window: "10s" });
    assertEquals(api.volumes![0]!.backup, {
      kind: "sqlite",
      file: "analytics.db",
    });
    // No shell in the distroless runtime: CMD, never CMD-SHELL.
    assertEquals(api.healthcheck!.test[0], "CMD");
  });

  test("Search Console needs the optional egress grant", async (t) => {
    const without = await t.compile({ config, grants });
    assert(!without.error);
    assertEquals(without.app.egress, undefined);
    assertEquals(without.app.services.api!.env?.ANALYTICS_GOOGLE, undefined);

    const granted: GrantsOf<typeof analytics> = {
      egress: { kind: "host.egress", data: true },
    };
    const withEgress = await t.compile({ config, grants: granted });
    assert(!withEgress.error);
    assertEquals(withEgress.app.egress, true);
    assertEquals(withEgress.app.services.api!.env?.ANALYTICS_GOOGLE, "1");
    // The sealing secret is a derived ref, resolved by Home, never a literal.
    assert(withEgress.app.services.api!.env?.ANALYTICS_SECRET !== undefined);
  });

  test("the dashboard is the app itself: a tile, no Realm surfaces", async (t) => {
    const result = await t.compile({ config, grants });
    assert(!result.error);
    assertEquals(result.app.services.api!.router!.tile, true);
    assertEquals(result.app.ui, undefined);
  });
});

Deno.test("declares an anonymous gate", () => {
  assertEquals(analytics.evaluate({ config }).hasAnonymousGate, true);
});

Deno.test("the gate admits only the tracker script and the event endpoint", () => {
  const gate = analytics.verifyAnonymousRequest!;
  const base = "https://analytics-api.w1.realm.test";
  const admits = (method: string, path: string) =>
    gate(new Request(base + path, { method }), () => "") === true;

  assert(admits("POST", "/api/event"));
  assert(admits("OPTIONS", "/api/event"));
  assert(admits("GET", "/script.js"));
  assert(admits("HEAD", "/script.js"));

  assert(!admits("GET", "/api/event"));
  assert(!admits("POST", "/script.js"));
  assert(!admits("GET", "/api/sites"));
  assert(!admits("GET", "/api/sites/abc/stats"));
  assert(!admits("POST", "/api/event/../sites"));
  assert(!admits("GET", "/api/me"));
  assert(!admits("GET", "/"));
});
