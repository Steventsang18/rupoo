# Rupoo — KI-gestützter Terminal-Assistent

Rupoo ist ein KI-Assistent, der direkt im Terminal läuft. Er unterstützt Planausführung, Skill-Management, Langzeitgedächtnis, eine Sicherheits-Sandbox, Git-Integration und das MCP-Protokoll – alles über natürliche Sprache oder die TUI-Benutzeroberfläche.

```
Version:  0.2.0        Sprache:   Rust 2021
Tests:    106 ✅       Binary:    ~14 MB (Release, ARM64)
TUI:      ratatui      LLM:       Anthropic / OpenAI / DeepSeek / Ollama
DB:       SQLite (FTS5)  Sicherheit: path_jail-Sandbox + SSRF-Schutz
```

---

## Schnellstart

### Installation

```bash
# Aus dem Quellcode installieren
cargo install --path .

# Oder die kompilierte Binärdatei direkt ausführen
cargo run --release
```

### LLM konfigurieren

```bash
# Anthropic Claude
rupoo config set api_key.anthropic sk-ant-xxx
rupoo config set model.anthropic claude-sonnet-4-20250514

# OpenAI / DeepSeek und andere kompatible Schnittstellen
rupoo config set api_key.openai sk-xxx
rupoo config set model.openai deepseek-chat
rupoo config set base_url.openai https://api.deepseek.com/v1

# Ollama lokales Modell
# Kein API-Key nötig, Ollama verwendet standardmäßig http://localhost:11434
```

### Starten

```bash
# Interaktive TUI (Standard)
rupoo

# TUI-Tastenkürzel
# Strg+P    Befehlspalette
# Strg+C    Beenden
# Tab       Fokus wechseln (Eingabebereich ↔ Seitenleiste)
# ↑/↓       Eingabeverlauf
# Umschalt+↑/↓   Chat-Bereich scrollen (oder Mausrad)
# Bild↑/Bild↓   Größere Sprünge
```

---

## Befehlszeilenschnittstelle

```
rupoo [OPTIONEN] [BEFEHL]
```

### Globale Optionen

| Option | Beschreibung |
|--------|--------------|
| `--verbose` | Debug-Logs auf stderr ausgeben |

### Unterbefehle

| Befehl | Beschreibung |
|--------|--------------|
| _(keiner)_ | Interaktive TUI starten (Dreispalten-Layout) |
| `run --task <id>` | Ein gespeichertes Plan ausführen |
| `demo` | Integriertes Demo-Plan ausführen |
| `status [--short]` | Systemstatus anzeigen |
| `model [show|list|set]` | LLM-Anbieter und -Modell anzeigen/wechseln |
| `session [list|show|resume|delete|prune]` | Ausführungspläne verwalten |
| `skills [list|show|run|install-builtin|learn]` | Skill-System verwalten |
| `config [set|get|list]` | Konfiguration und API-Keys verwalten |
| `git [status|commit|pr]` | Git-Integration |
| `doctor [--fix]` | Umgebungs- und Konfigurationsprobleme diagnostizieren |
| `logs [--follow] [--lines N] [--level STUFE]` | Laufzeitlogs anzeigen |
| `mcp-server` | MCP-Protokollserver starten (JSON-RPC über stdio) |
| `serve --port <port>` | Servermodus |

---

## Architektur

```
┌─ CLI (clap) ─────────────────────────────────────────────┐
│  rupoo  →  TUI (ratatui + crossterm)                     │
│         →  Unterbefehle (status/model/session/doctor/logs…)│
└──────────────────────┬───────────────────────────────────┘
                       │
┌──────────────────────▼───────────────────────────────────┐
│  Agent-Zustandsautomat                                     │
│  Think → ToolCall → WaitForInput → Finish               │
│  + Exec / HttpRequest / BrowserAction                    │
├──────────────────────────────────────────────────────────┤
│  LLM Gateway (rig-core)                                  │
│  Einheitliche Schnittstelle für Anthropic / OpenAI / Ollama│
├──────────────────────────────────────────────────────────┤
│  Tool Executor Layer                                     │
│  McpToolExecutor → rig_tools (Echo, FileRead/Write, Ls) │
│  + MCP-Server (JSON-RPC stdio)                          │
├──────────────────────────────────────────────────────────┤
│  SafetyContext                                           │
│  path_jail-Sandbox · Befehls-Blacklist · SSRF-Schutz ·   │
│  Timeout-Schutz                                          │
├──────────────────────────────────────────────────────────┤
│  SQLite (WAL + FTS5)                                     │
│  Plan-Persistenz · Checkpoint-Absturzwiederherstellung ·  │
│  Sitzungsverlauf · Langzeitgedächtnis                    │
└──────────────────────────────────────────────────────────┘
```

