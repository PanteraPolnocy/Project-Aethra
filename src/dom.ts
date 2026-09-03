// Tiny DOM helpers. Everything goes through textContent; no innerHTML with
// data from the mind or the web.

type Child = Node | string | number | null | undefined | false;
type Handler = (e: Event) => void;
type Attrs = Record<string, string | boolean | Handler | undefined>;

export function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  attrs?: Attrs | null,
  ...children: Child[]
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (attrs) {
    for (const [key, value] of Object.entries(attrs)) {
      if (value === undefined || value === false) continue;
      if (typeof value === "function") {
        node.addEventListener(key.startsWith("on") ? key.slice(2) : key, value);
      } else if (value === true) {
        node.setAttribute(key, "");
      } else {
        node.setAttribute(key, value);
      }
    }
  }
  append(node, ...children);
  return node;
}

export function append(node: Node, ...children: Child[]): void {
  for (const child of children) {
    if (child === null || child === undefined || child === false) continue;
    node.appendChild(typeof child === "object" ? child : document.createTextNode(String(child)));
  }
}

export function clear(node: Node): void {
  while (node.firstChild) node.removeChild(node.firstChild);
}

export function fmtTime(iso: string | null | undefined): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return d.toLocaleString(undefined, {
    year: "numeric",
    month: "short",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function fmtRelative(iso: string | null | undefined): string {
  if (!iso) return "";
  const t = new Date(iso).getTime();
  if (Number.isNaN(t)) return iso;
  const secs = Math.max(0, Math.round((Date.now() - t) / 1000));
  if (secs < 60) return `${secs}s ago`;
  const mins = Math.round(secs / 60);
  if (mins < 60) return `${mins} min ago`;
  const hours = Math.round(mins / 60);
  if (hours < 48) return `${hours} h ago`;
  return `${Math.round(hours / 24)} d ago`;
}

export function fmtDuration(secs: number): string {
  if (secs < 60) return `${secs}s`;
  const m = Math.floor(secs / 60);
  if (m < 60) return `${m} min`;
  const h = Math.floor(m / 60);
  return `${h} h ${m % 60} min`;
}

export function fmtBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

export function shortId(id: string | null | undefined): string {
  return id ? id.slice(0, 8) : "";
}

export function bar(value: number, max: number, label: string, tone?: string): HTMLElement {
  const ratio = max > 0 ? Math.min(1, value / max) : 0;
  const fill = el("div", { class: `bar-fill ${tone ?? ""}` });
  fill.style.width = `${Math.round(ratio * 100)}%`;
  return el(
    "div",
    { class: "bar" },
    el("div", { class: "bar-label" }, el("span", null, label), el("span", { class: "muted" }, `${Math.round(ratio * 100)}%`)),
    el("div", { class: "bar-track" }, fill),
  );
}

export function pill(text: string, tone?: string): HTMLElement {
  return el("span", { class: `pill ${tone ?? ""}` }, text);
}
