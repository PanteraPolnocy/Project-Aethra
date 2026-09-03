import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type {
  ChangeRow,
  ChatReply,
  ConfigView,
  Constitution,
  EpisodeItem,
  EpisodeRow,
  MindEvent,
  MindStatus,
  Note,
  Question,
  SelfModelSection,
  Summary,
} from "./types";

const EVENT_CHANNEL = "aethra://event";

export const api = {
  status: () => invoke<MindStatus>("get_status"),
  chat: (text: string) => invoke<ChatReply>("chat_send", { text }),
  timeline: (limit: number, before?: string) =>
    invoke<EpisodeRow[]>("get_timeline", { limit, before: before ?? null }),
  episodeItems: (episodeId: string) => invoke<EpisodeItem[]>("get_episode_items", { episodeId }),
  questions: (status: string | null, limit: number) =>
    invoke<Question[]>("get_questions", { status, limit }),
  addQuestion: (text: string) => invoke<Question | null>("add_question", { text }),
  retireQuestion: (id: string) => invoke<void>("retire_question", { id }),
  notes: (limit: number) => invoke<Note[]>("get_notes", { limit }),
  summaries: (limit: number) => invoke<Summary[]>("get_summaries", { limit }),
  selfModel: () => invoke<SelfModelSection[]>("get_self_model"),
  constitution: () => invoke<Constitution>("get_constitution"),
  setConstitution: (text: string) => invoke<Constitution>("set_constitution", { text }),
  changes: (limit: number) => invoke<ChangeRow[]>("get_changes", { limit }),
  requestLearning: () => invoke<void>("request_learning"),
  stopLearning: () => invoke<void>("stop_learning"),
  touchActivity: () => invoke<void>("touch_activity"),
  config: () => invoke<ConfigView>("get_config"),
  createSnapshot: () => invoke<string[]>("create_snapshot"),
  openDataDir: () => invoke<void>("open_data_dir"),
  openConfigFile: () => invoke<void>("open_config_file"),
};

export function onEvent(handler: (ev: MindEvent) => void): Promise<UnlistenFn> {
  return listen<MindEvent>(EVENT_CHANNEL, (e) => handler(e.payload));
}

export function errorText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  try {
    return JSON.stringify(e);
  } catch {
    return String(e);
  }
}
