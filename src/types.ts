// Mirrors the serde output of the Rust types in aethra-core. Keep in sync by hand
// for now; a generated binding step can replace this file later.

export type Mode = "idle" | "chat" | "learning";

export type LearningGate =
  | { kind: "allowed"; reason: string }
  | { kind: "outside_window"; window: string }
  | { kind: "user_active"; quiet_for_secs: number; required_secs: number }
  | { kind: "budget_exhausted"; reason: string }
  | { kind: "manually_stopped" };

export interface InternalState {
  curiosity: number;
  focus: number;
  energy: number;
  confidence: number;
}

export type Resource =
  | "learning_tokens"
  | "http_requests"
  | "http_bytes"
  | "learning_minutes"
  | "research_jobs";

export interface BudgetStatus {
  resource: Resource;
  used: number;
  limit: number;
}

export interface MindStatus {
  name: string;
  mode: Mode;
  learning_gate: LearningGate;
  learning_requested: boolean;
  model_reachable: boolean;
  model_loaded_profile: string | null;
  sidecar_managed: boolean;
  state: InternalState;
  budgets: BudgetStatus[];
  open_questions: number;
  unconsolidated_episodes: number;
  total_episodes: number;
  notes: number;
  current_job: string | null;
  last_user_activity: string;
  uptime_secs: number;
  data_dir: string;
  config_path: string;
}

export interface Usage {
  prompt_tokens: number;
  completion_tokens: number;
}

export type Taint = "self" | "user" | "web";

export interface ChatReply {
  text: string;
  episode_id: string;
  tool_uses: string[];
  usage: Usage;
  taint: Taint;
}

export interface EpisodeRow {
  id: string;
  kind: string;
  started_at: string;
  ended_at: string | null;
  summary: string;
  taint: string;
  mode: string;
  job_id: string | null;
  prompt_tokens: number;
  completion_tokens: number;
  outcome: string | null;
  consolidated: boolean;
}

export interface EpisodeItem {
  episode_id: string;
  seq: number;
  role: string;
  content: string;
  tool_name: string | null;
  tool_args: string | null;
  created_at: string;
}

export interface Question {
  id: string;
  text: string;
  origin: string;
  status: string;
  importance: number;
  tractability: number;
  attempts: number;
  created_at: string;
  updated_at: string;
  source_episode_id: string | null;
  notes: string | null;
}

export interface Summary {
  id: string;
  scope: string;
  period_start: string;
  period_end: string;
  text: string;
  episode_count: number;
  created_at: string;
}

export interface NoteSource {
  url: string;
  content_hash: string;
  fetched_at: string;
  title: string | null;
}

export interface Note {
  id: string;
  kind: string;
  question_id: string | null;
  title: string;
  text: string;
  confidence: string;
  sources: NoteSource[];
  episode_id: string | null;
  created_at: string;
}

export interface SelfModelSection {
  section: string;
  content: string;
  version: number;
  updated_at: string;
}

export interface Constitution {
  text: string;
  version: number;
  created_at: string;
  updated_at: string;
}

export interface ChangeRow {
  id: number;
  at: string;
  tier: string;
  target_table: string;
  target_id: string;
  before_json: string | null;
  after_json: string | null;
  reason: string;
  trigger_episode_id: string | null;
  approved_by: string;
}

export interface ConfigView {
  path: string;
  summary: Record<string, string>;
  json: string;
}

export type MindEvent =
  | { type: "mode_changed"; mode: Mode }
  | { type: "learning_gate_changed"; gate: LearningGate }
  | { type: "episode_recorded"; episode_id: string; kind: string; summary: string }
  | { type: "job_started"; job_id: string; kind: string; detail: string }
  | { type: "job_finished"; job_id: string; kind: string; outcome: string; success: boolean }
  | { type: "model_status"; reachable: boolean; loaded: boolean; detail: string }
  | { type: "state_changed"; state: InternalState }
  | { type: "log"; level: string; message: string };
