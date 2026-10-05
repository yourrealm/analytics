// The site list on the left, the picked site on the right (`?site=<id>`, or
// `?site=new` to add one): its snippet, hostnames, Search Console property and
// delete. On a phone the two stack. The snippet points at this page's own
// origin: the app serves the tracker next to the dashboard.

import { type FormEvent, useState } from "react";
import { api, type Google, type Site, type SiteActivity, useApi } from "./api.ts";
import { navigate, withQuery } from "./route.ts";
import { PropertyPicker } from "./SearchConsole.tsx";
import { Button, ErrorText, Input } from "./ui.tsx";

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

/** "just now", "5 min ago", "3 h ago", "2 days ago". */
function ago(seconds: number) {
  const d = Date.now() / 1000 - seconds;
  if (d < 60) return "just now";
  if (d < 3600) return `${Math.floor(d / 60)} min ago`;
  if (d < 86400) return `${Math.floor(d / 3600)} h ago`;
  const days = Math.floor(d / 86400);
  return days === 1 ? "yesterday" : `${days} days ago`;
}

function Status({ site }: { site: SiteActivity }) {
  return (
    <span className="flex items-center gap-1.5 text-sm text-muted">
      <Dot live={site.last_event !== null} />
      {site.last_event === null ? "Waiting for the first visit" : `Last event ${ago(site.last_event)}`}
    </span>
  );
}

function Dot({ live }: { live: boolean }) {
  return <span className={`size-2 shrink-0 rounded-full ${live ? "bg-ok" : "bg-wait"}`} />;
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
        className="whitespace-pre-wrap [overflow-wrap:anywhere] rounded-lg border border-line bg-bg p-3 pr-20 font-mono text-xs leading-relaxed"
      >
        {value}
      </pre>
      <Button variant="outline" className="absolute right-2 top-2 bg-card text-xs" onClick={copy}>
        {copied ? "Copied" : "Copy"}
      </Button>
    </div>
  );
}

function SiteList({ sites, selected }: { sites: SiteActivity[]; selected: string }) {
  return (
    <section className="flex min-w-0 flex-[1_1_260px] flex-col gap-1 rounded-2xl border border-line bg-card p-3">
      <div className="flex items-center gap-2 px-2 pb-2 pt-1">
        <h1 className="text-base font-semibold">Sites</h1>
        <Button className="ml-auto" onClick={() => navigate(withQuery({ site: "new" }))}>Add site</Button>
      </div>
      {sites.map((s) => (
        <button
          key={s.id}
          type="button"
          aria-pressed={s.id === selected}
          onClick={() => navigate(withQuery({ site: s.id }))}
          className={`flex min-h-12 cursor-pointer items-center gap-2.5 rounded-lg px-2.5 py-2 text-left text-sm hover:bg-accent-soft ${
            s.id === selected ? "bg-accent-soft font-semibold" : ""
          }`}
        >
          <Dot live={s.last_event !== null} />
          <span className="flex min-w-0 flex-1 flex-col">
            <span className="truncate">{s.name}</span>
            <span className="truncate text-xs font-normal text-muted">{s.hostnames[0] ?? "any host"}</span>
          </span>
        </button>
      ))}
    </section>
  );
}

