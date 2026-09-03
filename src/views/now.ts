import { api, errorText } from "../api";
import { bar, clear, el, fmtBytes, fmtDuration, fmtRelative, fmtTime, pill } from "../dom";
import type { View } from "../router";
import type { BudgetStatus, EpisodeRow, LearningGate, MindEvent, MindStatus } from "../types";

function gateText(g: LearningGate): string {
  switch (g.kind) {
    case "allowed":
      return `Learning allowed: ${g.reason}`;
    case "outside_window":
      return `Waiting for the learning window (${g.window})`;
    case "user_active":
      return `Waiting for quiet: ${Math.round(g.quiet_for_secs / 60)} of ${Math.round(g.required_secs / 60)} min`;
    case "budget_exhausted":
      return `Paused: ${g.reason}`;
    case "manually_stopped":
      return "Stopped by you until the next quiet period";
    default: {
      const never: never = g;
      return String(never);
    }
  }
}

function budgetLabel(b: BudgetStatus): string {
  switch (b.resource) {
    case "learning_tokens":
      return `Learning tokens ${b.used.toLocaleString()} / ${b.limit.toLocaleString()}`;
    case "http_requests":
      return `Web requests ${b.used} / ${b.limit}`;
    case "http_bytes":
      return `Web bytes ${fmtBytes(b.used)} / ${fmtBytes(b.limit)}`;
    case "learning_minutes":
      return `Learning minutes ${b.used} / ${b.limit}`;
    case "research_jobs":
      return `Research jobs ${b.used} / ${b.limit}`;
    default: {
      const never: never = b.resource;
      return String(never);
    }
  }
}

export class NowView implements View {
  private root: HTMLElement | null = null;
  private status: MindStatus | null = null;
  private recent: EpisodeRow[] = [];
  private log: string[] = [];
  private timer: number | null = null;

  async mount(root: HTMLElement): Promise<void> {
    this.root = root;
    await this.refresh();
    this.timer = window.setInterval(() => void this.refresh(), 20_000);
  }

  unmount(): void {
    if (this.timer !== null) window.clearInterval(this.timer);
    this.timer = null;
    this.root = null;
  }

  onStatus(status: MindStatus): void {
    this.status = status;
    this.render();
  }

  onEvent(ev: MindEvent): void {
    switch (ev.type) {
      case "job_started":
        this.pushLog(`Job started: ${ev.kind} - ${ev.detail}`);
        break;
      case "job_finished":
        this.pushLog(`${ev.success ? "Job done" : "Job failed"}: ${ev.kind} - ${ev.outcome}`);
        void this.refresh();
        break;
      case "episode_recorded":
        void this.refresh();
        break;
      case "model_status":
        this.pushLog(`Model: ${ev.detail}`);
        break;
      case "log":
        this.pushLog(ev.message);
        break;
      case "mode_changed":
      case "learning_gate_changed":
      case "state_changed":
        break;
      default: {
        const never: never = ev;
        void never;
      }
    }
  }

  private pushLog(line: string): void {
    this.log.unshift(`${new Date().toLocaleTimeString()}  ${line}`);
    if (this.log.length > 30) this.log.length = 30;
    this.render();
  }

  private async refresh(): Promise<void> {
    try {
      const [status, recent] = await Promise.all([api.status(), api.timeline(8)]);
      this.status = status;
      this.recent = recent;
    } catch (e) {
      this.pushLog(`Refresh failed: ${errorText(e)}`);
    }
    this.render();
  }

