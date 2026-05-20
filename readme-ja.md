# Rupoo — AI駆動ターミナルアシスタント

Rupoo はターミナル上で動作する AI アシスタントです。Plan 実行エンジン、スキル管理、長期記憶、セキュリティサンドボックス、Git 統合、MCP プロトコルをサポート——すべて自然言語または TUI で操作可能です。

```
Version:  0.2.0        Language: Rust 2021
Tests:    106 ✅       Binary:   ~14 MB (release, ARM64)
TUI:      ratatui      LLM:      Anthropic / OpenAI / DeepSeek / Ollama
DB:       SQLite (FTS5)  Safety:  path_jail サンドボックス + SSRF 対策
```

---

## クイックスタート

### インストール

```bash
# ソースからインストール
cargo install --path .

# またはコンパイル済みバイナリを実行
cargo run --release
```

### LLM の設定

```bash
# Anthropic Claude
rupoo config set api_key.anthropic sk-ant-xxx
rupoo config set model.anthropic claude-sonnet-4-20250514

# OpenAI / DeepSeek など互換インターフェース
rupoo config set api_key.openai sk-xxx
rupoo config set model.openai deepseek-chat
rupoo config set base_url.openai https://api.deepseek.com/v1

# Ollama ローカルモデル
# API Key 不要、Ollama デフォルト http://localhost:11434
```

### 起動

```bash
# インタラクティブ TUI（デフォルト）
rupoo

# TUI ショートカット
# Ctrl+P   コマンドパレット
# Ctrl+C   終了
# Tab      フォーカス切替（入力エリア ↔ サイドバー）
# ↑/↓      入力履歴
# Shift+↑/↓  チャットエリアスクロール（またはマウスホイール）
# PgUp/PgDn  大きくスクロール
```

---

## コマンドラインインターフェース

```
rupoo [OPTIONS] [COMMAND]
```

### グローバルオプション

| オプション | 説明 |
|-----------|------|
| `--verbose` | stderr にデバッグログを出力 |

### サブコマンド

| コマンド | 説明 |
|---------|------|
| _(なし)_ | インタラクティブ TUI を起動（3カラムレイアウト） |
| `run --task <id>` | 保存された Plan を実行 |
| `demo` | 内蔵デモ Plan を実行 |
| `status [--short]` | システムステータス概要を表示 |
| `model [show\|list\|set]` | LLM プロバイダとモデルの表示/切替 |
| `session [list\|show\|resume\|delete\|prune]` | 実行計画の管理 |
| `skills [list\|show\|run\|install-builtin\|learn]` | スキルシステム管理 |
| `config [set\|get\|list]` | 設定管理と API Keys |
| `git [status\|commit\|pr]` | Git 統合 |
| `doctor [--fix]` | 環境と設定の問題を診断 |
| `logs [--follow] [--lines N] [--level LEVEL]` | 実行ログの表示 |
| `mcp-server` | MCP プロトコルサーバーを起動（JSON-RPC over stdio） |
| `serve --port <port>` | サーバーモード |

---

## アーキテクチャ

```
┌─ CLI (clap) ─────────────────────────────────────────────┐
│  rupoo  →  TUI (ratatui + crossterm)                     │
│         →  サブコマンド (status/model/session/doctor/logs...) │
└──────────────────────┬───────────────────────────────────┘
                       │
┌──────────────────────▼───────────────────────────────────┐
│  Agent ステートマシン                                       │
│  Think → ToolCall → WaitForInput → Finish                 │
│  + Exec / HttpRequest / BrowserAction                     │
├──────────────────────────────────────────────────────────┤
│  LLM Gateway (rig-core)                                   │
│  Anthropic / OpenAI / Ollama 統一インターフェース            │
├──────────────────────────────────────────────────────────┤
│  Tool Executor Layer                                      │
│  McpToolExecutor → rig_tools (Echo, FileRead/Write, Ls)  │
│  + MCP Server (JSON-RPC stdio)                           │
├──────────────────────────────────────────────────────────┤
│  SafetyContext                                            │
│  path_jail サンドボックス · コマンドブラックリスト · SSRF対策 · タイムアウト保護 │
├──────────────────────────────────────────────────────────┤
│  SQLite (WAL + FTS5)                                      │
│  Plan 永続化 · Checkpoint クラッシュリカバリ · セッション履歴 · 長期記憶    │
└──────────────────────────────────────────────────────────┘
```

### モジュール説明

| モジュール | 行数 | 責務 |
|-----------|------|------|
| `main.rs` | 700+ | CLI エントリポイント、コマンドディスパッチ、`build_engine` |
| `agent.rs` | 840+ | Agent ステートマシン、7種の Step タイプ、クラッシュリカバリ |
| `db.rs` | 890 | SQLite レイヤー、Plan CRUD + Checkpoints + FTS5 記憶 |
| `llm.rs` | 350 | LLM ゲートウェイ、Anthropic/OpenAI/Ollama 統一インターフェース |
| `cli/mod.rs` | 680 | TUI イベントループ、Agent ブリッジスレッド |
| `cli/app.rs` | 370 | TUI アプリケーション状態、セッション管理、メッセージルーティング |
| `cli/ui.rs` | 420 | TUI レンダリング：3カラムレイアウト、吹き出し、コードブロック、ステータスバー |
| `cli/handlers.rs` | 380 | 入力モード戦略（Chat/Thinking/Approval/Palette） |
| `safety.rs` | 250 | セキュリティサンドボックス、path_jail、SSRF、コマンドブラックリスト |
| `mcp.rs` | 250+ | MCP Tool ディスパッチャー + JSON-RPC クライアント |
| `mcp_server.rs` | 380 | MCP サーバー（McpToolExecutor を再利用） |
| `rig_tools.rs` | 400 | Echo / FileRead / FileWrite / ListDir ツール |
| `task.rs` | 340 | Step/Plan/Checkpoint 型定義 |
| `memory.rs` | 140 | 長期記憶（FTS5 全文検索） |
| `skill.rs` | 390 | スキルシステム（JSON ファイル + 自動学習） |
| `git.rs` | 240 | Git 統合（git2 + gh CLI） |
| `error.rs` | 34 | 統一エラー型 |