function SiteDetail({ site, connected, properties, onChange }: {
  site: SiteActivity;
  /** Whether the user connected Search Console. */
  connected: boolean;
  /** The service account's properties; undefined while they load. */
  properties: string[] | undefined;
  onChange: () => void;
}) {
  const [hosts, setHosts] = useState(site.hostnames.join(", "));
  const [events, setEvents] = useState(false);
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
    <div className="flex flex-col gap-5">
      <div className="flex flex-wrap items-start gap-3">
        <div className="flex min-w-0 flex-col gap-1">
          <h2 className="text-lg font-semibold">{site.name}</h2>
          <Status site={site} />
        </div>
        <a
          href={`/?site=${site.id}`}
          onClick={(e) => (e.preventDefault(), navigate(`/?site=${site.id}`))}
          className="ml-auto rounded-lg border border-line px-3 py-1.5 text-sm hover:bg-accent-soft"
        >
          Open dashboard
        </a>
      </div>

      <div className="flex flex-col gap-2">
        <h3 className="text-sm font-semibold">Add to every page's &lt;head&gt;</h3>
        <Code label="Tracking snippet" value={code.tag} />
        <button
          type="button"
          aria-expanded={events}
          onClick={() => setEvents(!events)}
          className="cursor-pointer self-start py-1 text-sm text-accent"
        >
          {events ? "Hide custom events" : "Custom events"}
        </button>
        {events && (
          <>
            <Code label="Custom event example" value={code.custom} />
            <p className="text-xs text-muted">
              Visits from localhost are not counted. If your URLs hold something private, like an invite token, send
              events yourself with it removed: see the README.
            </p>
          </>
        )}
      </div>

      <div className="grid grid-cols-[repeat(auto-fit,minmax(220px,1fr))] gap-4">
        <form
          className="flex flex-col gap-1.5"
          onSubmit={(e) => {
            e.preventDefault();
            run(() => api(`/api/sites/${site.id}`, { method: "PUT", body: { name: site.name, hostnames: hostList(hosts) } }));
          }}
        >
          <label htmlFor={`hosts-${site.id}`} className="text-sm font-semibold">Allowed hostnames</label>
          <div className="flex gap-2">
            <Input
              id={`hosts-${site.id}`}
              className="flex-1"
              value={hosts}
              placeholder="example.com, *.example.com"
              onChange={(e) => setHosts(e.target.value)}
            />
            <Button variant="outline" disabled={busy}>Save</Button>
          </div>
          <span className="text-xs text-muted">Only pages on these hosts count. Empty accepts any.</span>
        </form>
        {connected ? <PropertyPicker site={site} properties={properties} onChange={onChange} /> : (
          <div className="flex flex-col gap-1.5">
            <span className="text-sm font-semibold">Search Console property</span>
            <span className="text-sm text-muted">
              Connect Google in{" "}
              <a
                href="/settings"
                onClick={(e) => (e.preventDefault(), navigate("/settings"))}
                className="text-accent underline"
              >
                Settings
              </a>{" "}
              to show search terms for this site.
            </span>
          </div>
        )}
      </div>

      <div className="flex flex-wrap items-center gap-3 rounded-xl border border-line px-4 py-3">
        <span className="flex flex-col gap-0.5">
          <span className="text-sm font-semibold">Delete site</span>
          <span className="text-xs text-muted">
            Removes {site.name} and all its events. Site id <code className="font-mono">{site.id}</code>
          </span>
        </span>
        {confirm
          ? (
            <span className="ml-auto flex gap-2">
              <Button
                variant="danger"
                disabled={busy}
                onClick={() =>
                  run(async () => {
                    await api(`/api/sites/${site.id}`, { method: "DELETE" });
                    navigate(withQuery({ site: "" }), true);
                  })}
              >
                Delete for good
              </Button>
              <Button variant="ghost" onClick={() => setConfirm(false)}>Cancel</Button>
            </span>
          )
          : <Button variant="outline" className="ml-auto text-danger" onClick={() => setConfirm(true)}>Delete…</Button>}
      </div>
      <ErrorText error={error} />
    </div>
  );
}

function AddSite({ first, onAdded }: { first: boolean; onAdded: () => void }) {
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
      const site = await api<Site>("/api/sites", {
        method: "POST",
        body: { name: name.trim(), hostnames: hostList(hosts) },
      });
      onAdded();
      navigate(withQuery({ site: site.id }), true);
    } catch (err) {
      setError((err as Error).message);
      setBusy(false);
    }
  };

  return (
    <form className="flex flex-col gap-4" onSubmit={submit}>
      <div className="flex flex-col gap-1">
        <h2 className="text-lg font-semibold">{first ? "Add your first site" : "Add a site"}</h2>
        <p className="text-sm text-muted">You get its snippet right after.</p>
      </div>
      <label className="flex flex-col gap-1.5">
        <span className="text-sm font-semibold">Name</span>
        <Input aria-label="Site name" placeholder="e.g. Blog" value={name} onChange={(e) => setName(e.target.value)} />
      </label>
      <label className="flex flex-col gap-1.5">
        <span className="text-sm font-semibold">Allowed hostnames</span>
        <Input placeholder="example.com, *.example.com" value={hosts} onChange={(e) => setHosts(e.target.value)} />
        <span className="text-xs text-muted">Only pages on these hosts count. Empty accepts any.</span>
      </label>
      <div className="flex gap-2">
        <Button disabled={busy || !name.trim()}>Create site</Button>
        {!first && (
          <Button type="button" variant="ghost" onClick={() => navigate(withQuery({ site: "" }), true)}>
            Cancel
          </Button>
        )}
      </div>
      <ErrorText error={error} />
    </form>
  );
}

export function Sites({ query }: { query: URLSearchParams }) {
  const sites = useApi<SiteActivity[]>("/api/sites", 60_000);
  const google = useApi<Google>("/api/google");
  const connected = !!google.data?.email;
  const properties = useApi<string[]>(connected ? "/api/google/properties" : null);

  const list = sites.data ?? [];
  const wanted = query.get("site");
  const adding = wanted === "new" || (sites.data !== undefined && list.length === 0);
  const site = adding ? undefined : list.find((s) => s.id === wanted) ?? list[0];

  return (
    <div className="flex flex-col gap-3">
      <ErrorText error={sites.error ?? properties.error} />
      <div className="flex flex-wrap items-start gap-4">
        <SiteList sites={list} selected={adding ? "" : site?.id ?? ""} />
        <section className="min-w-0 flex-[999_1_560px] rounded-2xl border border-line bg-card p-6">
          {adding
            ? <AddSite first={list.length === 0} onAdded={sites.reload} />
            : site
            ? (
              <SiteDetail
                key={site.id}
                site={site}
                connected={connected}
                properties={properties.data ?? (properties.error ? [] : undefined)}
                onChange={sites.reload}
              />
            )
            : <p className="text-sm text-muted">Loading…</p>}
        </section>
      </div>
    </div>
  );
}
