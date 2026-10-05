// Google Search Console: one service account key per user, set up on the
// Settings view (GoogleCard), and a property per site, picked on the Sites
// view (PropertyPicker).

import { type ReactNode, useState } from "react";
import { api, type Google, type Site } from "./api.ts";
import { navigate } from "./route.ts";
import { Button, Card, ErrorText } from "./ui.tsx";

function Ext({ href, children }: { href: string; children: ReactNode }) {
  return (
    <a href={href} target="_blank" rel="noopener" className="font-medium text-accent underline">
      {children}
    </a>
  );
}

/** Copies on click; the button says so for a moment. */
function CopyButton({ value }: { value: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <Button
      variant="outline"
      className="text-xs"
      onClick={async () => {
        await navigator.clipboard.writeText(value);
        setCopied(true);
        setTimeout(() => setCopied(false), 1500);
      }}
    >
      {copied ? "Copied" : "Copy"}
    </Button>
  );
}

// Linked straight so the steps don't depend on menus. The URLs and labels
// follow Google's docs as of 2026-10: docs.cloud.google.com/iam/docs/
// service-accounts-create and keys-create-delete, and Search Console Help
// answer 7687615. The API is `searchconsole.googleapis.com` (its discovery
// document).
const LINKS = {
  project: "https://console.cloud.google.com/projectcreate",
  api: "https://console.cloud.google.com/apis/enableflow?apiid=searchconsole.googleapis.com",
  createAccount: "https://console.cloud.google.com/projectselector/iam-admin/serviceaccounts/create",
  accounts: "https://console.cloud.google.com/projectselector/iam-admin/serviceaccounts",
  users: "https://search.google.com/search-console/users",
};

const STEPS: { title: ReactNode; body: ReactNode }[] = [
  {
    title: "Pick or create a Google Cloud project",
    body: (
      <>
        Any project works, and this costs nothing. Use one you already have, or{" "}
        <Ext href={LINKS.project}>create a Google Cloud project</Ext>.
      </>
    ),
  },
  {
    title: "Turn on the Search Console API",
    body: (
      <>
        Open the <Ext href={LINKS.api}>Google Search Console API</Ext>, pick your project if Google asks, then confirm
        to enable it.
      </>
    ),
  },
  {
    title: "Create a service account",
    body: (
      <>
        <Ext href={LINKS.createAccount}>Create a service account</Ext>: pick your project, give it any name (for example
        "analytics"), and click Done. It needs no roles or permissions, so skip Create and continue.
      </>
    ),
  },
  {
    title: "Download a key for it",
    body: (
      <>
        In the <Ext href={LINKS.accounts}>service accounts list</Ext>, click the account's email, open the Keys tab,
        and choose Add key → Create new key → JSON → Create. Your browser downloads a .json file once; keep it safe. If
        key creation is blocked, your Google Cloud organization forbids it (the default for organizations created since
        May 2024); a project with no organization, under a personal Google account, works.
      </>
    ),
  },
];

/** One collapsible step: a numbered header (a tick once done) over its body. */
function Step({ n, title, open, done, onToggle, children }: {
  n: number;
  title: ReactNode;
  open: boolean;
  done: boolean;
  onToggle: () => void;
  children: ReactNode;
}) {
  return (
    <li className="overflow-hidden rounded-xl border border-line">
      <button
        type="button"
        aria-expanded={open}
        onClick={onToggle}
        className="flex min-h-12 w-full cursor-pointer items-center gap-3 px-3.5 py-2.5 text-left text-sm hover:bg-accent-soft"
      >
        <span
          className={`flex size-6 shrink-0 items-center justify-center rounded-full text-xs font-semibold ${
            done ? "bg-ok text-white" : "bg-accent-soft"
          }`}
        >
          {done ? "✓" : n}
        </span>
        <span className="flex-1 font-medium">{title}</span>
        <span className="text-muted" aria-hidden>{open ? "▴" : "▾"}</span>
      </button>
      {open && <div className="flex flex-col gap-3 px-3.5 pb-3.5 pl-[50px] text-sm text-muted">{children}</div>}
    </li>
  );
}

