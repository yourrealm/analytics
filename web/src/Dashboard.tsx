// One site and period at a time, both in the URL (`?site=…&period=…`), plus
// the custom event whose properties are open (`&event=…`).

import { Area, AreaChart, CartesianGrid, Tooltip, XAxis, YAxis } from "recharts";
import {
  type Me,
  type Period,
  periods,
  type Prop,
  type Row,
  type Search,
  type Site,
  type Stats,
  useApi,
} from "./api.ts";
import { navigate, withQuery } from "./route.ts";
import { type Bar, BarList, Card, ErrorText, fmt } from "./ui.tsx";

const PERIOD_LABEL: Record<Period, string> = {
  today: "Today",
  yesterday: "Yesterday",
  "7d": "7 days",
  "30d": "30 days",
  "90d": "90 days",
  "365d": "12 months",
};

type Metric = "visitors" | "pageviews";

function country(code: string): string {
  const flag = String.fromCodePoint(...[...code.toUpperCase()].map((c) => 0x1f1a5 + c.charCodeAt(0)));
  let name = code;
  try {
    name = new Intl.DisplayNames(undefined, { type: "region" }).of(code) ?? code;
  } catch {
    // Not a region code Intl knows: show it as is.
  }
  return `${flag} ${name}`;
}

function bars(rows: Row[], empty: string, label: (k: string) => string = (k) => k): Bar[] {
  return rows.map((r) => ({ key: r.key ?? "", label: r.key === null ? empty : label(r.key), value: r.visitors }));
}

function Stat({ label, value, active, onClick }: {
  label: string;
  value: number;
  active?: boolean;
  onClick?: () => void;
}) {
  const body = (
    <>
      <span className="text-xs font-semibold uppercase tracking-wide text-muted">{label}</span>
      <span className="text-3xl font-bold tabular-nums">{fmt(value)}</span>
    </>
  );
  const base = "flex flex-col gap-1 rounded-xl px-4 py-3 text-left";
  return onClick
    ? (
      <button
        type="button"
        onClick={onClick}
        className={`${base} cursor-pointer ${active ? "bg-accent-soft" : "hover:bg-accent-soft/50"}`}
      >
        {body}
      </button>
    )
    : <div className={base}>{body}</div>;
}

function Chart({ stats, metric, timezone }: { stats: Stats; metric: Metric; timezone: string }) {
  const tick = new Intl.DateTimeFormat(undefined, {
    timeZone: timezone,
    ...(stats.bucket === "hour" ? { hour: "2-digit", minute: "2-digit" } : { month: "short", day: "numeric" }),
  });
  const full = new Intl.DateTimeFormat(undefined, {
    timeZone: timezone,
    ...(stats.bucket === "hour"
      ? { weekday: "short", hour: "2-digit", minute: "2-digit" }
      : { weekday: "short", month: "short", day: "numeric" }),
  });
  const data = stats.series.map((p) => ({ ...p, ms: p.t * 1000 }));
  return (
    <AreaChart
      responsive
      style={{ width: "100%", height: 260 }}
      data={data}
      margin={{ top: 8, right: 8, left: 0, bottom: 0 }}
    >
      <defs>
        <linearGradient id="fill" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor="var(--accent)" stopOpacity={0.35} />
          <stop offset="100%" stopColor="var(--accent)" stopOpacity={0} />
        </linearGradient>
      </defs>
      <CartesianGrid vertical={false} stroke="var(--line)" />
      <XAxis
        dataKey="ms"
        tickFormatter={(v: number) => tick.format(v)}
        stroke="var(--muted)"
        fontSize={12}
        tickLine={false}
        axisLine={false}
        minTickGap={24}
      />
      <YAxis
        allowDecimals={false}
        stroke="var(--muted)"
        fontSize={12}
        tickLine={false}
        axisLine={false}
        width="auto"
      />
      <Tooltip
        labelFormatter={(v) => full.format(Number(v))}
        formatter={(v) => [fmt(Number(v)), metric === "visitors" ? "Visitors" : "Pageviews"]}
        contentStyle={{
          background: "var(--card)",
          border: "1px solid var(--line)",
          borderRadius: 12,
          color: "var(--fg)",
        }}
      />
      <Area
        type="monotone"
        dataKey={metric}
        stroke="var(--accent)"
        strokeWidth={2}
        fill="url(#fill)"
        isAnimationActive={false}
      />
    </AreaChart>
  );
}

/** Search Console's performance report for a property. */
const consoleUrl = (property: string) =>
  `https://search.google.com/search-console/performance/search-analytics?resource_id=${encodeURIComponent(property)}`;

function SearchTerms({ site, period }: { site: Site; period: Period }) {
  const search = useApi<Search>(`/api/sites/${site.id}/search?period=${period}`);
  const s = search.data;
  const position = (p: number) => p.toFixed(1);
  return (
    <Card
      title="Search terms"
      action={
        <a
          href={consoleUrl(site.search_console!)}
          target="_blank"
          rel="noopener"
          className="text-xs text-muted underline"
        >
          Google Search Console
        </a>
      }
    >
      <ErrorText error={search.error} />
      {s?.totals && (
        <p className="mb-3 text-sm text-muted">
          <span className="font-semibold text-fg">{fmt(s.totals.clicks)}</span> clicks from{" "}
          <span className="font-semibold text-fg">{fmt(s.totals.impressions)}</span> impressions, average position{" "}
          <span className="font-semibold text-fg">{position(s.totals.position)}</span>
        </p>
      )}
      {s && (
        <BarList
          items={s.queries.map((q) => ({
            key: q.query,
            label: q.query,
            value: q.clicks,
            detail: `${fmt(q.impressions)} impr · pos ${position(q.position)}`,
          }))}
          empty="No searches in this period."
        />
      )}
      <p className="mt-3 text-xs text-muted">Google's numbers run about two to three days behind.</p>
    </Card>
  );
}

