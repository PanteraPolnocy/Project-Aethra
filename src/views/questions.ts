import { api, errorText } from "../api";
import { clear, el, fmtRelative, pill } from "../dom";
import type { View } from "../router";
import type { MindEvent, Question } from "../types";

export class QuestionsView implements View {
  private root: HTMLElement | null = null;
  private listEl: HTMLElement | null = null;
  private status: string | null = "open";
  private questions: Question[] = [];
  private message = "";

  async mount(root: HTMLElement): Promise<void> {
    this.root = root;
    this.renderShell();
    await this.reload();
  }

  unmount(): void {
    this.root = null;
    this.listEl = null;
  }

  onEvent(ev: MindEvent): void {
    if (ev.type === "job_finished") void this.reload();
  }

  private async reload(): Promise<void> {
    try {
      this.questions = await api.questions(this.status, 200);
      this.message = "";
    } catch (e) {
      this.message = errorText(e);
    }
    this.renderList();
  }

  private renderShell(): void {
    if (!this.root) return;
    clear(this.root);
    const input = el("input", { type: "text", placeholder: "Add something you want Aethra to look into", class: "grow" });
    const add = async () => {
      const text = input.value.trim();
      if (!text) return;
      try {
        const q = await api.addQuestion(text);
        this.message = q ? "Question added." : "An equivalent open question already exists.";
        input.value = "";
        await this.reload();
      } catch (e) {
        this.message = errorText(e);
        this.renderList();
      }
    };
    input.addEventListener("keydown", (e: KeyboardEvent) => {
      if (e.key === "Enter") void add();
    });
    const select = el("select", null,
      el("option", { value: "open" }, "Open"),
      el("option", { value: "investigating" }, "Investigating"),
      el("option", { value: "investigated" }, "Investigated"),
      el("option", { value: "retired" }, "Retired"),
      el("option", { value: "" }, "All"),
    );
    select.addEventListener("change", () => {
      this.status = select.value === "" ? null : select.value;
      void this.reload();
    });
    this.listEl = el("div", { class: "questions" });
    this.root.append(
      el("div", { class: "row wrap gap" },
        el("h1", null, "Questions"),
        el("span", { class: "muted small" }, "The curiosity queue. Learning mode picks the highest importance x tractability first."),
      ),
      el("div", { class: "row gap" }, input, el("button", { class: "primary", onclick: () => void add() }, "Add"), select),
      this.listEl,
    );
  }

  private renderList(): void {
    if (!this.listEl) return;
    clear(this.listEl);
    if (this.message) this.listEl.appendChild(el("p", { class: "muted" }, this.message));
    if (this.questions.length === 0) {
      this.listEl.appendChild(el("p", { class: "muted" }, "Nothing here. Questions appear after consolidation runs, or add one above."));
      return;
    }
    for (const q of this.questions) {
      const retire = el("button", { class: "link" }, "retire");
      retire.addEventListener("click", async () => {
        try {
          await api.retireQuestion(q.id);
          await this.reload();
        } catch (e) {
          this.message = errorText(e);
          this.renderList();
        }
      });
      this.listEl.appendChild(
        el("div", { class: "question" },
          el("div", { class: "question-text" }, q.text),
          el("div", { class: "row wrap gap small muted" },
            pill(q.status, `status-${q.status}`),
            pill(`from ${q.origin}`, "tone-neutral"),
            el("span", null, `importance ${q.importance.toFixed(2)}`),
            el("span", null, `tractability ${q.tractability.toFixed(2)}`),
            el("span", null, `${q.attempts} attempt${q.attempts === 1 ? "" : "s"}`),
            el("span", null, fmtRelative(q.updated_at)),
            q.status !== "retired" ? retire : null,
          ),
          q.notes ? el("div", { class: "muted small" }, q.notes) : null,
        ),
      );
    }
  }
}