export function GoogleCard({ google, properties, sites, onChange, onRefresh }: {
  google: Google | undefined;
  /** What the connected account can read; undefined while loading. */
  properties: string[] | undefined;
  /** The user's sites, to show which ones are linked. */
  sites: Site[];
  onChange: () => void;
  /** Reloads `properties`, after adding the account in Search Console. */
  onRefresh: () => void;
}) {
  const [key, setKey] = useState("");
  const [fileName, setFileName] = useState<string>();
  const [open, setOpen] = useState(0);
  const [done, setDone] = useState(0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();

  const run = async (f: () => Promise<unknown>) => {
    setBusy(true);
    setError(undefined);
    try {
      await f();
      setKey("");
      setFileName(undefined);
      onChange();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  if (!google) return null;
  if (!google.available) {
    return (
      <Card title="Google Search Console">
        <p className="text-sm text-muted">
          Search terms need internet access, which this app was not granted. An admin can grant it in Realm, on this
          app's permissions.
        </p>
      </Card>
    );
  }

  if (google.email) {
    const linked = sites.filter((s) => s.search_console);
    return (
      <Card
        title={
          <span className="flex items-center gap-2">
            Google Search Console
            <span className="flex items-center gap-1.5 text-xs font-normal text-muted">
              <span className="size-2 rounded-full bg-ok" /> Connected
            </span>
          </span>
        }
        action={
          <Button variant="ghost" disabled={busy} onClick={() => run(() => api("/api/google", { method: "DELETE" }))}>
            Disconnect
          </Button>
        }
      >
        <div className="flex flex-col gap-4 text-sm">
          <div className="flex flex-wrap items-center gap-2">
            <code className="break-all rounded-lg border border-line bg-bg px-2.5 py-1.5 font-mono text-xs">
              {google.email}
            </code>
            <CopyButton value={google.email} />
          </div>
          <div className="flex flex-col gap-1.5 rounded-xl border border-line px-4 py-3">
            <span className="font-semibold">
              {properties === undefined
                ? "Checking what this account can read…"
                : properties.length === 0
                ? "This account can't read any property yet"
                : `This account can read ${properties.length} ${properties.length === 1 ? "property" : "properties"}`}
            </span>
            <span className="text-muted">
              {linked.length
                ? `Linked: ${linked.map((s) => `${s.name} → ${s.search_console}`).join(", ")}.`
                : "No site is linked yet."} Pick a property on each site in{" "}
              <a
                href="/sites"
                onClick={(e) => (e.preventDefault(), navigate("/sites"))}
                className="text-accent underline"
              >
                Sites
              </a>.
            </span>
            <span className="text-muted">
              Missing one? Open <Ext href={LINKS.users}>Users and permissions</Ext>{" "}
              for the property in Search Console, click Add user, paste the address above and set Permission to
              Restricted. Only the property's owner can. Then{" "}
              <button type="button" onClick={onRefresh} className="cursor-pointer font-medium text-accent underline">
                check again
              </button>.
            </span>
          </div>
          <ErrorText error={error} />
        </div>
      </Card>
    );
  }

  const readFile = async (file: File | undefined) => {
    if (!file) return;
    setKey(await file.text());
    setFileName(file.name);
  };
  const toggle = (i: number) => setOpen(open === i ? -1 : i);

  return (
    <Card title="Google Search Console">
      <div className="flex flex-col gap-4 text-sm">
        <p className="text-muted">
          Show the Google searches that led to your sites. Google only shares them with an account you set up once, a
          service account, which then reads every property you add it to. It takes about five minutes.
        </p>
        <ol className="flex flex-col gap-1.5">
          {STEPS.map((s, i) => (
            <Step key={i} n={i + 1} title={s.title} open={open === i} done={i < done} onToggle={() => toggle(i)}>
              <span>{s.body}</span>
              <button
                type="button"
                onClick={() => (setOpen(i + 1), setDone(Math.max(done, i + 1)))}
                className="cursor-pointer self-start font-medium text-accent"
              >
                Done, next step
              </button>
            </Step>
          ))}
          <Step
            n={STEPS.length + 1}
            title="Choose that file here"
            open={open === STEPS.length}
            done={false}
            onToggle={() => toggle(STEPS.length)}
          >
            <label className="flex w-fit cursor-pointer items-center gap-2 rounded-lg border border-line px-3 py-1.5 font-medium text-fg hover:bg-accent-soft">
              <input
                type="file"
                accept="application/json,.json"
                aria-label="Service account key file"
                className="sr-only"
                onChange={(e) => readFile(e.target.files?.[0])}
              />
              {fileName ?? "Choose file…"}
            </label>
            <details>
              <summary className="cursor-pointer">Or paste its contents</summary>
              <textarea
                aria-label="Service account key"
                value={key}
                onChange={(e) => (setKey(e.target.value), setFileName(undefined))}
                rows={4}
                spellCheck={false}
                placeholder='{"type": "service_account", ...}'
                className="mt-2 w-full rounded-lg border border-line bg-bg p-3 font-mono text-xs text-fg outline-none placeholder:text-muted focus:border-accent"
              />
            </details>
            <div className="flex flex-wrap items-center gap-3">
              <Button
                disabled={busy || !key.trim()}
                onClick={() => run(() => api("/api/google", { method: "PUT", body: { key } }))}
              >
                {busy ? "Checking with Google…" : "Connect"}
              </Button>
              <span className="text-xs">
                The key is checked with Google, then stored encrypted. A brand-new account can take a minute before
                Google accepts it.
              </span>
            </div>
            <ErrorText error={error} />
          </Step>
        </ol>
      </div>
    </Card>
  );
}

export function PropertyPicker({ site, properties, onChange }: {
  site: Site;
  properties: string[] | undefined;
  onChange: () => void;
}) {
  const [error, setError] = useState<string>();
  const [busy, setBusy] = useState(false);
  const options = properties ?? [];
  const current = site.search_console ?? "";
  // A linked property the account lost access to still shows, so it can be unset.
  const all = current && !options.includes(current) ? [current, ...options] : options;

  const save = async (property: string) => {
    setBusy(true);
    setError(undefined);
    try {
      await api(`/api/sites/${site.id}/search-console`, {
        method: "PUT",
        body: { property: property || null },
      });
      onChange();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex flex-col gap-1.5">
      <label htmlFor={`gsc-${site.id}`} className="text-sm font-semibold">Search Console property</label>
      <select
        id={`gsc-${site.id}`}
        value={current}
        disabled={busy || !properties}
        onChange={(e) => save(e.target.value)}
        className="rounded-lg border border-line bg-bg px-3 py-1.5 text-sm"
      >
        <option value="">{properties ? "Not linked" : "Loading…"}</option>
        {all.map((p) => <option key={p} value={p}>{p}</option>)}
      </select>
      {properties && options.length === 0 && (
        <span className="text-xs text-muted">
          The service account can't read any property yet: see Settings.
        </span>
      )}
      <ErrorText error={error} />
    </div>
  );
}
