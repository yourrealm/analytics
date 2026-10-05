// Every site at a glance, as on Plausible's home: a card per site with its
// last 24 hours. A card opens that site's dashboard (`/?site=<id>`).

import { useEffect, useId, useRef, useState } from "react";
import { type Glance, useApi } from "./api.ts";
import { navigate } from "./route.ts";
import { Card, ErrorText, fmt } from "./ui.tsx";

/** Visitors per hour as a filled line, scaled to the busiest hour. */
function Sparkline({ values }: { values: number[] }) {
  const id = useId();
  const max = Math.max(1, ...values);
  const step = 100 / Math.max(1, values.length - 1);
  const points = values.map((v, i) => `${i * step},${29 - (v / max) * 27}`).join(" ");
  return (
    <svg viewBox="0 0 100 30" preserveAspectRatio="none" className="h-16 w-full" aria-hidden>
      <defs>
        <linearGradient id={id} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="var(--accent)" stopOpacity="0.25" />
          <stop offset="1" stopColor="var(--accent)" stopOpacity="0" />
        </linearGradient>
      </defs>
      <polygon points={`0,30 ${points} 100,30`} fill={`url(#${id})`} />
      <polyline
        points={points}
        fill="none"
        stroke="var(--accent)"
        strokeWidth={2}
        strokeLinejoin="round"
        vectorEffect="non-scaling-stroke"
      />
    </svg>
  );
}

/** The change against the 24 hours before; nothing to compare with a zero. */
function Change({ now, before }: { now: number; before: number }) {
  if (before === 0) return now === 0 ? <span className="text-sm text-muted">0%</span> : null;
  const pct = Math.round(((now - before) / before) * 100);
  return (
    <span
      title={`${fmt(before)} the 24 hours before`}
      className={`text-sm tabular-nums ${pct > 0 ? "text-ok" : pct < 0 ? "text-danger" : "text-muted"}`}
    >
      {pct > 0 ? "↗ " : pct < 0 ? "↘ " : ""}
      {Math.abs(pct)}%
    </span>
  );
}

function SiteCard({ site }: { site: Glance }) {
  const href = `/?site=${site.id}`;
  const settings = `/sites?site=${site.id}`;
  return (
    <article className="relative flex flex-col gap-3 rounded-2xl border border-line bg-card p-5 transition hover:border-accent has-[a:focus-visible]:border-accent">
      <div className="flex items-start gap-2">
        <span className="flex min-w-0 flex-1 flex-col">
          <a
            href={href}
            onClick={(e) => (e.preventDefault(), navigate(href))}
            className="truncate text-base font-semibold outline-none after:absolute after:inset-0 after:rounded-2xl"
          >
            {site.name}
          </a>
          <span className="truncate text-xs text-muted">{site.host ?? "any host"}</span>
        </span>
        <a
          href={settings}
          onClick={(e) => (e.preventDefault(), navigate(settings))}
          aria-label={`Settings for ${site.name}`}
          title="Snippet and settings"
          className="relative z-10 -mr-1.5 -mt-1 rounded-lg p-1.5 text-muted hover:bg-accent-soft hover:text-fg"
        >
          <svg viewBox="0 0 20 20" className="size-4" fill="currentColor" aria-hidden>
            <circle cx="10" cy="4" r="1.6" />
            <circle cx="10" cy="10" r="1.6" />
            <circle cx="10" cy="16" r="1.6" />
          </svg>
        </a>
      </div>
      <Sparkline values={site.series} />
      <div className="flex items-end gap-2">
        <span className="flex flex-col">
          <span className="text-2xl font-bold tabular-nums">{fmt(site.visitors)}</span>
          <span className="text-sm text-muted">
            {site.visitors === 1 ? "visitor" : "visitors"} in last 24h
          </span>
        </span>
        <span className="ml-auto">
          <Change now={site.visitors} before={site.previous} />
        </span>
      </div>
    </article>
  );
}

export function Overview() {
  const overview = useApi<Glance[]>("/api/overview", 60_000);
  const [search, setSearch] = useState("");
  const input = useRef<HTMLInputElement>(null);

  // "/" jumps to the search box, as on Plausible.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const typing = e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement;
      if (e.key === "/" && !typing) (e.preventDefault(), input.current?.focus());
    };
    addEventListener("keydown", onKey);
    return () => removeEventListener("keydown", onKey);
  }, []);

  if (overview.error) return <ErrorText error={overview.error} />;
  const all = overview.data;
  if (!all) return <p className="text-sm text-muted">Loading…</p>;
  if (all.length === 0) {
    return (
      <Card>
        <div className="flex flex-col items-start gap-3 py-6">
          <h2 className="text-lg font-semibold">No sites yet</h2>
          <p className="text-sm text-muted">Add a site, then paste its snippet into your pages.</p>
          <a
            href="/sites?site=new"
            onClick={(e) => (e.preventDefault(), navigate("/sites?site=new"))}
            className="rounded-lg bg-accent px-3 py-1.5 text-sm font-medium text-white"
          >
            Add a site
          </a>
        </div>
      </Card>
    );
  }

  const q = search.trim().toLowerCase();
  const shown = all
    .filter((s) => !q || s.name.toLowerCase().includes(q) || s.host?.toLowerCase().includes(q))
    .sort((a, b) => b.visitors - a.visitors);

  return (
    <div className="flex flex-col gap-4">
      <h1 className="sr-only">Overview</h1>
      <div className="flex flex-wrap items-center gap-3">
        <input
          ref={input}
          type="search"
          aria-label="Search sites"
          placeholder="Press / to search"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          className="min-w-0 flex-[1_1_200px] rounded-lg sm:flex-[0_1_320px] border border-line bg-card px-3 py-1.5 text-sm outline-none placeholder:text-muted focus:border-accent"
        />
        <span className="hidden text-xs text-muted sm:inline">Most visitors first</span>
        <a
          href="/sites?site=new"
          onClick={(e) => (e.preventDefault(), navigate("/sites?site=new"))}
          className="ml-auto rounded-lg bg-accent px-3 py-1.5 text-sm font-medium text-white hover:opacity-90"
        >
          Add site
        </a>
      </div>
      <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
        {shown.map((s) => <SiteCard key={s.id} site={s} />)}
      </div>
      {shown.length === 0 && <p className="text-sm text-muted">No site matches “{search}”.</p>}
    </div>
  );
}
