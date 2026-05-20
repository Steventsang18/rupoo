# Rupoo — Assistant Terminal piloté par IA

Rupoo est un assistant AI fonctionnant dans le terminal, prenant en charge l'exécution de plans, la gestion de compétences, la mémoire à long terme, le bac à sable de sécurité, l'intégration Git et le protocole MCP — le tout via le langage naturel ou l'interface TUI.

```
Version:  0.2.0        Langage: Rust 2021
Tests:    106 ✅       Binaire:  ~14 Mo (release, ARM64)
TUI:      ratatui      LLM:      Anthropic / OpenAI / DeepSeek / Ollama
DB:       SQLite (FTS5)  Sécurité: bac à sable path_jail + protection SSRF
```

---

## Démarrage rapide

### Installation

```bash
# Installation depuis les sources
cargo install --path .

# Ou exécution directe du binaire compilé
cargo run --release
```

### Configuration LLM

```bash
# Anthropic Claude
rupoo config set api_key.anthropic sk-ant-xxx
rupoo config set model.anthropic claude-sonnet-4-20250514

# Interfaces compatibles OpenAI / DeepSeek etc.
rupoo config set api_key.openai sk-xxx
rupoo config set model.openai deepseek-chat
rupoo config set base_url.openai https://api.deepseek.com/v1

# Modèle local Ollama
# Pas de clé API nécessaire, Ollama utilise http://localhost:11434 par défaut
```

### Lancement

```bash
# TUI interactif (par défaut)
rupoo

# Raccourcis TUI
# Ctrl+P    Panneau de commandes
# Ctrl+C    Quitter
# Tab       Changer le focus (zone de saisie ↔ barre latérale)
# ↑/↓       Historique de saisie
# Shift+↑/↓   Défilement de la zone de discussion (ou molette de souris)
# PgUp/PgDn   Défilement rapide
```

---

## Interface en ligne de commande

```
rupoo [OPTIONS] [COMMANDE]
```

### Options globales

| Option | Description |
|--------|-------------|
| `--verbose` | Affiche les logs de débogage sur stderr |

### Sous-commandes

| Commande | Description |
|----------|-------------|
| _(aucune)_ | Entre dans le TUI interactif (disposition trois colonnes) |
| `run --task <id>` | Exécute un Plan sauvegardé |
| `demo` | Exécute le Plan de démonstration intégré |
| `status [--short]` | Affiche un aperçu de l'état du système |
| `model [show\|list\|set]` | Consulte/change le fournisseur LLM et le modèle |
| `session [list\|show\|resume\|delete\|prune]` | Gère les plans d'exécution |
| `skills [list\|show\|run\|install-builtin\|learn]` | Gestion du système de compétences |
| `config [set\|get\|list]` | Gestion de la configuration et des clés API |
| `git [status\|commit\|pr]` | Intégration Git |
| `doctor [--fix]` | Diagnostique les problèmes d'environnement et de configuration |
| `logs [--follow] [--lines N] [--level LEVEL]` | Consulte les journaux d'exécution |
| `mcp-server` | Démarre un serveur de protocole MCP (JSON-RPC sur stdio) |
| `serve --port <port>` | Mode serveur |

---

## Architecture

```
┌─ CLI (clap) ─────────────────────────────────────────────┐
│  rupoo  →  TUI (ratatui + crossterm)                     │
│         →  Sous-commandes (status/model/session/doctor/logs…)
└──────────────────────┬───────────────────────────────────┘
                       │
┌──────────────────────▼───────────────────────────────────┐
│  Machine d'état Agent                                     │
│  Think → ToolCall → WaitForInput → Finish                │
│  + Exec / HttpRequest / BrowserAction                    │
├──────────────────────────────────────────────────────────┤
│  Passerelle LLM (rig-core)                                │
│  Interface unifiée Anthropic / OpenAI / Ollama           │
├──────────────────────────────────────────────────────────┤
│  Couche d'exécution d'outils                             │
│  McpToolExecutor → rig_tools (Echo, FileRead/Write, Ls) │
│  + Serveur MCP (JSON-RPC stdio)                         │
├──────────────────────────────────────────────────────────┤
│  SafetyContext                                            │
│  Bac à sable path_jail · Liste noire de commandes ·      │
│  Protection SSRF · Protection par timeout                │
├──────────────────────────────────────────────────────────┤
│  SQLite (WAL + FTS5)                                     │
│  Persistance des Plans · Checkpoint de récupération      │
│  après crash · Historique des sessions · Mémoire long    │
│  terme                                                    │
└──────────────────────────────────────────────────────────┘
```

### Description des modules

| Module | Lignes | Responsabilité |
|--------|--------|----------------|
| `main.rs` | 700+ | Point d'entrée CLI, répartition des commandes, `build_engine` |
| `agent.rs` | 840+ | Machine d'état Agent, 7 types d'étapes, récupération après crash |
| `db.rs` | 890 | Couche SQLite, CRUD des Plans + Checkpoints + Mémoire FTS5 |
| `llm.rs` | 350 | Passerelle LLM, interface unifiée Anthropic/OpenAI/Ollama |
| `cli/mod.rs` | 680 | Boucle d'événements TUI, thread de pont Agent |
| `cli/app.rs` | 370 | État de l'application TUI, gestion des sessions, routage des messages |
| `cli/ui.rs` | 420 | Rendu TUI : disposition trois colonnes, bulles, blocs de code, barre d'état |
| `cli/handlers.rs` | 380 | Stratégies de mode de saisie (Chat/Thinking/Approval/Palette) |
| `safety.rs` | 250 | Bac à sable de sécurité, path_jail, SSRF, liste noire de commandes |
| `mcp.rs` | 250+ | Planificateur d'outils MCP + client JSON-RPC |
| `mcp_server.rs` | 380 | Serveur MCP (réutilise McpToolExecutor) |
| `rig_tools.rs` | 400 | Outils Echo / FileRead / FileWrite / ListDir |
| `task.rs` | 340 | Définitions de types Step/Plan/Checkpoint |
| `memory.rs` | 140 | Mémoire à long terme (recherche plein texte FTS5) |
| `skill.rs` | 390 | Système de compétences (fichiers JSON + apprentissage automatique) |
| `git.rs` | 240 | Intégration Git (git2 + gh CLI) |
| `error.rs` | 34 | Type d'erreur unifié |

