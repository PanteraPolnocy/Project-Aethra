import { api, errorText } from "../api";
import { clear, el, fmtTime, pill } from "../dom";
import type { View } from "../router";
import type { EpisodeItem, MindEvent } from "../types";

interface Bubble {
  role: "user" | "assistant" | "system";
  text: string;
  at?: string;
  meta?: string;
  pending?: boolean;
}

const HISTORY_EPISODES = 10;

export class ChatView implements View {
  private root: HTMLElement | null = null;
  private list: HTMLElement | null = null;
  private input: HTMLTextAreaElement | null = null;
  private sendButton: HTMLButtonElement | null = null;
  private bubbles: Bubble[] = [];
  private busy = false;

  async mount(root: HTMLElement): Promise<void> {
    this.root = root;
    this.renderShell();
    await this.loadHistory();
  }

  unmount(): void {
    this.root = null;
    this.list = null;
    this.input = null;
    this.sendButton = null;
  }

  onEvent(ev: MindEvent): void {
    if (ev.type === "mode_changed" && ev.mode === "learning" && !this.busy) {
      this.push({ role: "system", text: "Learning mode started in the background. Sending a message will pause it." });
    }
  }

  private renderShell(): void {
    if (!this.root) return;
    clear(this.root);
    this.list = el("div", { class: "chat-list" });
    this.input = el("textarea", {
      class: "chat-input",
      placeholder: "Talk to Aethra. Enter sends, Shift+Enter for a new line. Paste a URL and it can read it.",
      rows: "3",
    });
    this.input.addEventListener("keydown", (e: KeyboardEvent) => {
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        void this.send();
      }
    });
    this.sendButton = el("button", { class: "primary", onclick: () => void this.send() }, "Send");
    this.root.append(
      el("div", { class: "chat-header" }, el("h1", null, "Chat"), el("span", { class: "muted small" }, "One continuing conversation. History is memory, not a session.")),
      this.list,
      el("div", { class: "chat-compose" }, this.input, this.sendButton),
    );
  }

  private async loadHistory(): Promise<void> {
    try {
      const episodes = (await api.timeline(60)).filter((e) => e.kind === "conversation").slice(0, HISTORY_EPISODES).reverse();
      const all: EpisodeItem[][] = await Promise.all(episodes.map((e) => api.episodeItems(e.id)));
      this.bubbles = [];
      for (const items of all) {
        for (const it of items) {
          if (it.role === "user" || it.role === "assistant") {
            this.bubbles.push({ role: it.role, text: it.content, at: it.created_at });
          } else if (it.role === "tool") {
            this.bubbles.push({ role: "system", text: `used ${it.tool_name ?? "tool"} ${compactArgs(it.tool_args)}`, at: it.created_at });
          }
        }
      }
      if (this.bubbles.length === 0) {
        this.bubbles.push({ role: "system", text: "No conversation yet. Whatever you say here becomes part of Aethra's memory." });
      }
    } catch (e) {
      this.bubbles = [{ role: "system", text: `Could not load history: ${errorText(e)}` }];
    }
    this.renderList();
  }

  private push(b: Bubble): void {
    this.bubbles.push(b);
    this.renderList();
  }

  private renderList(): void {
    if (!this.list) return;
    clear(this.list);
    for (const b of this.bubbles) {
      const node = el("div", { class: `bubble ${b.role}${b.pending ? " pending" : ""}` });
      if (b.role !== "system") {
        node.appendChild(el("div", { class: "bubble-meta muted small" }, b.role === "user" ? "You" : "Aethra", b.at ? ` - ${fmtTime(b.at)}` : ""));
      }
      node.appendChild(el("div", { class: "bubble-text" }, b.text));
      if (b.meta) node.appendChild(el("div", { class: "bubble-meta muted small" }, b.meta));
      this.list.appendChild(node);
    }
    this.list.scrollTop = this.list.scrollHeight;
  }

  private async send(): Promise<void> {
    if (!this.input || this.busy) return;
    const text = this.input.value.trim();
    if (!text) return;
    this.busy = true;
    this.input.value = "";
    this.input.disabled = true;
    if (this.sendButton) this.sendButton.disabled = true;

    this.push({ role: "user", text, at: new Date().toISOString() });
    const pending: Bubble = { role: "assistant", text: "thinking...", pending: true };
    this.push(pending);

    try {
      const reply = await api.chat(text);
      pending.text = reply.text;
      pending.pending = false;
      pending.at = new Date().toISOString();
      const parts: string[] = [];
      if (reply.tool_uses.length > 0) parts.push(`tools: ${reply.tool_uses.join("; ")}`);
      parts.push(`${reply.usage.prompt_tokens + reply.usage.completion_tokens} tokens`);
      if (reply.taint === "web") parts.push("includes web-sourced material");
      pending.meta = parts.join(" - ");
    } catch (e) {
      pending.text = `I could not answer: ${errorText(e)}`;
      pending.pending = false;
      pending.meta = "error";
    } finally {
      this.busy = false;
      if (this.input) {
        this.input.disabled = false;
        this.input.focus();
      }
      if (this.sendButton) this.sendButton.disabled = false;
      this.renderList();
    }
  }
}

function compactArgs(args: string | null): string {
  if (!args) return "";
  try {
    const v = JSON.parse(args) as Record<string, unknown>;
    if (typeof v.url === "string") return v.url;
    return args.length > 80 ? `${args.slice(0, 80)}...` : args;
  } catch {
    return args.length > 80 ? `${args.slice(0, 80)}...` : args;
  }
}

export function taintPill(t: string): HTMLElement {
  return pill(t, t === "web" ? "tone-bad" : t === "user" ? "tone-chat" : "tone-neutral");
}
