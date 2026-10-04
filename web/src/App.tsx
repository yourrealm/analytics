import { type Me, useApi } from "./api.ts";
import { Dashboard } from "./Dashboard.tsx";
import { navigate, useRoute, type View } from "./route.ts";
import { Sites } from "./Sites.tsx";
import { ErrorText } from "./ui.tsx";

const NAV: { view: View; label: string; href: string }[] = [
  { view: "dashboard", label: "Dashboard", href: "/" },
  { view: "sites", label: "Sites", href: "/sites" },
];

export function App() {
  const { view, query } = useRoute();
  const me = useApi<Me>("/api/me");

  return (
    <div className="mx-auto flex min-h-screen max-w-6xl flex-col gap-6 px-4 py-6">
      <header className="flex items-center gap-6">
        <span className="flex items-center gap-2 text-base font-bold">
          <picture>
            <source srcSet="/logo-dark.svg" media="(prefers-color-scheme: dark)" />
            <img src="/logo.svg" alt="" className="size-8" />
          </picture>
          Analytics
        </span>
        <nav className="flex gap-1">
          {NAV.map((n) => (
            <a
              key={n.view}
              href={n.href}
              aria-current={n.view === view ? "page" : undefined}
              onClick={(e) => (e.preventDefault(), navigate(n.href))}
              className={`rounded-lg px-3 py-1.5 text-sm ${
                n.view === view ? "bg-accent-soft font-semibold text-fg" : "text-muted hover:text-fg"
              }`}
            >
              {n.label}
            </a>
          ))}
        </nav>
        <span className="ml-auto text-sm text-muted">{me.data?.username}</span>
      </header>
      <main>
        <ErrorText error={me.error} />
        {view === "sites" ? <Sites me={me.data} reloadMe={me.reload} /> : <Dashboard query={query} me={me.data} />}
      </main>
    </div>
  );
}