export function Dashboard({ query, me }: { query: URLSearchParams; me: Me | undefined }) {
  const sites = useApi<Site[]>("/api/sites");
  const list = sites.data ?? [];
  const site = list.find((s) => s.id === query.get("site")) ?? list[0];
  const period = (periods as readonly string[]).includes(query.get("period") ?? "")
    ? (query.get("period") as Period)
    : "30d";
  const event = query.get("event") ?? "";
  const metric: Metric = query.get("metric") === "pageviews" ? "pageviews" : "visitors";

  const stats = useApi<Stats>(site ? `/api/sites/${site.id}/stats?period=${period}` : null, 60_000);
  const props = useApi<Prop[]>(
    site && event
      ? `/api/sites/${site.id}/props?period=${period}&event=${encodeURIComponent(event)}`
      : null,
  );
  const set = (patch: Record<string, string>) => navigate(withQuery(patch), true);

  if (sites.error) return <ErrorText error={sites.error} />;
  if (sites.data && !site) {
    return (
      <Card>
        <div className="flex flex-col items-start gap-3 py-6">
          <h2 className="text-lg font-semibold">No sites yet</h2>
          <p className="text-sm text-muted">Add a site, then paste its snippet into your pages.</p>
          <a
            href="/sites"
            onClick={(e) => (e.preventDefault(), navigate("/sites"))}
            className="rounded-lg bg-accent px-3 py-1.5 text-sm font-medium text-white"
          >
            Add a site
          </a>
        </div>
      </Card>
    );
  }

  const s = stats.data;
  return (
    <div className="flex flex-col gap-4">
      <div className="flex flex-wrap items-center gap-3">
        {list.length > 1
          ? (
            <select
              aria-label="Site"
              value={site?.id ?? ""}
              onChange={(e) => set({ site: e.target.value, event: "" })}
              className="rounded-lg border border-line bg-card px-3 py-1.5 text-sm font-semibold"
            >
              {list.map((x) => <option key={x.id} value={x.id}>{x.name}</option>)}
            </select>
          )
          : <h1 className="text-lg font-semibold">{site?.name}</h1>}
        <div role="tablist" aria-label="Period" className="flex flex-wrap rounded-lg border border-line bg-card p-0.5">
          {periods.map((p) => (
            <button
              key={p}
              type="button"
              role="tab"
              aria-selected={p === period}
              onClick={() => set({ period: p === "30d" ? "" : p, event: "" })}
              className={`cursor-pointer rounded-md px-2.5 py-1 text-sm ${
                p === period ? "bg-accent text-white" : "text-muted hover:text-fg"
              }`}
            >
              {PERIOD_LABEL[p]}
            </button>
          ))}
        </div>
        {s && <span className="text-sm text-muted">{s.from === s.to ? s.from : `${s.from} to ${s.to}`}</span>}
      </div>

      <ErrorText error={stats.error} />

      {s && (
        <>
          <Card>
            <div className="mb-4 flex flex-wrap gap-2">
              <Stat
                label="Visitors"
                value={s.totals.visitors}
                active={metric === "visitors"}
                onClick={() => set({ metric: "" })}
              />
              <Stat
                label="Pageviews"
                value={s.totals.pageviews}
                active={metric === "pageviews"}
                onClick={() => set({ metric: "pageviews" })}
              />
              <Stat label="Events" value={s.totals.events} />
            </div>
            <Chart stats={s} metric={metric} timezone={me?.timezone ?? "UTC"} />
          </Card>

          <div className="grid items-start gap-4 md:grid-cols-2">
            <Card title="Pages">
              <BarList items={bars(s.pages, "(none)")} />
            </Card>
            <Card title="Referrers">
              <BarList items={bars(s.referrers, "Direct / none")} />
            </Card>
            <Card title="Countries">
              <BarList items={bars(s.countries, "Unknown", country)} />
            </Card>
            <Card title="Devices">
              <BarList items={bars(s.devices, "Unknown")} />
            </Card>
            {s.campaigns.some((c) => c.key !== null) && (
              <Card title="Campaigns" action={<span className="text-xs text-muted">utm_source</span>}>
                <BarList items={bars(s.campaigns.filter((c) => c.key !== null), "")} />
              </Card>
            )}
            {site?.search_console && <SearchTerms site={site} period={period} />}
            <Card title="Events" action={<span className="text-xs text-muted">click for properties</span>}>
              <BarList
                items={s.events.map((e) => ({
                  key: e.name,
                  label: e.name,
                  value: e.count,
                  detail: `${fmt(e.visitors)} ${e.visitors === 1 ? "visitor" : "visitors"}`,
                }))}
                selected={event}
                onSelect={(k) => set({ event: k === event ? "" : k })}
                empty="No custom events in this period."
              />
              {event && (
                <div className="mt-4 border-t border-line pt-4">
                  <p className="mb-2 text-xs font-semibold text-muted">Properties of {event}</p>
                  <ErrorText error={props.error} />
                  <BarList
                    items={(props.data ?? []).map((p) => ({
                      key: `${p.key}=${p.value}`,
                      label: (
                        <>
                          <span className="text-muted">{p.key}:</span> {p.value}
                        </>
                      ),
                      value: p.count,
                    }))}
                    empty="No properties."
                  />
                </div>
              )}
            </Card>
          </div>
        </>
      )}

      <p className="text-xs text-muted">
        Countries:{" "}
        <a href="https://db-ip.com" target="_blank" rel="noopener" className="underline">
          IP Geolocation by DB-IP
        </a>
      </p>
    </div>
  );
}
