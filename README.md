# Project-Aethra
An experiment in building a persistent artificial mind - one that can learn, remember, explore, reflect, and evolve over time... maybe.

## What it is

Aethra is a desktop application (Tauri 2, Rust, TypeScript) that hosts a persistent agent. The language model is a
replaceable reasoning engine; everything that makes Aethra *Aethra* - constitution, self-model, episodes, questions,
notes, internal state, audit log - lives in SQLite files you own and can inspect.

Two modes:

- **Chat** - you talk, it answers. It may read web pages you link or pages on an allowlist. Every turn becomes an episode.
- **Learning** - runs overnight (default 01:00-07:00 local, after 20 quiet minutes) or when you press *Learn now*.
  It consolidates recent episodes into first-person memory and open questions, then researches the most promising
  question from allowlisted sources, writing notes whose quotes are verified against the cached page text.
  A chat message pre-empts learning immediately.

Closing the window hides the app to the tray; the mind keeps running. *Quit* in the tray menu is the only way out.

## Repository layout

```
Cargo.toml                 workspace
crates/aethra-models/      LanguageModel trait, OpenAI-compatible client, llama-server sidecar supervision
crates/aethra-core/        the mind: config, SQLite storage, identity, episodes, knowledge, policy, tools, jobs, scheduler
src-tauri/                 desktop shell: tray, hide-on-close, single instance, IPC commands
src/                       frontend (vanilla TypeScript, no framework yet): Now, Chat, Timeline, Questions, Knowledge, Self, Settings
```

## Getting started

Prerequisites: Rust stable, Node 24, and the Tauri 2 platform dependencies for your OS.

