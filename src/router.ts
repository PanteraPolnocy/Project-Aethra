import type { MindEvent, MindStatus } from "./types";

export type Route = "now" | "chat" | "timeline" | "questions" | "knowledge" | "self" | "settings";

export const ROUTES: ReadonlyArray<{ id: Route; label: string }> = [
  { id: "now", label: "Now" },
  { id: "chat", label: "Chat" },
  { id: "timeline", label: "Timeline" },
  { id: "questions", label: "Questions" },
  { id: "knowledge", label: "Knowledge" },
  { id: "self", label: "Self" },
  { id: "settings", label: "Settings" },
];

export interface View {
  mount(root: HTMLElement): void | Promise<void>;
  unmount?(): void;
  onEvent?(ev: MindEvent): void;
  onStatus?(status: MindStatus): void;
}

export function currentRoute(): Route {
  const raw = location.hash.replace(/^#\/?/, "");
  const found = ROUTES.find((r) => r.id === raw);
  return found ? found.id : "now";
}

export function navigate(route: Route): void {
  location.hash = `#/${route}`;
}
