// What belongs to the user rather than to a site: the Google Search Console
// account and the time zone the dashboard counts days in.

import { useState } from "react";
import { api, type Google, type Me, type Site, useApi } from "./api.ts";
import { GoogleCard } from "./SearchConsole.tsx";
import { Button, Card, ErrorText } from "./ui.tsx";

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

export function Settings({ me, reloadMe }: { me: Me | undefined; reloadMe: () => void }) {
  const google = useApi<Google>("/api/google");
  const sites = useApi<Site[]>("/api/sites");
  const connected = !!google.data?.email;
  const properties = useApi<string[]>(connected ? "/api/google/properties" : null);
  return (
    <div className="flex flex-col gap-4">
      <h1 className="text-lg font-semibold">Settings</h1>
      <div className="flex flex-wrap items-start gap-4">
        <div className="flex min-w-0 flex-[999_1_560px] flex-col gap-2">
          <GoogleCard
            google={google.data}
            properties={properties.data}
            sites={sites.data ?? []}
            onChange={() => {
              google.reload();
              sites.reload();
            }}
            onRefresh={properties.reload}
          />
          <ErrorText error={properties.error} />
        </div>
        <div className="min-w-0 flex-[1_1_280px]">
          <TimeZone me={me} onChange={reloadMe} />
        </div>
      </div>
    </div>
  );
}
