import { api, errorText, onEvent } from "./api";
import { clear, el, pill } from "./dom";
import { ROUTES, currentRoute, navigate, type Route, type View } from "./router";
import type { MindEvent, MindStatus } from "./types";
import { ChatView } from "./views/chat";
import { KnowledgeView } from "./views/knowledge";
import { NowView } from "./views/now";
import { QuestionsView } from "./views/questions";
import { SelfView } from "./views/self";
import { SettingsView } from "./views/settings";
import { TimelineView } from "./views/timeline";

const STATUS_POLL_MS = 15_000;

function createView(route: Route): View {
  switch (route) {
    case "now":
      return new NowView();
    case "chat":
      return new ChatView();
    case "timeline":
      return new TimelineView();
    case "questions":
      return new QuestionsView();
    case "knowledge":
      return new KnowledgeView();
    case "self":
      return new SelfView();
    case "settings":
      return new SettingsView();
    default: {
      const never: never = route;
      throw new Error(`unknown route ${String(never)}`);
    }
  }
}

class App {
  private viewRoot: HTMLElement;
  private navLinks = new Map<Route, HTMLAnchorElement>();
  private statusBox: HTMLElement;
  private footer: HTMLElement;
  private current: { route: Route; view: View } | null = null;
  private status: MindStatus | null = null;

  constructor(root: HTMLElement) {
    clear(root);
    const nav = el("nav", { class: "sidebar" });
    nav.appendChild(el("div", { class: "brand" }, "Aethra"));
    const links = el("ul", { class: "nav" });
    for (const r of ROUTES) {
      const a = el("a", { href: `#/${r.id}` }, r.label);
      this.navLinks.set(r.id, a);
      links.appendChild(el("li", null, a));
    }
    nav.appendChild(links);
    this.statusBox = el("div", { class: "status-box" });
    nav.appendChild(this.statusBox);

    this.viewRoot = el("main", { class: "content" });
    this.footer = el("footer", { class: "statusbar muted small" }, "connecting to the mind...");
    root.append(nav, this.viewRoot, this.footer);

    window.addEventListener("hashchange", () => void this.route());

    // Presence signal for the scheduler, throttled so typing does not flood IPC.
    let lastTouch = 0;
    document.addEventListener(
      "keydown",
      () => {
        const now = Date.now();
        if (now - lastTouch < 30_000) return;
        lastTouch = now;
        void api.touchActivity().catch(() => undefined);
      },
      { passive: true },
    );
  }

  async start(): Promise<void> {
    await onEvent((ev) => this.handleEvent(ev));
    await this.refreshStatus();
    window.setInterval(() => void this.refreshStatus(), STATUS_POLL_MS);
    await this.route();
  }

  private async route(): Promise<void> {
    const route = currentRoute();
    if (location.hash === "") navigate(route);
    if (this.current?.route === route) return;
    this.current?.view.unmount?.();
    clear(this.viewRoot);
    const view = createView(route);
    this.current = { route, view };
    for (const [id, a] of this.navLinks) a.classList.toggle("active", id === route);
    try {
      await view.mount(this.viewRoot);
      if (this.status) view.onStatus?.(this.status);
    } catch (e) {
      this.viewRoot.appendChild(el("p", { class: "error" }, errorText(e)));
    }
  }

  private async refreshStatus(): Promise<void> {
    try {
      this.status = await api.status();
      this.renderStatus();
      this.current?.view.onStatus?.(this.status);
    } catch (e) {
      this.footer.textContent = `status unavailable: ${errorText(e)}`;
    }
  }

  private renderStatus(): void {
    const s = this.status;
    clear(this.statusBox);
    if (!s) return;
    const modeTone = s.mode === "learning" ? "tone-learning" : s.mode === "chat" ? "tone-chat" : "tone-idle";
    this.statusBox.append(
      el("div", { class: "row gap" }, pill(s.mode, modeTone), pill(s.model_reachable ? "model" : "no model", s.model_reachable ? "tone-ok" : "tone-bad")),
      el("div", { class: "muted small" }, s.current_job ? `working: ${s.current_job}` : gateSummary(s)),
      el("div", { class: "muted small" }, `${s.open_questions} open questions - ${s.unconsolidated_episodes} to consolidate`),
    );
    this.footer.textContent = `${s.name} - ${s.total_episodes} episodes - ${s.notes} notes - energy ${s.state.energy.toFixed(2)} - curiosity ${s.state.curiosity.toFixed(2)}`;
  }

  private handleEvent(ev: MindEvent): void {
    switch (ev.type) {
      case "mode_changed":
      case "learning_gate_changed":
      case "model_status":
      case "state_changed":
      case "job_started":
      case "job_finished":
        void this.refreshStatus();
        break;
      case "episode_recorded":
      case "log":
        break;
      default: {
        const never: never = ev;
        void never;
      }
    }
    this.current?.view.onEvent?.(ev);
  }
}

function gateSummary(s: MindStatus): string {
  const g = s.learning_gate;
  switch (g.kind) {
    case "allowed":
      return "ready to learn";
    case "outside_window":
      return `learning window ${g.window}`;
    case "user_active":
      return "waiting for quiet";
    case "budget_exhausted":
      return "budget spent for today";
    case "manually_stopped":
      return "learning stopped";
    default: {
      const never: never = g;
      return String(never);
    }
  }
}

window.addEventListener("DOMContentLoaded", () => {
  const root = document.getElementById("app");
  if (!root) throw new Error("missing #app root");
  void new App(root).start();
});
