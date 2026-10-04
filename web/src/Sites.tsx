// Add a site, get its snippet, limit it to its hosts, delete it, and set the
// time zone the dashboard counts days in. The snippet points at this page's
// own origin: the app serves the tracker next to the dashboard.

import { type FormEvent, useState } from "react";
import { api, type Google, type Me, type Site, useApi } from "./api.ts";
import { GoogleCard, PropertyPicker } from "./SearchConsole.tsx";
import { Button, Card, ErrorText, Input } from "./ui.tsx";

/** "a.com, *.b.com" or one per line, into a list. */
const hostList = (text: string) => text.split(/[\s,]+/).map((h) => h.trim()).filter(Boolean);

function snippets(s: Site) {
  const origin = location.origin;
  const tag = `<script type="module" src="${origin}/script.js" data-entity="${s.id}" data-exit="false"></script>`;
  const custom = [
    `<script type="module">`,
    `  import { event } from "${origin}/script.js";`,
    `  document.querySelector("#signup")`,
    `    .addEventListener("click", () => event("signup", { properties: { plan: "pro" } }));`,
    `</script>`,
  ].join("\n");
  return { tag, custom };
}

function Code({ value, label }: { value: string; label: string }) {
  const [copied, setCopied] = useState(false);
  const copy = async () => {
    await navigator.clipboard.writeText(value);
    setCopied(true);
    setTimeout(() => setCopied(false), 1500);
  };
  return (
    <div className="relative">
      <pre
        aria-label={label}
        className="overflow-x-auto rounded-lg border border-line bg-bg p-3 pr-20 font-mono text-xs leading-relaxed"
      >
        {value}
      </pre>
      <Button variant="outline" className="absolute right-2 top-2 bg-card text-xs" onClick={copy}>
        {copied ? "Copied" : "Copy"}
      </Button>
    </div>
  );
}

function SiteCard({ site, connected, properties, onChange }: {
  site: Site;
  /** Whether the user connected Search Console. */
  connected: boolean;
  /** The service account's properties; undefined while they load. */
  properties: string[] | undefined;
  onChange: () => void;
}) {
  const [hosts, setHosts] = useState(site.hostnames.join(", "));
  const [confirm, setConfirm] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const code = snippets(site);

  const run = async (f: () => Promise<unknown>) => {
    setBusy(true);
    setError(undefined);
    try {
      await f();
      onChange();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Card
      title={site.name}
      action={<span className="font-mono text-xs text-muted">{site.id}</span>}
    >
      <div className="flex flex-col gap-4">
        <div className="flex flex-col gap-1.5">
          <span className="text-sm font-medium">Add to every page's &lt;head&gt;</span>
          <Code label="Tracking snippet" value={code.tag} />
        </div>
        <div className="flex flex-col gap-1.5">
          <span className="text-sm font-medium">Custom events (optional)</span>
          <Code label="Custom event example" value={code.custom} />
          <span className="text-xs text-muted">Visits from localhost are not counted.</span>
        </div>
        <form
          className="flex flex-col gap-1.5"
          onSubmit={(e) => {
            e.preventDefault();
            run(() =>
              api(`/api/sites/${site.id}`, {
                method: "PUT",
                body: { name: site.name, hostnames: hostList(hosts) },
              })
            );
          }}
        >
          <label htmlFor={`hosts-${site.id}`} className="text-sm font-medium">Allowed hostnames</label>
          <div className="flex gap-2">
            <Input
              id={`hosts-${site.id}`}
              className="flex-1"
              value={hosts}
              placeholder="example.com, *.example.com (empty allows any)"
              onChange={(e) => setHosts(e.target.value)}
            />
            <Button variant="outline" disabled={busy}>Save</Button>
          </div>
        </form>
        {connected && <PropertyPicker site={site} properties={properties} onChange={onChange} />}
        <div className="flex items-center gap-2">
          {confirm
            ? (
              <>
                <span className="text-sm">Delete {site.name} and all its data?</span>
                <Button
                  variant="danger"
                  disabled={busy}
                  onClick={() => run(() => api(`/api/sites/${site.id}`, { method: "DELETE" }))}
                >
                  Delete
                </Button>
                <Button variant="ghost" onClick={() => setConfirm(false)}>Cancel</Button>
              </>
            )
            : <Button variant="ghost" onClick={() => setConfirm(true)}>Delete site</Button>}
        </div>
        <ErrorText error={error} />
      </div>
    </Card>
  );
}

function AddSite({ onAdded }: { onAdded: () => void }) {
  const [name, setName] = useState("");
  const [hosts, setHosts] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!name.trim()) return;
    setBusy(true);
    setError(undefined);
    try {
      await api("/api/sites", { method: "POST", body: { name: name.trim(), hostnames: hostList(hosts) } });
      setName("");
      setHosts("");
      onAdded();
    } catch (err) {
      setError((err as Error).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Card title="Add a site">
      <p className="mb-3 text-sm text-muted">
        Hostnames limit which pages may count for the site. Leave them empty to accept any.
      </p>
      <form className="flex flex-col gap-2 sm:flex-row" onSubmit={submit}>
        <Input
          aria-label="Site name"
          placeholder="Name, e.g. Blog"
          value={name}
          onChange={(e) => setName(e.target.value)}
        />
        <Input
          aria-label="Allowed hostnames"
          className="flex-1"
          placeholder="example.com, *.example.com"
          value={hosts}
          onChange={(e) => setHosts(e.target.value)}
        />
        <Button disabled={busy}>Add site</Button>
      </form>
      <div className="mt-2">
        <ErrorText error={error} />
      </div>
    </Card>
  );
}

function TimeZone({ me, onChange }: { me: Me | undefined; onChange: () => void }) {
  const browser = Intl.DateTimeFormat().resolvedOptions().timeZone;
  const [error, setError] = useState<string>();
  const use = async () => {
    setError(undefined);
    try {
      await api("/api/settings", { method: "PUT", body: { timezone: browser } });
      onChange();
    } catch (e) {
      setError((e as Error).message);
    }
  };
  return (
    <Card title="Time zone">
      <p className="mb-3 text-sm text-muted">Days and hours on the dashboard start in this zone.</p>
      <div className="flex flex-wrap items-center gap-3">
        <span className="font-mono text-sm">{me?.timezone ?? "…"}</span>
        {me && me.timezone !== browser && <Button variant="outline" onClick={use}>Use {browser}</Button>}
      </div>
      <ErrorText error={error} />
    </Card>
  );
}

export function Sites({ me, reloadMe }: { me: Me | undefined; reloadMe: () => void }) {
  const sites = useApi<Site[]>("/api/sites");
  const google = useApi<Google>("/api/google");
  const connected = !!google.data?.email;
  const properties = useApi<string[]>(connected ? "/api/google/properties" : null);
  return (
    <div className="flex flex-col gap-4">
      <h1 className="text-lg font-semibold">Sites</h1>
      <ErrorText error={sites.error} />
      {sites.data?.map((s) => (
        <SiteCard
          key={s.id}
          site={s}
          connected={connected}
          properties={properties.data ?? (properties.error ? [] : undefined)}
          onChange={sites.reload}
        />
      ))}
      <AddSite onAdded={sites.reload} />
      <GoogleCard
        google={google.data}
        properties={properties.data}
        onChange={() => {
          google.reload();
          sites.reload();
        }}
        onRefresh={properties.reload}
      />
      <ErrorText error={properties.error} />
      <TimeZone me={me} onChange={reloadMe} />
    </div>
  );
}
