// Small shared pieces. No component library: the app is three views.

import type { ReactNode } from "react";

export const fmt = (n: number) => n.toLocaleString();

export function Card({ title, action, children, className = "" }: {
  title?: ReactNode;
  action?: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  return (
    <section className={`rounded-2xl border border-line bg-card p-5 ${className}`}>
      {(title || action) && (
        <div className="mb-4 flex items-center justify-between gap-3">
          {title && <h2 className="text-sm font-semibold">{title}</h2>}
          {action}
        </div>
      )}
      {children}
    </section>
  );
}

const buttonStyles = {
  primary: "bg-accent text-white hover:opacity-90",
  outline: "border border-line hover:bg-accent-soft",
  ghost: "text-muted hover:bg-accent-soft hover:text-fg",
  danger: "bg-danger text-white hover:opacity-90",
};

export function Button({ variant = "primary", className = "", ...props }:
  & React.ButtonHTMLAttributes<HTMLButtonElement>
  & { variant?: keyof typeof buttonStyles })
{
  return (
    <button
      {...props}
      className={`inline-flex cursor-pointer items-center justify-center gap-1.5 rounded-lg px-3 py-1.5 text-sm font-medium transition disabled:cursor-default disabled:opacity-50 ${
        buttonStyles[variant]
      } ${className}`}
    />
  );
}

export function Input(props: React.InputHTMLAttributes<HTMLInputElement>) {
  return (
    <input
      {...props}
      className={`min-w-0 rounded-lg border border-line bg-bg px-3 py-1.5 text-sm outline-none placeholder:text-muted focus:border-accent ${
        props.className ?? ""
      }`}
    />
  );
}

export type Bar = { key: string; label: ReactNode; value: number; detail?: string };

/** Ranked rows, each with a bar scaled to the largest value. */
export function BarList({ items, selected, onSelect, empty = "Nothing yet." }: {
  items: Bar[];
  selected?: string;
  onSelect?: (key: string) => void;
  empty?: string;
}) {
  if (!items.length) return <p className="text-sm text-muted">{empty}</p>;
  const max = Math.max(...items.map((i) => i.value), 1);
  return (
    <ul className="flex flex-col gap-1">
      {items.map((item) => {
        const row = (
          <>
            <span
              className="absolute inset-y-0 left-0 rounded-md bg-accent-soft"
              style={{ width: `${(item.value / max) * 100}%` }}
            />
            <span className="relative min-w-0 flex-1 truncate">{item.label}</span>
            {item.detail && <span className="relative text-xs text-muted">{item.detail}</span>}
            <span className="relative w-14 text-right font-semibold tabular-nums">{fmt(item.value)}</span>
          </>
        );
        const base = "relative flex h-8 w-full items-center gap-3 rounded-md px-2 text-left text-sm";
        return (
          <li key={item.key}>
            {onSelect
              ? (
                <button
                  type="button"
                  aria-pressed={selected === item.key}
                  onClick={() => onSelect(item.key)}
                  className={`${base} cursor-pointer hover:outline hover:outline-line ${
                    selected === item.key ? "outline outline-accent" : ""
                  }`}
                >
                  {row}
                </button>
              )
              : <div className={base}>{row}</div>}
          </li>
        );
      })}
    </ul>
  );
}

export function ErrorText({ error }: { error?: Error | string | null }) {
  if (!error) return null;
  return <p className="text-sm text-danger">{typeof error === "string" ? error : error.message}</p>;
}
