import { api, errorText } from "../api";
import { clear, el } from "../dom";
import type { View } from "../router";
import type { ConfigView } from "../types";

export class SettingsView implements View {
  private root: HTMLElement | null = null;
  private config: ConfigView | null = null;
  private message = "";

  async mount(root: HTMLElement): Promise<void> {
    this.root = root;
    try {
      this.config = await api.config();
    } catch (e) {
      this.message = errorText(e);
    }
    this.render();
  }

  unmount(): void {
    this.root = null;
  }

  private render(): void {
    if (!this.root) return;
    clear(this.root);
    this.root.appendChild(el("h1", null, "Settings"));
    this.root.appendChild(
      el("p", { class: "muted" },
        "Configuration lives in a file the mind cannot write. Edit it, then restart Aethra from the tray. ",
        "Boundaries (network domains, budgets, model launch profiles) are yours to change, not the agent's."),
    );
    if (this.message) this.root.appendChild(el("p", { class: "error" }, this.message));

    const actions = el("div", { class: "row wrap gap" },
      el("button", { class: "primary", onclick: () => void this.run(api.openConfigFile, "Opened config file.") }, "Open config file"),
      el("button", { onclick: () => void this.run(api.openDataDir, "Opened data directory.") }, "Open data directory"),
      el("button", { onclick: () => void this.snapshot() }, "Create snapshot now"),
    );
    this.root.appendChild(actions);

    if (this.config) {
      this.root.appendChild(el("p", { class: "mono small" }, this.config.path));
      const table = el("table", { class: "kv" });
      for (const [k, v] of Object.entries(this.config.summary)) {
        table.appendChild(el("tr", null, el("td", { class: "mono" }, k), el("td", null, v)));
      }
      this.root.appendChild(el("div", { class: "card" }, el("h3", null, "Effective settings"), table));
      const details = el("details", null, el("summary", null, "Full configuration (secrets redacted)"), el("pre", { class: "mono small" }, this.config.json));
      this.root.appendChild(el("div", { class: "card" }, details));
    }

    this.root.appendChild(
      el("div", { class: "card" },
        el("h3", null, "Running a local model"),
        el("p", null,
          "Aethra talks to any OpenAI-compatible server. The intended setup is llama.cpp's llama-server on the CPU: ",
          "set models.sidecar.enabled = true, models.sidecar.executable to the llama-server binary and models.chat.model_path to a GGUF file. ",
          "With 64 GB of RAM and no GPU use, a mixture-of-experts model such as Qwen3.5-35B-A3B at Q4 gives the best quality per token per second; ",
          "a dense 4B model at Q8 is the fast fallback. See README.md for the exact steps."),
      ),
    );
  }

  private async run(fn: () => Promise<void>, ok: string): Promise<void> {
    try {
      await fn();
      this.message = ok;
    } catch (e) {
      this.message = errorText(e);
    }
    this.render();
  }

  private async snapshot(): Promise<void> {
    try {
      const paths = await api.createSnapshot();
      this.message = `Snapshot written: ${paths.join(", ")}`;
    } catch (e) {
      this.message = errorText(e);
    }
    this.render();
  }
}
