// Two views and some query parameters don't need a router library: the path
// picks the view, the query holds the dashboard's filters, so every view can
// be bookmarked.

import { useEffect, useState } from "react";

export type View = "dashboard" | "sites";

function read() {
  return {
    view: (location.pathname.startsWith("/sites") ? "sites" : "dashboard") as View,
    query: new URLSearchParams(location.search),
  };
}

export function useRoute() {
  const [route, setRoute] = useState(read);
  useEffect(() => {
    const onPop = () => setRoute(read());
    addEventListener("popstate", onPop);
    return () => removeEventListener("popstate", onPop);
  }, []);
  return route;
}

/** Pushes a new URL and tells `useRoute` about it. */
export function navigate(url: string, replace = false) {
  if (replace) history.replaceState(null, "", url);
  else history.pushState(null, "", url);
  dispatchEvent(new PopStateEvent("popstate"));
}

/** The current URL with some query parameters set (empty removes one). */
export function withQuery(patch: Record<string, string>): string {
  const q = new URLSearchParams(location.search);
  for (const [k, v] of Object.entries(patch)) {
    if (v) q.set(k, v);
    else q.delete(k);
  }
  const s = q.toString();
  return location.pathname + (s ? `?${s}` : "");
}