### Modulbeschreibung

| Modul | Zeilen | Verantwortung |
|-------|--------|---------------|
| `main.rs` | 700+ | CLI-Einstieg, Befehlsverteilung, `build_engine` |
| `agent.rs` | 840+ | Agent-Zustandsautomat, 7 Step-Typen, Absturzwiederherstellung |
| `db.rs` | 890 | SQLite-Schicht, Plan-CRUD + Checkpoints + FTS5-Gedächtnis |
| `llm.rs` | 350 | LLM-Gateway, vereinheitlicht Anthropic/OpenAI/Ollama |
| `cli/mod.rs` | 680 | TUI-Ereignisschleife, Agent-Bridge-Thread |
| `cli/app.rs` | 370 | TUI-Anwendungsstatus, Sitzungsverwaltung, Nachrichtenrouting |
| `cli/ui.rs` | 420 | TUI-Rendering: Dreispalten-Layout, Sprechblasen, Codeblöcke, Statusleiste |
| `cli/handlers.rs` | 380 | Eingabemodus-Strategien (Chat/Thinking/Approval/Palette) |
| `safety.rs` | 250 | Sicherheits-Sandbox, path_jail, SSRF, Befehls-Blacklist |
| `mcp.rs` | 250+ | MCP-Tool-Scheduler + JSON-RPC-Client |
| `mcp_server.rs` | 380 | MCP-Server (wiederverwendet McpToolExecutor) |
| `rig_tools.rs` | 400 | Echo / FileRead / FileWrite / ListDir-Werkzeuge |
| `task.rs` | 340 | Step/Plan/Checkpoint-Typdefinitionen |
| `memory.rs` | 140 | Langzeitgedächtnis (FTS5-Volltextsuche) |
| `skill.rs` | 390 | Skill-System (JSON-Dateien + automatisches Lernen) |
| `git.rs` | 240 | Git-Integration (git2 + gh CLI) |
| `error.rs` | 34 | Vereinheitlichter Fehlertyp |

### Sicherheitsarchitektur

| Schutzschicht | Implementierung |
|---------------|-----------------|
| Befehls-Blacklist | 20+ gefährliche Befehle (sudo, rm, mkfs, dd usw.) |
| Dateipfad-Sandbox | `path_jail`-Crate, verhindert `../../etc/passwd` und Symlink-Eskalation |
| SSRF-Schutz | Blockiert localhost/127.0.0.1/0.0.0.0/`[::1]`/169.254.x.x/nip.io |
| Timeout-Schutz | Befehl 30s / HTTP 30s / Browser 30s |
| Umgebungsvariablen-Bereinigung | Nur PATH/HOME/USER/SHELL/LANG/TERM |
| Ausgabe-Kürzung | Befehl 10K / Dateilesen 4K |
| Mehrpfad-Sicherheit | McpToolExecutor + LLM Agent + MCP Server – dreifacher Schutz |

---

## Kernfunktionen

### Plan-Ausführungs-Engine

Unterstützt 7 Schritt-Typen:

