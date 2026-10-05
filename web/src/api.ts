// The owner API (server/src/api.rs and stats.rs). Same origin: in production
// the Realm gate injects the identity header on every request, in dev the
// Vite proxy does.

import { useCallback, useEffect, useState } from "react";

export type Site = {
  id: string;
  name: string;
  hostnames: string[];
  created_at: number;
  /** The linked Search Console property, e.g. `sc-domain:example.com`. */
  search_console: string | null;
};

/** A site in the Sites list, with what it has seen lately. */
export type SiteActivity = Site & {
  /** Seconds since the epoch; null until the first visit. */
  last_event: number | null;
  /** Distinct visitors over the last 7 days. */
  visitors: number;
};

/** `available` is false when the operator did not grant internet access. */
export type Google = { available: boolean; email: string | null };

export type SearchRow = {
  query: string;
  clicks: number;
  impressions: number;
  ctr: number;
  position: number;
};

export type Search = {
  property: string;
  from: string;
  to: string;
  totals: { clicks: number; impressions: number; ctr: number; position: number } | null;
  queries: SearchRow[];
};

export type Me = { username: string; timezone: string };

export const periods = ["today", "yesterday", "7d", "30d", "90d", "365d"] as const;
export type Period = (typeof periods)[number];

export type Row = { key: string | null; visitors: number; pageviews: number };

export type Stats = {
  from: string;
  to: string;
  bucket: "hour" | "day";
  totals: { visitors: number; pageviews: number; events: number };
  series: { t: number; visitors: number; pageviews: number }[];
  pages: Row[];
  referrers: Row[];
  campaigns: Row[];
  countries: Row[];
  devices: Row[];
  events: { name: string; count: number; visitors: number }[];
};

export type Prop = { key: string; value: string; count: number; visitors: number };

export async function api<T>(path: string, init?: { method?: string; body?: unknown }): Promise<T> {
  const res = await fetch(path, {
    method: init?.method ?? "GET",
    headers: init?.body === undefined ? undefined : { "content-type": "application/json" },
    body: init?.body === undefined ? undefined : JSON.stringify(init.body),
  });
  if (res.status === 204) return undefined as T;
  const json = await res.json().catch(() => null);
  if (!res.ok) throw new Error(json?.error ?? `${res.status} ${res.statusText}`);
  return json as T;
}

export type Loaded<T> = {
  data: T | undefined;
  error: Error | undefined;
  loading: boolean;
  reload: () => void;
};

/** GETs `path` (skipped while null), again on `reload` and every `refreshMs`. */
export function useApi<T>(path: string | null, refreshMs?: number): Loaded<T> {
  const [data, setData] = useState<T>();
  const [error, setError] = useState<Error>();
  const [loading, setLoading] = useState(false);
  const [tick, setTick] = useState(0);
  const reload = useCallback(() => setTick((n) => n + 1), []);

  useEffect(() => {
    if (!path) return;
    let live = true;
    setLoading(true);
    api<T>(path)
      .then((d) => live && (setData(d), setError(undefined)))
      .catch((e: Error) => live && setError(e))
      .finally(() => live && setLoading(false));
    return () => {
      live = false;
    };
  }, [path, tick]);

  useEffect(() => {
    if (!path || !refreshMs) return;
    const id = setInterval(reload, refreshMs);
    return () => clearInterval(id);
  }, [path, refreshMs, reload]);

  return { data, error, loading, reload };
}
