# Rupoo — AI-drevet Terminalassistent

Rupoo er en AI-assistent som kjører i terminalen, og støtter planutføring, ferdighetshåndtering, langtidsminne, sikkerhetssandkasse, Git-integrasjon og MCP-protokollen — alt via naturlig språk eller TUI-interaksjon.

```
Versjon:  0.2.0        Språk:    Rust 2021
Tester:   106 ✅       Binær:    ~14 MB (release, ARM64)
TUI:      ratatui      LLM:      Anthropic / OpenAI / DeepSeek / Ollama
DB:       SQLite (FTS5)  Sikkerhet:  path_jail sandkasse + SSRF-beskyttelse
```

---

## Kom i gang

### Installasjon

```bash
# Bygg fra kildekode
cargo install --path .

# Eller kjør den kompilerte binæren direkte
cargo run --release
```

### Konfigurer LLM

```bash
# Anthropic Claude
rupoo config set api_key.anthropic sk-ant-xxx
rupoo config set model.anthropic claude-sonnet-4-20250514

# OpenAI / DeepSeek og andre kompatible grensesnitt
rupoo config set api_key.openai sk-xxx
rupoo config set model.openai deepseek-chat
rupoo config set base_url.openai https://api.deepseek.com/v1

# Ollama lokale modeller
# Ingen API-nøkkel nødvendig, Ollama kjører som standard på http://localhost:11434
```

### Start

```bash
# Interaktiv TUI (standard)
rupoo

# TUI-hurtigtaster
# Ctrl+P    Kommandopanel
# Ctrl+C    Avslutt
# Tab       Bytt fokus (skrivefelt ↔ sidepanel)
# ↑/↓       Skrivehistorikk
# Shift+↑/↓   Rull i chat-området (eller musehjul)
# PgUp/PgDn   Større rull
```

---

## Kommandolinjegrensesnitt

```
rupoo [OPSJONER] [KOMMANDO]
```

### Globale opsjoner

| Opsjon   | Beskrivelse                            |
|----------|----------------------------------------|
| `--verbose` | Skriv ut feilsøkingslogger til stderr |

### Underkommandoer

| Kommando | Beskrivelse |
|----------|-------------|
| _(ingen)_ | Gå inn i interaktiv TUI (trekolonners layout) |
| `run --task <id>` | Utfør en lagret plan |
| `demo` | Kjør innebygd demo-plan |
| `status [--short]` | Vis systemstatusoversikt |
| `model [show\|list\|set]` | Vis/bytt LLM-leverandør og -modell |
| `session [list\|show\|resume\|delete\|prune]` | Administrer utføringsplaner |
| `skills [list\|show\|run\|install-builtin\|learn]` | Ferdighetssystemhåndtering |
| `config [set\|get\|list]` | Konfigurasjonshåndtering og API-nøkler |
| `git [status\|commit\|pr]` | Git-integrasjon |
| `doctor [--fix]` | Diagnostiser miljø- og konfigurasjonsproblemer |
| `logs [--follow] [--lines N] [--level NIVÅ]` | Vis kjøringslogger |
| `mcp-server` | Start MCP-protokolltjener (JSON-RPC over stdio) |
| `serve --port <port>` | Tjenermodus |

---

## Arkitektur

```
┌─ CLI (clap) ─────────────────────────────────────────────┐
│  rupoo  →  TUI (ratatui + crossterm)                     │
│         →  Underkommandoer (status/model/session/doctor/logs...) │
└──────────────────────┬───────────────────────────────────┘
                       │
┌──────────────────────▼───────────────────────────────────┐
│  Agent-tilstandsautomat                                     │
│  Think → ToolCall → WaitForInput → Finish               │
│  + Exec / HttpRequest / BrowserAction                    │
├──────────────────────────────────────────────────────────┤
│  LLM Gateway (rig-core)                                  │
│  Enhetlig grensesnitt for Anthropic / OpenAI / Ollama    │
├──────────────────────────────────────────────────────────┤
│  Verktøyutføreringslag                                     │
│  McpToolExecutor → rig_tools (Echo, FileRead/Write, Ls) │
│  + MCP-tjener (JSON-RPC stdio)                           │
├──────────────────────────────────────────────────────────┤
│  Sikkerhetskontekst                                       │
│  path_jail sandkasse · Kommandosvarteliste · SSRF-beskyttelse · Tidsavbrudd │
├──────────────────────────────────────────────────────────┤
│  SQLite (WAL + FTS5)                                     │
│  Plan-persistens · Sjekkpunkt-krasjgjenoppretting · Øktlogg · Langtidsminne  │
└──────────────────────────────────────────────────────────┘
```

### Modulbeskrivelse

| Modul | Linjer | Ansvarsområde |
|-------|--------|---------------|
| `main.rs` | 700+ | CLI-inngang, kommandodistribusjon, `build_engine` |
| `agent.rs` | 840+ | Agent-tilstandsautomat, 7 stegtyper, krasjgjenoppretting |
| `db.rs` | 890 | SQLite-lag, Plan CRUD + Sjekkpunkter + FTS5-minne |
| `llm.rs` | 350 | LLM-gateway, enhetlig Anthropic/OpenAI/Ollama |
| `cli/mod.rs` | 680 | TUI-hendelsesløkke, Agent-brotråd |
| `cli/app.rs` | 370 | TUI-applikasjonstilstand, øktbehandling, meldingsruting |
| `cli/ui.rs` | 420 | TUI-gjengivelse: trekolonners layout, bobler, kodeblokker, statuslinje |
| `cli/handlers.rs` | 380 | Inndatamodustrategier (Chat/Thinking/Approval/Palette) |
| `safety.rs` | 250 | Sikkerhetssandkasse, path_jail, SSRF, kommandosvarteliste |
| `mcp.rs` | 250+ | MCP verktøysplanlegger + JSON-RPC-klient |
| `mcp_server.rs` | 380 | MCP-tjener (gjenbruker McpToolExecutor) |
| `rig_tools.rs` | 400 | Echo / FileRead / FileWrite / ListDir-verktøy |
| `task.rs` | 340 | Steg/Plan/Sjekkpunkt-typedefinisjoner |
| `memory.rs` | 140 | Langtidsminne (FTS5 fulltekstsøk) |
| `skill.rs` | 390 | Ferdighetssystem (JSON-filer + automatisk læring) |
| `git.rs` | 240 | Git-integrasjon (git2 + gh CLI) |
| `error.rs` | 34 | Enhetlig feiltype |

