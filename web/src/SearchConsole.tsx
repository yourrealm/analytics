// Google Search Console on the Sites view: one service account key per user
// (GoogleCard), and a property per site (PropertyPicker).

import { type ReactNode, useState } from "react";
import { api, type Google, type Site } from "./api.ts";
import { Button, Card, ErrorText } from "./ui.tsx";

function Ext({ href, children }: { href: string; children: ReactNode }) {
  return (
    <a href={href} target="_blank" rel="noopener" className="font-medium text-accent underline">
      {children}
    </a>
  );
}

function Step({ n, title, children }: { n: number; title: ReactNode; children?: ReactNode }) {
  return (
    <li className="flex gap-3">
      <span className="flex size-6 shrink-0 items-center justify-center rounded-full bg-accent-soft text-xs font-semibold">
        {n}
      </span>
      <div className="flex min-w-0 flex-1 flex-col gap-1 pt-0.5">
        <span className="font-medium">{title}</span>
        {children && <span className="text-muted">{children}</span>}
      </div>
    </li>
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

export function GoogleCard({ google, properties, onChange, onRefresh }: {
  google: Google | undefined;
  /** What the connected account can read; undefined while loading. */
  properties: string[] | undefined;
  onChange: () => void;
  /** Reloads `properties`, after adding the account in Search Console. */
  onRefresh: () => void;
}) {
  const [key, setKey] = useState("");
  const [fileName, setFileName] = useState<string>();
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
    return (
      <Card
        title="Google Search Console"
        action={
          <Button variant="ghost" disabled={busy} onClick={() => run(() => api("/api/google", { method: "DELETE" }))}>
            Disconnect
          </Button>
        }
      >
        <div className="flex flex-col gap-4 text-sm">
          <div className="flex flex-wrap items-center gap-2">
            <span>Connected as</span>
            <code className="rounded bg-bg px-1.5 py-0.5 font-mono text-xs">{google.email}</code>
            <CopyButton value={google.email} />
          </div>
          <div className="flex flex-col gap-3">
            <span className="text-muted">
              For each site, let this account read its Search Console property:
            </span>
            <ol className="flex flex-col gap-3">
              <Step
                n={1}
                title={<>Open <Ext href={LINKS.users}>Users and permissions</Ext> in Search Console</>}
              >
                Pick the property at the top. The page is under Settings, and only the property's owner sees it.
              </Step>
              <Step n={2} title="Click Add user">
                Paste the address above, set Permission to Restricted (view only), and add it.
              </Step>
              <Step n={3} title="Come back and pick the property on the site above">
                {properties === undefined
                  ? "Loading what this account can read…"
                  : properties.length === 0
                  ? "This account can't read any property yet."
                  : `This account can read ${properties.length} ${properties.length === 1 ? "property" : "properties"}.`}
                {" "}
                <button type="button" onClick={onRefresh} className="cursor-pointer font-medium text-accent underline">
                  Check again
                </button>
              </Step>
            </ol>
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

  return (
    <Card title="Google Search Console">
      <div className="flex flex-col gap-4 text-sm">
        <p className="text-muted">
          Show the Google searches that led to your sites. Google only shares them with an account you set up once, a
          service account, which then reads every property you add it to. It takes about five minutes.
        </p>
        <ol className="flex flex-col gap-3">
          <Step n={1} title={<>Pick or <Ext href={LINKS.project}>create a Google Cloud project</Ext></>}>
            Any project works, and this costs nothing. Use one you already have if you like.
          </Step>
          <Step n={2} title={<>Turn on the <Ext href={LINKS.api}>Google Search Console API</Ext></>}>
            Pick your project if Google asks, then confirm to enable it.
          </Step>
          <Step n={3} title={<><Ext href={LINKS.createAccount}>Create a service account</Ext></>}>
            Pick your project, give the account any name (for example "analytics"), and click Done. It needs no
            roles or permissions, so skip Create and continue.
          </Step>
          <Step n={4} title="Download a key for it">
            In the <Ext href={LINKS.accounts}>service accounts list</Ext>, click the account's email, open the Keys
            tab, and choose Add key → Create new key → JSON → Create. Your browser downloads a .json file once; keep
            it safe. If key creation is blocked, your Google Cloud organization forbids it (the default for
            organizations created since May 2024); a project with no organization, under a personal Google account,
            works.
          </Step>
          <Step n={5} title="Choose that file here">
            <span className="mt-1 flex flex-col gap-2">
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
            </span>
          </Step>
        </ol>
        <div className="flex items-center gap-3">
          <Button
            disabled={busy || !key.trim()}
            onClick={() => run(() => api("/api/google", { method: "PUT", body: { key } }))}
          >
            {busy ? "Checking with Google…" : "Connect"}
          </Button>
          <span className="text-xs text-muted">
            The key is checked with Google, then stored encrypted. A brand-new account can take a minute before Google
            accepts it.
          </span>
        </div>
        <ErrorText error={error} />
        <p className="text-xs text-muted">
          After connecting, you add the account's address to each property in Search Console. The next step shows how.
        </p>
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
      <label htmlFor={`gsc-${site.id}`} className="text-sm font-medium">Search Console property</label>
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
          The service account can't read any property yet: see Google Search Console below.
        </span>
      )}
      <ErrorText error={error} />
    </div>
  );
}