| Schritt | Beschreibung |
|---------|--------------|
| Think | LLM-Schlussfolgerung mit FTS5-Gedächtnisabruf als Kontext |
| ToolCall | Integrierte Werkzeuge aufrufen (Datei lesen/schreiben, Verzeichnis auflisten, Echo) |
| WaitForInput | Auf Benutzereingabe warten, dann fortfahren |
| Exec | Externen Befehl ausführen (durch Sicherheits-Sandbox eingeschränkt) |
| HttpRequest | HTTP-GET/POST-Anfrage (mit SSRF-Schutz) |
| BrowserAction | Browser-Automatisierung (Navigieren/Screenshot/Klicken/GetText) |
| Finish | Plan abschließen, automatisch Skill-Lernen auslösen |

### Absturzwiederherstellung

- **Heartbeat-Checkpoint**: Vor langen Operationen wird ein Running-Status-CP geschrieben
- **Transaktionsatomarität**: `record_step_completion` aktualisiert Plan + Checkpoint in einer einzigen SQLite-Transaktion
- **Dreistufige Wiederherstellung**: `reset_running_plans→get_last_checkpoint→Wiederherstellungspunkt je nach Status bestimmen`

### TUI

- **Dreispalten-Layout**: Linke Seite Sitzungsliste, Mitte Chat-Bereich, Rechte Seite Status-Panel
- **Nachrichten-Sprechblasen**: Benutzer/Assistent/System in drei Farben unterschieden
- **Codeblock-Hervorhebung**: Code-Rahmen-Rendering + vorzeitiger Zeilenumbruch
- **Eingabeverlauf**: ↑/↓ navigiert durch die letzten 100 Eingaben
- **Automatisches Scrollen**: Neue Nachrichten scrollen automatisch nach unten; nach manuellem Zurückscrollen wird beim Senden wieder nach unten gescrollt
- **Fensteranpassung**: Automatische Neu-Layout und Neu-Umbruch bei Terminalgrößenänderung

### Skill-System

- **JSON-Dateiverwaltung**: `~/.skills/*.json`
- **Integrierte Skills**: code-review, generate-readme
- **Automatisches Lernen**: Nach Plan-Ausführung automatisch als wiederverwendbaren Skill extrahieren
- **Manuelles Lernen**: `rupoo skills learn <plan_id> <skill_name>`

### Langzeitgedächtnis

- **FTS5-Volltextsuche**: Unterstützt BM25-Relevanzsortierung
- **Sitzungspersistenz**: SQLite speichert UI-Sitzungsverlauf
- **Kontexteinspritzung**: Think-Schritte rufen automatisch relevante Erinnerungen ab

---

## Abhängigkeiten

| Crate | Verwendung |
|-------|------------|
| tokio | Asynchrone Laufzeit |
| clap | CLI-Parsing |
| ratatui + crossterm | TUI-Framework |
| rig-core 0.30 | LLM-Multi-Provider-Gateway |
| rusqlite (WAL + FTS5) | SQLite-Datenbank |
| git2 | Git-Operationen |
| reqwest | HTTP-Client |
| path_jail | Dateipfad-Sicherheit |
| tui-textarea | TUI-Eingabekomponente |
| serde + serde_json | Serialisierung |
| tracing + tracing-subscriber | Logging |
| uuid | Plan-/Step-ID |
| chrono | Zeitstempel |
| crossbeam-channel | Thread-übergreifende Kommunikation |

---

## Tests

```bash
# Alle Tests
cargo test

# Nur Bibliothekstests
cargo test --lib

# Nur Integrationstests
cargo test --test db_test
cargo test --test crash_recovery_test
cargo test --test cli_db_test

# Ausführungsplan
cargo run --release demo
```

106 Tests decken ab:
- 54 Unit-Tests (Agent, DB, LLM, MCP, Safety, Memories, Skills, Git)
- 33 main-crate-Tests (CLI-Befehle + TUI-Handler)
- 4 CLI-DB-Integrationstests
- 2 Integrationsstests zur Absturzwiederherstellung
- 13 DB-Integrationstests

---

## Bauen

```bash
# Entwicklungsbuild
cargo build

# Release-Build (empfohlen)
cargo build --release

# Mit GUI-Unterstützung
cargo build --release --features gui

# Binary-Größe
# ~14 MB (Release, ARM64)
```

---

## Lizenz

MIT
