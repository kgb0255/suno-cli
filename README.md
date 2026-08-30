<div align="center">

# suno

**Write and generate AI music from your terminal — full Suno v5.5 support**

<br />

[![Star this repo](https://img.shields.io/github/stars/paperfoot/suno-cli?style=for-the-badge&logo=github&label=%E2%AD%90%20Star%20this%20repo&color=yellow)](https://github.com/paperfoot/suno-cli/stargazers)
&nbsp;&nbsp;
[![Follow @longevityboris](https://img.shields.io/badge/Follow_%40longevityboris-000000?style=for-the-badge&logo=x&logoColor=white)](https://x.com/longevityboris)

<br />

[![License: MIT](https://img.shields.io/badge/License-MIT-blue?style=for-the-badge)](LICENSE)
&nbsp;
[![Rust](https://img.shields.io/badge/Rust-2024-orange?style=for-the-badge&logo=rust)](https://www.rust-lang.org/)
&nbsp;
[![crates.io](https://img.shields.io/crates/v/suno?style=for-the-badge)](https://crates.io/crates/suno)
&nbsp;
[![PRs Welcome](https://img.shields.io/badge/PRs-welcome-brightgreen?style=for-the-badge)](https://github.com/paperfoot/suno-cli/pulls)

---

A single Rust binary that talks directly to Suno's API. Generate songs with custom lyrics, style tags, your own voice persona, vocal control, weirdness/style sliders, covers, remasters, and every v5.5 feature. Zero-friction auth — one command extracts credentials from your browser automatically.

[Install](#install) | [Quick Start](#quick-start) | [Commands](#commands) | [Features](#features) | [Contributing](#contributing)

</div>

## Why

Suno has no official API. The web UI works, but you can't script it, pipe lyrics from a file, batch-generate, or integrate it into a music production workflow.

This CLI fixes that. Auto-auth from your browser, every generation parameter exposed as a flag, dual JSON/table output for both humans and AI agents. Downloads auto-embed synced lyrics into MP3 files.

## Install

### Homebrew (macOS/Linux)

```bash
brew tap paperfoot/tap
brew install suno
```

### Cargo (any platform)

```bash
cargo install suno
```

### Pre-built binaries

Download from [GitHub Releases](https://github.com/paperfoot/suno-cli/releases) — binaries for macOS (Apple Silicon + Intel), Linux (x86_64 + ARM), and Windows.

### Updating

`suno update` is distribution-aware: it detects how the binary was installed and never overwrites a package-manager-owned install.

```bash
suno update --check    # see what's available (JSON when piped)
suno update            # standalone installs: self-replace from GitHub Releases
                       # brew/cargo installs: prints the right upgrade command instead
```

| Install source | What `suno update` does |
|---|---|
| Homebrew | Never self-replaces — tells you to run `brew upgrade paperfoot/tap/suno` |
| Cargo | Never self-replaces — tells you to run `cargo install --locked --force suno` |
| Standalone binary | Downloads the latest GitHub release over HTTPS and swaps it in |
| Unrecognized | Fails closed (exit 2) rather than risk overwriting a package-manager binary — reinstall from a known channel |

Standalone self-update fetches the release asset from GitHub over HTTPS. Signed-artifact / attestation verification of the downloaded binary is a tracked follow-up (see [Known limitations](#known-limitations)).

After updating, run `suno skill install` to refresh the agent skill.

## Quick Start

```bash
# 1. Authenticate (auto-extracts from Chrome/Arc/Brave/Firefox/Edge)
suno auth --login

# 2. Verify the setup end to end (auth, JWT freshness, Chrome, API reach, credits)
suno doctor

# 3. Check your credits
suno credits

# 4. Write a song — the composer scaffolds it, you fill the <...> lyric slots
suno write --genre "indie rock" --theme "late-night city drives" --vocal male --out song.txt

# 5. Generate the audio from the file you just filled (write prints this exact command)
suno generate \
  --title "Night Drive" \
  --tags "Indie rock, jangly and nostalgic, 110 BPM, warm male vocals, clean guitars, driving drums" \
  --lyrics-file song.txt \
  --wait --download ./songs/

# 6. Generate with your voice persona
suno generate \
  --title "My Song" \
  --tags "pop, warm" \
  --persona e483d2f0-50ca-4a09-8a74-b9e074646377 \
  --lyrics "[Verse]\nHello from the CLI"

# 7. Or skip the composer and let Suno write the lyrics from a description
suno describe --prompt "a chill lo-fi track about rainy mornings" --wait
```

Generation costs ~10 credits per call on v5.5 (5 per clip, 2 clips per call — measured against `suno credits` before and after a call on 2026-08-30). Older models are cheaper; `suno lyrics` is free. Check `suno models` for what your plan can use.

## Write a song

`suno write` is the way to compose. It assembles a Suno-ready song scaffold from a genre grammar compiled into the binary — a Style Prompt line, a meta-tagged `[Verse]`/`[Chorus]` skeleton with inline `<...>` lyric placeholders, and a Suno Tags line — then hands you the exact `suno generate` command to run. The grammar is executable, so you never hand-assemble a style prompt: run one command, fill the `<...>` slots, generate.

```bash
# 1. Scaffold the song (free, no credits) — --out writes the lyric block to a file
suno write --genre "indie rock" --theme "late-night city drives" --vocal male --viral --out song.txt

# 2. Fill the <...> lyric lines in song.txt, then run the command `write` printed
suno generate --title "..." --tags "..." --lyrics-file song.txt --wait --download ./songs/
```

`--out` writes the **lyric block only** — no title, no style prompt, no tag list — so the file feeds `generate --lyrics-file` directly and nothing but lyrics reaches the model. Bare `suno write` at a terminal prints that same lyric block to stdout, so copy-paste is always safe too. The title, Style Prompt and Suno Tags go to stderr (human mode) and into the JSON envelope; `--project-out FILE` additionally saves the full composite document for humans (it must be a different path than `--out`). `suno generate` and `suno extend` refuse lyrics that still contain `<...>` scaffold placeholders — even spans split across lines (exit 3, naming the line numbers) — so an unfilled draft can never burn credits; `--force` overrides both that preflight and the duplicate-run guard.

Note that shell redirection (`suno write > song.txt`) receives the JSON envelope, not lyrics: output is a JSON envelope whenever stdout is not a terminal. `--out` is the way to get an editable lyrics file.

Fuzzy genre matching covers ~24 subgenres; an unknown genre is passed through verbatim as a style tag, so `write` never fails on input. Piped or with `--json` you get a `{title, mode, genre, style_prompt, structure, suno_tags, structure_tags, bpm, vocal, theme, viral, instrumental, placeholders_remaining, ready_to_generate, missing_requirements, next_action, written}` envelope. `next_action.argv` is the authoritative handoff — run it as argv, never shell-parse `next_action.command`. It is `null` until `--out` names a real file, and the emitted command omits `--model` so your configured default applies (v5.5, Suno's latest, out of the box).

### Priming / research songs

`--mode priming` swaps in a chill-lounge, low-arousal scaffold (72 BPM) and appends a Prime-Stack Map table plus a research-artefact block:

```bash
suno write --mode priming \
  --target "anonymised batch (n=40)" \
  --objective "increase recall of brand X" \
  --domain marketing --subtlety stealth --out song.txt
```

Priming is consent-based, so `--target`, `--objective` and `--domain` are required: an incomplete request exits 3 with the missing flags named, rather than emitting a scaffold and a ready-to-run command. The objective also seeds the song theme. The Prime-Stack Map and research artefact stay out of the lyrics file — they live in the JSON envelope and `--project-out`.

The deep references live in the built-in guides: `suno guide songwriting` for the full grammar, `suno guide priming` for the consent frame, evidence-graded prime library, and quality gates.

| Flag | What it does | Values |
|---|---|---|
| `--theme` | What the song is about | free text |
| `--genre` | Genre/subgenre | fuzzy match; unknown → verbatim style tag |
| `--mood` | Mood override | e.g. `"bittersweet and hopeful"` (else genre default) |
| `--vocal` | Vocal gender direction | male, female |
| `--bpm` | Tempo | number (else the genre's default) |
| `--viral` | Add earworm/hook meta-tags | flag |
| `--instrumental` | No vocals, no lyric placeholders; adds `--instrumental` to the emitted command | flag |
| `--title` | Song title | free text (else derived from theme) |
| `--mode` | Composition mode | songwriting (default), priming |
| `--target` / `--objective` / `--domain` | Priming research fields | required with `--mode priming` |
| `--subtlety` | Priming subtlety dial | stealth, medium (default), overt |
| `--out` | Write the lyric block to a file (the generation input) | path |
| `--project-out` | Write the composite human document to a file | path |
| `--download` | Download dir baked into the emitted generate command | path (default `./`) |

## Commands

### Create

```
suno write           Compose a Suno-ready song scaffold from the built-in grammar (free)
suno generate        Custom mode — lyrics + tags + title + sliders + voice persona
suno describe        Description mode — Suno writes lyrics from your prompt
suno lyrics          Generate lyrics only (free, no credits)
suno extend          Continue a clip from a timestamp
suno concat          Stitch clips into a full song
suno cover           Create a cover with different style/model
suno remaster        Remaster with a different model version
suno stems           Extract vocals and instruments
```

### Browse & Inspect

```
suno list            List your songs (--cursor for the next page)
suno search <query>  Search songs by title or tags
suno info <id>       Detailed view of a single clip
suno persona <id>    View a voice persona
suno status <ids>    Check generation progress
suno credits         Show balance and plan info
suno models          List available models with limits
```

### Manage

```
suno download <ids>  Download audio/video with embedded lyrics
suno delete <ids>    Move clips to trash (-y to confirm; --restore undoes it)
suno set <id>        Update title, lyrics, caption, or remove cover
suno publish <ids>   Toggle public/private visibility
suno timed-lyrics    Get word-level timestamped lyrics (--lrc for LRC format)
```

### Config, Auth & Tooling

```
suno auth            Set up authentication (--login | --refresh | --cookie | --jwt | --logout)
suno config          show | set | path | check
suno doctor          Health checks: auth, JWT, Chrome, API reach, credits, captcha state
suno agent-info      Machine-readable capabilities JSON
suno guide           List built-in songwriting guides, or print one (guides <name>)
suno skill           install | status — agent skill for Claude Code / Codex / Gemini
suno update          Distribution-aware update (--check to peek first)
```

## Guides

The CLI ships its songwriting knowledge as built-in guides — a single source of truth compiled into the binary, so what agents read never drifts from the tool.

```bash
suno guide                  # list every guide (name, aliases, description)
suno guide songwriting      # print the guide as raw markdown to stdout
suno guide priming          # aliases resolve too: `write`, `grammar`, `prime`
```

| Guide | What it covers |
| --- | --- |
| `songwriting` | How to write for Suno: structure, meta-tags, genres, vocal styles, hooks — the base grammar every song builds on |
| `priming` | Research/priming songs: evidence-graded psychological priming woven into lyrics, consent-first |

Write, then generate — the guide's output maps straight onto the flags:

```bash
suno guide songwriting > song.md      # read it, draft the Style Prompt + [Verse]/[Chorus] block
suno generate --title "Weekend Code" \
  --tags "indie rock, upbeat, male vocals" \
  --lyrics-file song.txt --wait --download ./songs/
```

Piped or `--json`, `suno guide <name>` returns a `{name, content}` envelope; the bare `suno guide` list returns an array of `{name, aliases, description}`.

## Features

### Zero-Friction Auth

```bash
suno auth --login    # Extracts session from your browser automatically
```

Reads the Clerk auth cookie from Chrome, Arc, Brave, Firefox, or Edge. Exchanges it for a JWT via Clerk token exchange, stores the refreshable session in a `0600` local auth file, and refreshes stale JWTs automatically when the underlying browser session is still valid.

Auth methods (in order of convenience):
1. `suno auth --login` — automatic browser extraction (recommended)
2. `suno auth --cookie <cookie>` — manual paste for headless servers; accepts either raw `__client` or a full browser `Cookie` header
3. `suno auth --jwt <token>` — direct JWT, expires in ~1 hour
4. `suno auth --refresh` — force a fresh JWT from the stored Clerk session

`suno auth` with no flags checks the existing session, or starts browser login if no auth is configured. `suno auth --logout` removes stored credentials.

### Generation Parameters

| Flag | What it does | Values |
|---|---|---|
| `--title` | Song title | up to 100 chars |
| `--tags` | Style direction | `"pop, synths, upbeat"` (1000 chars) |
| `--exclude` | Styles to avoid | `"metal, heavy, dark"` (1000 chars) |
| `--lyrics` / `--lyrics-file` | Custom lyrics with `[Verse]` tags | up to 5000 chars |
| `--prompt` (describe) | Free text description | up to 500 chars |
| `--model` | Model version | v5.5, v5, v4.5+, v4.5-all, v4.5, v4, v3.5, v3, v2 |
| `--vocal` | Vocal gender | male, female |
| `--persona` | Voice persona ID | UUID from Suno voice creation |
| `--weirdness` | How experimental | 0-100 |
| `--style-influence` | How strictly to follow tags | 0-100 |
| `--audio-influence` | How strongly source audio shapes the output (generate/cover) | 0-100 |
| `--instrumental` | No vocals | flag |
| `--wait` | Block until done | flag |
| `--download <dir>` | Auto-download after generation | directory path |
| `--token` | Pre-solved hCaptcha token (headless servers) | token string |
| `--no-captcha` | Never run the captcha auto-solver | flag |
| `--force` | Bypass the duplicate-run guard | flag |

`--wait` exits non-zero when Suno reports the generation failed (moderation rejections exit 3 — retrying the same prompt fails identically).

### Captcha Preflight

Before every generate/describe/extend/cover/remaster, the CLI asks Suno whether this account is captcha-gated (`POST /api/c/check`). Most accounts are above the trust threshold, so the Chrome-piloting hCaptcha solver is skipped entirely (`Captcha not required — skipping solver` on stderr). When a captcha IS required, the solver pilots a **headless** Chrome (no window, no Dock icon); if hCaptcha rejects the headless fingerprint, it transparently retries once in a headed instance parked offscreen. Either way the Chrome is killed when the CLI exits — nothing lingers. `SUNO_CAPTCHA_HEADLESS=1` / `SUNO_CAPTCHA_HEADED=1` pin a mode; `--token` supplies a pre-solved response instead, and `--no-captcha` disables solving outright. If a challenge keeps failing, generate one song in the suno.com UI to clear it, then retry.

### Voice Personas

Generate songs using your own voice. Create a voice in Suno's web UI, then use the persona ID:

```bash
# View persona details
suno persona <persona_id>

# Generate with your voice
suno generate --persona <persona_id> --title "My Song" --tags "pop" --lyrics "[Verse]\nHello world"

# Works with describe mode too
suno describe --persona <persona_id> --prompt "a warm ballad about starlight"
```

### Covers & Remasters

Create covers with different styles or remaster clips with newer models:

```bash
# Cover with different style tags
suno cover <clip_id> --tags "jazz, smooth piano" --model v5.5 --wait

# Remaster an old clip with the latest model
suno remaster <clip_id> --model v5.5 --wait --download ./remastered/
```

Both route through Suno's unified web generation endpoint (`/api/generate/v2-web/`).

### Clip Info

```bash
# Full details for any clip
suno info <clip_id>

# JSON for scripting
suno info <clip_id> --json | jq '.data.audio_url'
```

### Edit & Manage

```bash
# Update title and lyrics on an existing clip
suno set <clip_id> --title "New Title" --lyrics-file updated.txt

# Make clips public
suno publish <clip_id_1> <clip_id_2>

# Get timed lyrics in LRC format
suno timed-lyrics <clip_id> --lrc > song.lrc
```

### Downloads with Embedded Lyrics

Downloads automatically embed lyrics into MP3 files via ID3 tags:
- **USLT** (plain lyrics) — shown in most music players
- **SYLT** (synced word-by-word timestamps) — shown in Apple Music with timing

```bash
suno download <id1> <id2> --output ./songs/
```

Files use slug format: `title-slug-clipid8.mp3` — no overwrites when Suno generates 2 variations.

### Models

| Version | Codename | Default | Notes |
|---|---|---|---|
| **v5.5** | chirp-fenix | Yes | Latest, best quality — ≈10 credits per call (5/clip) |
| v5 | chirp-crow | | Previous generation |
| v4.5+ | chirp-bluejay | | Extended capabilities |
| v4.5-all | chirp-auk-turbo | | "Best free model" per Suno — cheapest generation |
| v4.5 | chirp-auk | | Stable |
| v4 | chirp-v4 | | Legacy |
| v3.5 / v3 / v2 | chirp-v3-5 / chirp-v3-0 / chirp-v2-xxl-alpha | | Early models |

Remaster models: v5.5 = chirp-flounder, v5 = chirp-carp, v4.5+ = chirp-bass.

`suno models` shows what your plan can actually use, live from the API.

### Configuration

Config lives in a TOML file (`suno config path` shows where) and every key is overridable via `SUNO_*` env vars. Precedence: **flag > env > config file > default**.

| Key | Env var | Default | What it does |
|---|---|---|---|
| `default_model` | `SUNO_DEFAULT_MODEL` | `v5.5` | Default `--model` for generate/describe/extend/cover |
| `poll_interval_secs` | `SUNO_POLL_INTERVAL_SECS` | `5` | Initial `--wait` poll backoff (doubles up to 15s) |
| `poll_timeout_secs` | `SUNO_POLL_TIMEOUT_SECS` | `600` | Total `--wait` timeout |
| `output_dir` | `SUNO_OUTPUT_DIR` | `.` | Default directory for `download` |

`SUNO_CONFIG_DIR` and `SUNO_DATA_DIR` relocate the config/auth directory and
the state directory (guard locks, captcha Chrome profile) — useful for
sandboxing or running isolated instances. `suno config path` shows the
resolved location.

```bash
suno config show                        # effective merged config
suno config set default_model v5.5      # persist a value (v5.5 is already the default)
suno config check                       # validate the file
```

### Agent-Friendly

Every command supports `--json` for structured output. When stdout is piped, JSON is auto-detected. Progress and errors go to stderr. Exit codes are semantic:

| Code | Meaning | Agent action |
|---|---|---|
| 0 | Success | Continue |
| 1 | Transient error (network, API, download) | Retry with backoff |
| 2 | Configuration or auth error | Run `suno doctor`; for auth `suno auth --login` |
| 3 | Bad input (arguments, unknown ID, moderation rejection, duplicate run) | Fix before retrying |
| 4 | Rate limited | Wait 30-60s, retry |

> **Breaking change in v0.6.0:** exit codes were remapped to the [agent-cli-framework](https://github.com/paperfoot/agent-cli-framework) contract. Auth errors moved 3 → 2, not-found moved 5 → 3, and code 5 no longer exists. `list --json` data changed from a bare clip array to `{clips, next_cursor, has_more}`, `list --page` was replaced by `--cursor`, and `generate --variation` was removed. Agents pinned to the 0.5.x contract must update their handling.

Error responses include actionable suggestions:

```json
{
  "version": "1",
  "status": "error",
  "error": {
    "code": "auth_expired",
    "message": "JWT expired or rejected by Suno",
    "suggestion": "Run `suno auth --refresh`; if that fails, run `suno auth --login`"
  }
}
```

```bash
# Pipe-friendly: auto-JSON when piped
suno list | jq '.data.clips[0].title'

# Paginate with the opaque cursor
suno list --cursor "$(suno list | jq -r '.data.next_cursor')"

# Agent capabilities discovery
suno agent-info

# Deterministic exit-code probe (hidden, for conformance tests)
suno contract 3; echo $?   # 3
```

The vendored framework conformance probe runs in CI: `./conformance/conformance.sh target/release/suno`.

### Install as a Coding Agent Skill

Teach Claude Code, Codex CLI, and Gemini CLI how to use `suno` with one command:

```bash
suno skill install   # writes SKILL.md to every detected platform:
                     #   ~/.claude/skills/suno/  ~/.codex/skills/suno/  ~/.gemini/skills/suno/
suno skill status    # which platforms have it, and whether it's current
```

Install is idempotent (`already_current` when nothing changed). The 0.5.x spelling `suno install-skill` still works as a hidden alias. After a CLI update, re-run `suno skill install` so agents see the new surface.

### API Endpoint Versions (Confirmed)

| Endpoint | Version | Status |
|---|---|---|
| Feed | **v3** (`POST /api/feed/v3`) | Latest |
| Generate | **v2-web** (`POST /api/generate/v2-web/`) | Latest web generation route |
| Concat | **v2** (`POST /api/generate/concat/v2/`) | Latest |
| Aligned lyrics | **v2** (`GET /api/gen/{id}/aligned_lyrics/v2/`) | Latest |
| Persona | `GET /api/persona/get-persona-paginated/{id}/` | Confirmed |

Generation tasks use `/api/generate/v2-web/` with the current web request shape. Normal generation and voice-persona generation are verified; cover/remaster support is implemented but should be recaptured whenever Suno changes the web schema.

## Known limitations

- **Self-update artifact verification is a follow-up.** Standalone self-update downloads the release binary from GitHub over HTTPS but does not yet verify a signature or attestation on the downloaded artifact. That requires release-signing infrastructure (an embedded public key + signed release assets) and is tracked as a follow-up. Until it lands, an install source that can't be recognized fails closed instead of self-replacing.

## Contributing

1. Fork the repo
2. Create a branch (`git checkout -b feature/your-idea`)
3. Make your changes and test with `cargo test`
4. Open a PR

We especially welcome:
- Audio upload implementation (S3 presigned flow documented in `API_INTELLIGENCE.md`)
- Voice persona creation workflow (endpoints captured, request bodies needed)
- OS keychain/Secret Service/CredMan storage for auth secrets

## License

MIT — see [LICENSE](LICENSE).

---

<div align="center">

Built by [Boris Djordjevic](https://github.com/longevityboris) at [199 Biotechnologies](https://github.com/199-biotechnologies)

<br />

**If this saves you time:**

[![Star this repo](https://img.shields.io/github/stars/paperfoot/suno-cli?style=for-the-badge&logo=github&label=%E2%AD%90%20Star%20this%20repo&color=yellow)](https://github.com/paperfoot/suno-cli/stargazers)
&nbsp;&nbsp;
[![Follow @longevityboris](https://img.shields.io/badge/Follow_%40longevityboris-000000?style=for-the-badge&logo=x&logoColor=white)](https://x.com/longevityboris)

</div>