1. `npm install`
2. `npm run tauri dev` - first start writes `config.toml` under your data directory
   (`%APPDATA%\Aethra\` on Windows, `~/.local/share/Aethra/` on Linux) and creates `data/mind.db`,
   `data/episodes.db`, `data/cache.db`.
3. Point it at a model (below), restart, say hello in *Chat*.

### Closing, quitting, reloading

- **Closing the window hides Aethra to the tray.** The mind keeps running. Left-click the tray icon to bring the window
  back.
- **Quit** is in the tray menu. It stops the scheduler, closes the databases and kills the inference server.
- **`config.toml` is read once, at start.** After editing it use the tray's *Restart (reload config)*. In a `tauri dev`
  build that item quits instead (the Tauri CLI stops Vite when the app exits), so run `npm run tauri dev` again.
- **Do not launch `target\debug\project-aethra.exe` by hand.** A dev binary loads its UI from Vite on port 1420 and shows
  "localhost refused to connect" without it. For a standalone executable use `npm run tauri build`; it lands under
  `target\release\`.

### Local model (CPU-first)

Aethra speaks to any OpenAI-compatible server. The intended setup is [llama.cpp](https://github.com/ggml-org/llama.cpp)'s
`llama-server`, kept on the CPU so the GPU stays free for other work. Two downloads, one config edit. Neither the
server nor the model needs to live on the system drive; the examples below use `D:\`.

#### 1. llama-server

Grab the latest archive from the [llama.cpp releases](https://github.com/ggml-org/llama.cpp/releases) page. Builds
ship several times a day; the build number in the filename does not matter, the variant does:

| Hardware plan | Archive | Notes |
| --- | --- | --- |
| CPU only (the default here) | `llama-<build>-bin-win-cpu-x64.zip` | ~16 MB. All you need while `gpu_layers = 0`. |
| NVIDIA, partial offload later | `llama-<build>-bin-win-cuda-12.4-x64.zip` **and** `cudart-llama-bin-win-cuda-12.4-x64.zip` | Unzip both into the same folder; the second one is the CUDA runtime. Only worth it once you raise `models.chat.gpu_layers`. |
| Any GPU incl. AMD/Intel | `llama-<build>-bin-win-vulkan-x64.zip` | Slower than CUDA on NVIDIA; skip unless CUDA fails. |

Unzip anywhere, e.g. `D:\llama\`. The file Aethra needs is `llama-server.exe`; the rest of the folder is its DLLs and
tools, keep them together. Check it runs before wiring it in:

```powershell
D:\llama\llama-server.exe --version
```

#### 2. Model

Recommendations for a 64 GB RAM machine with the GPU kept free:

| Model | File | Size | Character |
| --- | --- | --- | --- |
| **Qwen3.5-35B-A3B** (MoE, 3B active) | `Qwen3.5-35B-A3B-UD-Q4_K_XL.gguf` from [unsloth/Qwen3.5-35B-A3B-GGUF](https://huggingface.co/unsloth/Qwen3.5-35B-A3B-GGUF) | 22.2 GB | Best quality per second on a CPU; 10-15 tok/s on a 12th-gen laptop core. Needs ~24 GB RAM resident. |
| **Qwen3.5-4B** | `Qwen3.5-4B-Q8_0.gguf` from [unsloth/Qwen3.5-4B-GGUF](https://huggingface.co/unsloth/Qwen3.5-4B-GGUF) | 4.5 GB | Faster, shallower. Good for checking the plumbing before the big download lands. |

`UD-` files are Unsloth's dynamic quants (per-tensor bit widths chosen against a calibration set), which is why they
beat a plain `Q4_K_M` of the same size. The 35B `UD-Q4_K_XL` was re-uploaded on 27 Feb 2026 to fix a recipe bug;
anything fetched after that date is the corrected file.

Put the `.gguf` in a folder of your choosing, e.g. `D:\models\`. Ways to get it there:

- **Browser.** Open the repo's *Files* tab, click the download icon next to the file. Simple, but a 22 GB browser
  download does not resume if it drops.
- **Hugging Face CLI** (resumable, hash-verified, lands exactly where you say):

  ```powershell
  pip install -U "huggingface_hub[cli]"
  hf download unsloth/Qwen3.5-35B-A3B-GGUF Qwen3.5-35B-A3B-UD-Q4_K_XL.gguf --local-dir D:\models
  ```

- **Let llama-server fetch it.** Recent builds accept a Hugging Face reference and cache the file locally:

  ```powershell
  D:\llama\llama-server.exe -hf unsloth/Qwen3.5-35B-A3B-GGUF:UD-Q4_K_XL
  ```

  It lands under `%LOCALAPPDATA%\llama.cpp\` on the system drive with a mangled name. Fine for a test; for the real
  setup move it to `D:\models\` and rename it back, or use the CLI above.

Optional sanity check before involving Aethra - load the model once by hand and watch the log. If this works, the
config below will work:

```powershell
D:\llama\llama-server.exe --model D:\models\Qwen3.5-35B-A3B-UD-Q4_K_XL.gguf --ctx-size 16384 --threads 8 -ngl 0 --jinja
```

Expect a couple of minutes of disk reads the first time; `Ctrl+C` once it says it is listening.

#### 3. config.toml

The file is at `%APPDATA%\Aethra\config.toml` (created with defaults on first run). Edit these sections:

```toml
[models.sidecar]
enabled = true
executable = 'D:\llama\llama-server.exe'
port = 8080
startup_timeout_secs = 300

[models.chat]
model_path = 'D:\models\Qwen3.5-35B-A3B-UD-Q4_K_XL.gguf'
ctx_size = 16384
threads = 8
gpu_layers = 0

[models.learning]
ctx_size = 16384
threads = 8
gpu_layers = 0
```

Single quotes keep TOML from treating the backslashes as escapes. Aethra passes `--model`, `--ctx-size`, `--threads`,
`-ngl` and `--jinja` from these fields; anything else goes in `extra_args`.

`models.learning.model_path` may be left unset to reuse the chat model. Keep the two profiles identical unless you
mean it: switching between chat and learning only restarts llama-server when the launch arguments differ, and a cold
load of a 22 GB model takes minutes. If you would rather run the server yourself (or use Ollama, mistral.rs, etc.),
leave `sidecar.enabled = false` and set `models.endpoint`.

Useful `extra_args` for llama-server, depending on your build: `["--reasoning-budget", "0"]` disables thinking for
faster CPU turns; `["--cache-type-k", "q8_0", "--cache-type-v", "q8_0"]` halves KV-cache memory.

### Configuration is a boundary

`config.toml` is read once at startup and never written by the agent. Network domains, budgets, learning window and
model launch profiles are yours. The mind can ask you to change them; it cannot do so itself. There is no tool for it.

## Scripts

- `npm run tauri dev` - run the app
- `npm run typecheck` - TypeScript check
- `npm run test:rust` - Rust unit tests for the core and model crates (`cargo test --workspace --exclude project-aethra`)

## Status

Phase 0 of the roadmap: one entity that survives closing the window, remembers conversations, learns overnight from
what it experienced, and is fully inspectable. Knowledge claims with provenance, hybrid retrieval, goals, heuristics
and the code sandbox are later phases.