  private render(): void {
    if (!this.root) return;
    clear(this.root);
    const s = this.status;
    if (!s) {
      this.root.appendChild(el("p", { class: "muted" }, "Loading..."));
      return;
    }

    const modeTone = s.mode === "learning" ? "tone-learning" : s.mode === "chat" ? "tone-chat" : "tone-idle";
    const header = el(
      "div",
      { class: "row wrap gap" },
      el("h1", null, s.name),
      pill(s.mode, modeTone),
      pill(s.model_reachable ? "model reachable" : "model unreachable", s.model_reachable ? "tone-ok" : "tone-bad"),
      s.model_loaded_profile ? pill(`loaded: ${s.model_loaded_profile}`, "tone-neutral") : null,
    );

    const controls = el(
      "div",
      { class: "row gap" },
      el(
        "button",
        {
          class: "primary",
          onclick: async () => {
            try {
              await api.requestLearning();
              await this.refresh();
            } catch (e) {
              this.pushLog(errorText(e));
            }
          },
        },
        "Learn now",
      ),
      el(
        "button",
        {
          onclick: async () => {
            try {
              await api.stopLearning();
              await this.refresh();
            } catch (e) {
              this.pushLog(errorText(e));
            }
          },
        },
        "Stop learning",
      ),
      el("button", { onclick: () => void this.refresh() }, "Refresh"),
    );

    const gate = el(
      "div",
      { class: "card" },
      el("h3", null, "Scheduler"),
      el("p", null, gateText(s.learning_gate)),
      s.current_job ? el("p", null, el("strong", null, "Working on: "), s.current_job) : el("p", { class: "muted" }, "No job running."),
      el("p", { class: "muted small" }, `Last user activity ${fmtRelative(s.last_user_activity)} - up ${fmtDuration(s.uptime_secs)}`),
    );

    const state = el(
      "div",
      { class: "card" },
      el("h3", null, "Internal state"),
      bar(s.state.curiosity, 1, "Curiosity", "tone-curiosity"),
      bar(s.state.focus, 1, "Focus", "tone-focus"),
      bar(s.state.energy, 1, "Energy", "tone-energy"),
      bar(s.state.confidence, 1, "Confidence", "tone-confidence"),
      el("p", { class: "muted small" }, "Deterministic variables updated by events, never written by the model."),
    );

    const budgets = el("div", { class: "card" }, el("h3", null, "Today's budgets"));
    for (const b of s.budgets) {
      const ratio = b.limit > 0 ? b.used / b.limit : 0;
      budgets.appendChild(bar(b.used, b.limit, budgetLabel(b), ratio > 0.9 ? "tone-bad" : undefined));
    }

    const counts = el(
      "div",
      { class: "card" },
      el("h3", null, "Memory"),
      el("div", { class: "stats" },
        stat(s.total_episodes, "episodes"),
        stat(s.unconsolidated_episodes, "awaiting consolidation"),
        stat(s.open_questions, "open questions"),
        stat(s.notes, "research notes"),
      ),
    );

    const recent = el("div", { class: "card" }, el("h3", null, "Recent episodes"));
    if (this.recent.length === 0) {
      recent.appendChild(el("p", { class: "muted" }, "Nothing yet. Say hello in Chat."));
    } else {
      const list = el("ul", { class: "plain" });
      for (const ep of this.recent) {
        list.appendChild(
          el(
            "li",
            null,
            pill(ep.kind, `kind-${ep.kind}`),
            " ",
            el("span", { class: "muted small" }, fmtTime(ep.started_at)),
            " ",
            ep.summary || el("span", { class: "muted" }, "(no summary)"),
          ),
        );
      }
      recent.appendChild(list);
    }

    const log = el("div", { class: "card" }, el("h3", null, "Live log"));
    if (this.log.length === 0) {
      log.appendChild(el("p", { class: "muted" }, "Events will appear here as the mind works."));
    } else {
      log.appendChild(el("pre", { class: "log" }, this.log.join("\n")));
    }

    this.root.append(header, controls, el("div", { class: "grid two" }, gate, state, budgets, counts), recent, log);
  }
}

function stat(value: number, label: string): HTMLElement {
  return el("div", { class: "stat" }, el("div", { class: "stat-value" }, String(value)), el("div", { class: "stat-label" }, label));
}