### Architecture de sécurité

| Couche de protection | Implémentation |
|----------------------|----------------|
| Liste noire de commandes | 20+ commandes dangereuses (sudo, rm, mkfs, dd, etc.) |
| Bac à sable des chemins de fichiers | Crate `path_jail`, empêche `../../etc/passwd`, échappement par lien symbolique |
| Protection SSRF | Blocage de localhost/127.0.0.1/0.0.0.0/`[::1]`/169.254.x.x/nip.io |
| Protection par timeout | Commande 30s / HTTP 30s / Navigateur 30s |
| Nettoyage des variables d'environnement | Seuls PATH/HOME/USER/SHELL/LANG/TERM sont conservés |
| Troncature de la sortie | Commande 10 Ko / Lecture de fichier 4 Ko |
| Sécurité multi-chemin | Triple protection McpToolExecutor + Agent LLM + Serveur MCP |

---

## Fonctionnalités principales

### Moteur d'exécution de Plan

Prend en charge 7 types d'étapes :

| Étape | Description |
|-------|-------------|
| Think | Raisonnement LLM, avec récupération de mémoire FTS5 comme contexte |
| ToolCall | Appelle des outils intégrés (lecture/écriture de fichiers, liste de répertoires, Echo) |
| WaitForInput | Attend la saisie de l'utilisateur avant de continuer |
| Exec | Exécute des commandes externes (limitées par le bac à sable de sécurité) |
| HttpRequest | Requêtes HTTP GET/POST (avec protection SSRF) |
| BrowserAction | Automatisation du navigateur (Navigation/Capture d'écran/Clic/GetText) |
| Finish | Termine le plan, déclenche automatiquement l'apprentissage de compétences |

### Récupération après crash

- **Checkpoint dynamique (heartbeat)** : Écrit un Checkpoint d'état « Running » avant les opérations longues
- **Atomicité transactionnelle** : `record_step_completion` met à jour Plan + Checkpoint dans une seule transaction SQLite
- **Récupération en trois couches** : `reset_running_plans → get_last_checkpoint → décision du point de reprise selon l'état`

### TUI

- **Disposition trois colonnes** : Liste des sessions à gauche, zone de discussion au centre, panneau d'état à droite
- **Bulles de messages** : Distinction par couleur pour utilisateur/assistant/système
- **Surlignage des blocs de code** : Rendu avec bordures et pré-pliage
- **Historique de saisie** : Navigation parmi les 100 dernières saisies avec ↑/↓
- **Défilement automatique** : Les nouveaux messages défilent automatiquement vers le bas ; le défilement manuel est réinitialisé lors de l'envoi d'un message
- **Adaptation à la fenêtre** : Réorganisation et repliage automatiques lors du redimensionnement du terminal

### Système de compétences

- **Gestion par fichiers JSON** : `~/.skills/*.json`
- **Compétences intégrées** : code-review, generate-readme
- **Apprentissage automatique** : Extraction automatique en compétence réutilisable après exécution d'un Plan
- **Apprentissage manuel** : `rupoo skills learn <plan_id> <nom_competence>`

### Mémoire à long terme

- **Recherche plein texte FTS5** : Classement par pertinence BM25
- **Persistance des sessions** : Stockage SQLite de l'historique des sessions TUI
- **Injection contextuelle** : Récupération automatique des mémoires pertinentes lors de l'étape Think

---

## Dépendances

| Crate | Utilisation |
|-------|-------------|
| tokio | Runtime asynchrone |
| clap | Analyse CLI |
| ratatui + crossterm | Framework TUI |
| rig-core 0.30 | Passerelle multi-fournisseur LLM |
| rusqlite (WAL + FTS5) | Base de données SQLite |
| git2 | Opérations Git |
| reqwest | Client HTTP |
| path_jail | Sécurité des chemins de fichiers |
| tui-textarea | Composant de saisie TUI |
| serde + serde_json | Sérialisation |
| tracing + tracing-subscriber | Journalisation |
| uuid | Identifiants Plan / Step |
| chrono | Horodatage |
| crossbeam-channel | Communication inter-threads |

---

## Tests

```bash
# Tous les tests
cargo test

# Tests de bibliothèque uniquement
cargo test --lib

# Tests d'intégration uniquement
cargo test --test db_test
cargo test --test crash_recovery_test
cargo test --test cli_db_test

# Plan d'exécution
cargo run --release demo
```

106 tests couvrent :
- 54 tests unitaires (Agent, DB, LLM, MCP, Safety, Memories, Skills, Git)
- 33 tests du crate principal (commandes CLI + handlers TUI)
- 4 tests d'intégration CLI-DB
- 2 tests d'intégration de récupération après crash
- 13 tests d'intégration DB

---

## Construction

```bash
# Construction de développement
cargo build

# Construction de release (recommandée)
cargo build --release

# Avec support GUI
cargo build --release --features gui

# Taille du binaire
# ~14 Mo (release, ARM64)
```

---

## Licence

MIT
