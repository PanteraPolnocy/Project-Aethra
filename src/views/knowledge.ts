import { api, errorText } from "../api";
import { clear, el, fmtTime, pill, shortId } from "../dom";
import type { View } from "../router";
import type { MindEvent, Note, Summary } from "../types";

export class KnowledgeView implements View {
  private root: HTMLElement | null = null;
  private notes: Note[] = [];
  private summaries: Summary[] = [];
  private error = "";

  async mount(root: HTMLElement): Promise<void> {
    this.root = root;
    await this.reload();
  }

  unmount(): void {
    this.root = null;
  }

  onEvent(ev: MindEvent): void {
    if (ev.type === "job_finished") void this.reload();
  }

  private async reload(): Promise<void> {
    try {
      [this.notes, this.summaries] = await Promise.all([api.notes(100), api.summaries(50)]);
      this.error = "";
    } catch (e) {
      this.error = errorText(e);
    }
    this.render();
  }

  private render(): void {
    if (!this.root) return;
    clear(this.root);
    this.root.appendChild(
      el("div", { class: "row wrap gap" },
        el("h1", null, "Knowledge"),
        el("span", { class: "muted small" }, "Research notes carry their sources; consolidated memory is what Aethra remembers about its days."),
        el("button", { onclick: () => void this.reload() }, "Refresh"),
      ),
    );
    if (this.error) this.root.appendChild(el("p", { class: "error" }, this.error));

    const notes = el("section", null, el("h2", null, `Research notes (${this.notes.length})`));
    if (this.notes.length === 0) {
      notes.appendChild(el("p", { class: "muted" }, "No notes yet. Notes are written when learning mode researches an open question."));
    }
    for (const n of this.notes) notes.appendChild(renderNote(n));

    const summaries = el("section", null, el("h2", null, `Consolidated memory (${this.summaries.length})`));
    if (this.summaries.length === 0) {
      summaries.appendChild(el("p", { class: "muted" }, "No consolidations yet. They run in learning mode once there are finished episodes."));
    }
    for (const s of this.summaries) {
      summaries.appendChild(
        el("div", { class: "card" },
          el("div", { class: "row wrap gap small muted" },
            pill(s.scope, "tone-neutral"),
            el("span", null, `${fmtTime(s.period_start)} to ${fmtTime(s.period_end)}`),
            el("span", null, `${s.episode_count} episodes`),
          ),
          el("pre", { class: "prose" }, s.text),
        ),
      );
    }

    this.root.append(el("div", { class: "grid two" }, notes, summaries));
  }
}

function renderNote(n: Note): HTMLElement {
  const body = el("pre", { class: "prose" }, n.text);
  body.hidden = true;
  const toggle = el("button", { class: "link" }, "show");
  toggle.addEventListener("click", () => {
    body.hidden = !body.hidden;
    toggle.textContent = body.hidden ? "show" : "hide";
  });
  const sources = el("ul", { class: "plain small" });
  for (const s of n.sources) {
    sources.appendChild(el("li", null, el("span", { class: "mono" }, s.url), s.title ? ` - ${s.title}` : "", el("span", { class: "muted" }, ` (fetched ${fmtTime(s.fetched_at)}, hash ${shortId(s.content_hash)})`)));
  }
  return el("div", { class: "card note" },
    el("div", { class: "row wrap gap" }, el("strong", null, n.title), toggle),
    el("div", { class: "muted small" }, `${fmtTime(n.created_at)} - ${n.confidence}`),
    body,
    n.sources.length > 0 ? el("div", null, el("div", { class: "muted small" }, "Sources"), sources) : el("div", { class: "muted small" }, "No sources recorded."),
  );
}
