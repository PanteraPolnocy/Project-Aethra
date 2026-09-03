import { api, errorText } from "../api";
import { clear, el, fmtTime, pill, shortId } from "../dom";
import type { View } from "../router";
import type { EpisodeItem, EpisodeRow, MindEvent } from "../types";
import { taintPill } from "./chat";

const PAGE = 40;

export class TimelineView implements View {
  private root: HTMLElement | null = null;
  private listEl: HTMLElement | null = null;
  private rows: EpisodeRow[] = [];
  private filter: string = "all";
  private exhausted = false;
  private expanded = new Map<string, EpisodeItem[]>();

  async mount(root: HTMLElement): Promise<void> {
    this.root = root;
    this.rows = [];
    this.exhausted = false;
    this.renderShell();
    await this.loadMore();
  }

  unmount(): void {
    this.root = null;
    this.listEl = null;
  }

  onEvent(ev: MindEvent): void {
    if (ev.type === "episode_recorded") {
      void this.reload();
    }
  }

  private async reload(): Promise<void> {
    this.rows = [];
    this.exhausted = false;
    await this.loadMore();
  }

  private async loadMore(): Promise<void> {
    if (this.exhausted) return;
    const before = this.rows.length > 0 ? this.rows[this.rows.length - 1].started_at : undefined;
    try {
      const page = await api.timeline(PAGE, before);
      if (page.length < PAGE) this.exhausted = true;
      this.rows.push(...page);
    } catch (e) {
      this.showError(errorText(e));
      return;
    }
    this.renderList();
  }

  private showError(msg: string): void {
    if (this.listEl) {
      clear(this.listEl);
      this.listEl.appendChild(el("p", { class: "error" }, msg));
    }
  }

  private renderShell(): void {
    if (!this.root) return;
    clear(this.root);
    const select = el("select", null,
      el("option", { value: "all" }, "All kinds"),
      el("option", { value: "conversation" }, "Conversations"),
      el("option", { value: "learning" }, "Learning"),
      el("option", { value: "system" }, "System"),
    );
    select.addEventListener("change", () => {
      this.filter = select.value;
      this.renderList();
    });
    this.listEl = el("div", { class: "timeline" });
    this.root.append(
      el("div", { class: "row wrap gap" },
        el("h1", null, "Timeline"),
        el("span", { class: "muted small" }, "Everything the mind did, newest first. Learning episodes appear while you were away."),
      ),
      el("div", { class: "row gap" }, select, el("button", { onclick: () => void this.reload() }, "Refresh")),
      this.listEl,
    );
  }

  private renderList(): void {
    if (!this.listEl) return;
    clear(this.listEl);
    const rows = this.filter === "all" ? this.rows : this.rows.filter((r) => r.kind === this.filter);
    if (rows.length === 0) {
      this.listEl.appendChild(el("p", { class: "muted" }, "No episodes yet."));
    }
    let lastDay = "";
    for (const r of rows) {
      const day = r.started_at.slice(0, 10);
      if (day !== lastDay) {
        lastDay = day;
        this.listEl.appendChild(el("h3", { class: "day-header" }, new Date(`${day}T00:00:00`).toLocaleDateString(undefined, { weekday: "long", year: "numeric", month: "long", day: "numeric" })));
      }
      this.listEl.appendChild(this.renderRow(r));
    }
    if (!this.exhausted && this.filter === "all") {
      this.listEl.appendChild(el("button", { onclick: () => void this.loadMore() }, "Load older"));
    }
  }

  private renderRow(r: EpisodeRow): HTMLElement {
    const details = el("div", { class: "episode-details" });
    const items = this.expanded.get(r.id);
    if (items) this.fillDetails(details, items);

    const toggle = el("button", { class: "link" }, items ? "hide" : "details");
    toggle.addEventListener("click", async () => {
      if (this.expanded.has(r.id)) {
        this.expanded.delete(r.id);
        clear(details);
        toggle.textContent = "details";
        return;
      }
      try {
        const loaded = await api.episodeItems(r.id);
        this.expanded.set(r.id, loaded);
        this.fillDetails(details, loaded);
        toggle.textContent = "hide";
      } catch (e) {
        details.appendChild(el("p", { class: "error" }, errorText(e)));
      }
    });

    const tokens = r.prompt_tokens + r.completion_tokens;
    return el(
      "div",
      { class: `episode kind-${r.kind}` },
      el("div", { class: "row wrap gap small" },
        el("span", { class: "muted" }, fmtTime(r.started_at)),
        pill(r.kind, `kind-${r.kind}`),
        pill(r.mode, "tone-neutral"),
        taintPill(r.taint),
        r.outcome && r.outcome !== "ok" ? pill(r.outcome.length > 60 ? `${r.outcome.slice(0, 60)}...` : r.outcome, "tone-bad") : null,
        tokens > 0 ? el("span", { class: "muted" }, `${tokens} tokens`) : null,
        r.consolidated ? el("span", { class: "muted" }, "consolidated") : null,
        el("span", { class: "muted" }, shortId(r.id)),
        toggle,
      ),
      el("div", { class: "episode-summary" }, r.summary || el("span", { class: "muted" }, "(no summary)")),
      details,
    );
  }

  private fillDetails(container: HTMLElement, items: EpisodeItem[]): void {
    clear(container);
    if (items.length === 0) {
      container.appendChild(el("p", { class: "muted small" }, "No items recorded."));
      return;
    }
    for (const it of items) {
      const label = it.tool_name ? `${it.role}:${it.tool_name}` : it.role;
      container.appendChild(
        el("div", { class: `item role-${it.role}` },
          el("div", { class: "muted small" }, label, it.tool_args ? ` ${it.tool_args}` : ""),
          el("pre", { class: "item-content" }, it.content),
        ),
      );
    }
  }
}