### Sikkerhetsarkitektur

| Beskyttelseslag | Implementering |
|-----------------|----------------|
| Kommandosvarteliste | 20+ farlige kommandoer (sudo, rm, mkfs, dd, osv.) |
| Filsystemsandkasse | `path_jail` crate, hindrer `../../etc/passwd`, symbolsøkkelunndragelse |
| SSRF-beskyttelse | Blokkerer localhost/127.0.0.1/0.0.0.0/`[::1]`/169.254.x.x/nip.io |
| Tidsavbruddsbeskyttelse | Kommando 30s / HTTP 30s / Nettleser 30s |
| Miljøvariabelrensing | Beholder kun PATH/HOME/USER/SHELL/LANG/TERM |
| Utdatabeskjæring | Kommando 10K / Fil lesing 4K |
| Flerbanesikkerhet | McpToolExecutor + LLM Agent + MCP-tjener trippel beskyttelse |

---

## Kjernefunksjoner

### Plan-utføringsmotor

Støtter 7 stegtyper:

| Steg | Beskrivelse |
|------|-------------|
| Think | LLM-resonnering med FTS5-minnehenting som kontekst |
| ToolCall | Kall innebygde verktøy (fil les/skriv, katalogliste, Echo) |
| WaitForInput | Vent på brukerinndata før fortsettelse |
| Exec | Utfør ekstern kommando (begrenset av sikkerhetssandkasse) |
| HttpRequest | HTTP GET/POST-forespørsel (med SSRF-beskyttelse) |
| BrowserAction | Nettleserautomatisering (Naviger/Skjermbilde/Klikk/HentTekst) |
| Finish | Fullfør plan, utløs automatisk ferdighetslæring |

### Krasjgjenoppretting

- **Hjerteslag-sjekkpunkt**: Skriver Running-status sjekkpunkt før langvarige operasjoner
- **Transaksjonsatomisitet**: `record_step_completion` oppdaterer Plan + Sjekkpunkt i én enkelt SQLite-transaksjon
- **Trelagsgjenoppretting**: `reset_running_plans→get_last_checkpoint→bestem gjenopprettingspunkt basert på status`

### TUI

- **Trekolonners layout**: Venstre øktliste, sentralt chat-område, høyre statuspanel
- **Meldingsbobler**: Bruker/assistent/system i tre farger
- **Kodeblokkmarkering**: Koderamme-gjengivelse + forhåndsorddeling
- **Skrivehistorikk**: ↑/↓ navigerer de siste 100 inntastede meldingene
- **Autorull**: Nye meldinger ruller automatisk til bunnen; manuell rulling tilbakestilles ved ny melding
- **Vindustilpasning**: Endringer i terminalstørrelse utløser automatisk ny layout og ny orddeling

### Ferdighetssystem

- **JSON-filhåndtering**: `~/.skills/*.json`
- **Innebygde ferdigheter**: code-review, generate-readme
- **Automatisk læring**: Etter fullført plan, trekkes den automatisk ut som en gjenbrukbar ferdighet
- **Manuell læring**: `rupoo skills learn <plan_id> <ferdighetsnavn>`

### Langtidsminne

- **FTS5 fulltekstsøk**: Støtter BM25-relevanssortering
- **Øktpersistens**: SQLite lagrer UI-øktlogg
- **Kontekstinnsetting**: Think-steg henter automatisk relevante minner

---

## Avhengigheter

| Crate | Bruksområde |
|-------|-------------|
| tokio | Asynkron kjøringsmiljø |
| clap | CLI-tolkning |
| ratatui + crossterm | TUI-rammeverk |
| rig-core 0.30 | LLM flerleverandør-gateway |
| rusqlite (WAL + FTS5) | SQLite-database |
| git2 | Git-operasjoner |
| reqwest | HTTP-klient |
| path_jail | Filsystemsikkerhet |
| tui-textarea | TUI-skrivekomponent |
| serde + serde_json | Serialisering |
| tracing + tracing-subscriber | Logging |
| uuid | Plan / Steg-ID |
| chrono | Tidsstempler |
| crossbeam-channel | Trådoverskridende kommunikasjon |

---

## Tester

```bash
# Alle tester
cargo test

# Kun bibliotekstester
cargo test --lib

# Kun integreringstester
cargo test --test db_test
cargo test --test crash_recovery_test
cargo test --test cli_db_test

# Kjør demo-plan
cargo run --release demo
```

106 tester dekker:
- 54 enhetstester (Agent, DB, LLM, MCP, Safety, Memories, Skills, Git)
- 33 main-crate-tester (CLI-kommandoer + TUI-handler)
- 4 CLI-DB-integrasjonstester
- 2 krasjgjenopprettings-integrasjonstester
- 13 DB-integrasjonstester

---

## Bygging

```bash
# Utviklingsbygging
cargo build

# Utgivelsesbygging (anbefalt)
cargo build --release

# Med GUI-støtte
cargo build --release --features gui

# Binærstørrelse
# ~14 MB (release, ARM64)
```

---

## Lisens

MIT