### セキュリティアーキテクチャ

| 保護層 | 実装 |
|--------|------|
| コマンドブラックリスト | 20+ の危険コマンド（sudo, rm, mkfs, dd など） |
| ファイルパスサンドボックス | `path_jail` crate、`../../etc/passwd` やシンボリックリンクのエスケープを防止 |
| SSRF 対策 | localhost/127.0.0.1/0.0.0.0/`[::1]`/169.254.x.x/nip.io をブロック |
| タイムアウト保護 | コマンド 30s / HTTP 30s / ブラウザ 30s |
| 環境変数のクリーンアップ | PATH/HOME/USER/SHELL/LANG/TERM のみ保持 |
| 出力のトランケーション | コマンド 10K / ファイル読み取り 4K |
| マルチパスセキュリティ | McpToolExecutor + LLM Agent + MCP Server の三重防御 |

---

## コア機能

### Plan 実行エンジン

7種類のステップタイプをサポート：

| ステップ | 説明 |
|---------|------|
| Think | LLM 推論、FTS5 記憶検索によるコンテキスト付き |
| ToolCall | 内蔵ツールの呼び出し（ファイル読み書き、ディレクトリ一覧、Echo） |
| WaitForInput | ユーザー入力を待ってから続行 |
| Exec | 外部コマンドの実行（セキュリティサンドボックスによる制限付き） |
| HttpRequest | HTTP GET/POST リクエスト（SSRF 対策付き） |
| BrowserAction | ブラウザ自動操作（Navigate/Screenshot/Click/GetText） |
| Finish | Plan 完了、自動的にスキル学習をトリガー |

### クラッシュリカバリ

- **ハートビート Checkpoint**：長時間操作の前に Running 状態の CP を書き込み
- **トランザクション原子性**：`record_step_completion` が単一 SQLite トランザクション内で Plan + Checkpoint を更新
- **3層リカバリ**：`reset_running_plans→get_last_checkpoint→状態に応じてリカバリポイントを決定`

### TUI

- **3カラムレイアウト**：左側にセッションリスト、中央にチャットエリア、右側にステータスパネル
- **メッセージ吹き出し**：ユーザー/アシスタント/システムの3色で識別
- **コードブロックハイライト**：コード枠線描画 + 事前折り返し
- **入力履歴**：↑/↓ で過去100件の入力をナビゲーション
- **自動スクロール**：新着メッセージが自動で最下部へスクロール、手動で遡った後もメッセージ送信で復帰
- **ウィンドウ適応**：ターミナルサイズ変更に応じて自動でレイアウト再調整、折り返し再計算

### スキルシステム

- **JSON ファイル管理**：`~/.skills/*.json`
- **内蔵スキル**：code-review, generate-readme
- **自動学習**：Plan 実行完了後に自動的に再利用可能なスキルとして抽出
- **手動学習**：`rupoo skills learn <plan_id> <skill_name>`

### 長期記憶

- **FTS5 全文検索**：BM25 関連性ランキング対応
- **セッション永続化**：SQLite に UI セッション履歴を保存
- **コンテキスト注入**：Think ステップで自動的に関連記憶を検索

---

## 依存関係

| Crate | 用途 |
|-------|------|
| tokio | 非同期ランタイム |
| clap | CLI パース |
| ratatui + crossterm | TUI フレームワーク |
| rig-core 0.30 | LLM マルチプロバイダゲートウェイ |
| rusqlite (WAL + FTS5) | SQLite データベース |
| git2 | Git 操作 |
| reqwest | HTTP クライアント |
| path_jail | ファイルパスセキュリティ |
| tui-textarea | TUI 入力コンポーネント |
| serde + serde_json | シリアライゼーション |
| tracing + tracing-subscriber | ログ |
| uuid | Plan / Step ID |
| chrono | タイムスタンプ |
| crossbeam-channel | スレッド間通信 |

---

## テスト

```bash
# 全テスト
cargo test

# ライブラリテストのみ
cargo test --lib

# 統合テストのみ
cargo test --test db_test
cargo test --test crash_recovery_test
cargo test --test cli_db_test

# 実行計画
cargo run --release demo
```

106 項目のテスト網羅：
- 54 単体テスト（Agent、DB、LLM、MCP、Safety、Memories、Skills、Git）
- 33 main crate テスト（CLI コマンド + TUI handler）
- 4 CLI-DB 統合テスト
- 2 クラッシュリカバリ統合テスト
- 13 DB 統合テスト

---

## ビルド

```bash
# 開発ビルド
cargo build

# リリースビルド（推奨）
cargo build --release

# GUI サポート付き
cargo build --release --features gui

# バイナリサイズ
# ~14 MB (release, ARM64)
```

---

## ライセンス

MIT
