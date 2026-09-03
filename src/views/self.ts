import { api, errorText } from "../api";
import { clear, el, fmtTime, pill } from "../dom";
import type { View } from "../router";
import type { ChangeRow, Constitution, MindEvent, SelfModelSection } from "../types";

const CORE_SECTIONS = new Set(["identity", "values", "relationship"]);

export class SelfView implements View {
  private root: HTMLElement | null = null;
  private constitution: Constitution | null = null;
  private sections: SelfModelSection[] = [];
  private changes: ChangeRow[] = [];
  private message = "";

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
      const [constitution, sections, changes] = await Promise.all([api.constitution(), api.selfModel(), api.changes(60)]);
      this.constitution = constitution;
      this.sections = sections;
      this.changes = changes;
    } catch (e) {
      this.message = errorText(e);
    }
    this.render();
  }

  private render(): void {
    if (!this.root) return;
    clear(this.root);
    this.root.appendChild(el("h1", null, "Self"));
    if (this.message) this.root.appendChild(el("p", { class: "muted" }, this.message));

    // Constitution: user-owned, editable here and nowhere else.
    const textarea = el("textarea", { class: "constitution", rows: "14" });
    textarea.value = this.constitution?.text ?? "";
    const save = el("button", { class: "primary" }, "Save constitution");
    save.addEventListener("click", async () => {
      try {
        this.constitution = await api.setConstitution(textarea.value);
        this.message = `Constitution saved as version ${this.constitution.version}.`;
        await this.reload();
      } catch (e) {
        this.message = errorText(e);
        this.render();
      }
    });
    const constitutionCard = el("div", { class: "card" },
      el("div", { class: "row wrap gap" },
        el("h3", null, "Constitution"),
        this.constitution ? el("span", { class: "muted small" }, `version ${this.constitution.version}, updated ${fmtTime(this.constitution.updated_at)}`) : null,
      ),
      el("p", { class: "muted small" }, "Root goals and hard constraints. Only you can edit this; the mind can read it and ask you to change it. Every edit is recorded as a Tier C change."),
      textarea,
      el("div", { class: "row gap" }, save),
    );

    const selfModel = el("div", { class: "card" }, el("h3", null, "Self-model"),
      el("p", { class: "muted small" }, "Core sections (identity, values, relationship) are Tier C: user only. The rest are Tier B: the mind may revise them through reflection, with history kept."));
    for (const s of this.sections) {
      selfModel.appendChild(
        el("div", { class: "section" },
          el("div", { class: "row wrap gap" },
            el("strong", null, s.section),
            pill(CORE_SECTIONS.has(s.section) ? "tier C" : "tier B", CORE_SECTIONS.has(s.section) ? "tone-bad" : "tone-neutral"),
            el("span", { class: "muted small" }, `v${s.version}, ${fmtTime(s.updated_at)}`),
          ),
          el("p", null, s.content),
        ),
      );
    }

    const changes = el("div", { class: "card" }, el("h3", null, "Recent changes"),
      el("p", { class: "muted small" }, "Append-only audit of every write to persistent state that is not a plain episode."));
    if (this.changes.length === 0) {
      changes.appendChild(el("p", { class: "muted" }, "No changes recorded yet."));
    } else {
      const table = el("table", { class: "changes" },
        el("thead", null, el("tr", null, el("th", null, "When"), el("th", null, "Tier"), el("th", null, "Target"), el("th", null, "Reason"), el("th", null, "By"))));
      const tbody = el("tbody");
      for (const c of this.changes) {
        tbody.appendChild(el("tr", null,
          el("td", { class: "muted small" }, fmtTime(c.at)),
          el("td", null, pill(c.tier, c.tier === "C" ? "tone-bad" : c.tier === "B" ? "tone-learning" : "tone-neutral")),
          el("td", { class: "mono small" }, `${c.target_table}/${c.target_id.slice(0, 8)}`),
          el("td", null, c.reason),
          el("td", { class: "small" }, c.approved_by),
        ));
      }
      table.appendChild(tbody);
      changes.appendChild(table);
    }

    this.root.append(el("div", { class: "grid two" }, constitutionCard, selfModel), changes);
  }
}
