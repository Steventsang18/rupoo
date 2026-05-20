# Rupoo — AI 기반 터미널 어시스턴트

Rupoo는 터미널에서 실행되는 AI 어시스턴트로, 계획 실행, 스킬 관리, 장기 기억, 보안 샌드박스, Git 통합 및 MCP 프로토콜을 지원합니다—모든 것을 자연어 또는 TUI로 조작할 수 있습니다.

```
Version:  0.2.0        Language: Rust 2021
Tests:    106 ✅       Binary:   ~14 MB (release, ARM64)
TUI:      ratatui      LLM:      Anthropic / OpenAI / DeepSeek / Ollama
DB:       SQLite (FTS5)  Safety:  path_jail 샌드박스 + SSRF 보호
```

---

## 빠른 시작

### 설치

```bash
# 소스코드에서 설치
cargo install --path .

# 또는 컴파일된 바이너리 직접 실행
cargo run --release
```

### LLM 설정

```bash
# Anthropic Claude
rupoo config set api_key.anthropic sk-ant-xxx
rupoo config set model.anthropic claude-sonnet-4-20250514

# OpenAI / DeepSeek 등 호환 인터페이스
rupoo config set api_key.openai sk-xxx
rupoo config set model.openai deepseek-chat
rupoo config set base_url.openai https://api.deepseek.com/v1

# Ollama 로컬 모델
# API Key 불필요, Ollama 기본값 http://localhost:11434
```

### 실행

```bash
# 인터랙티브 TUI (기본)
rupoo

# TUI 단축키
# Ctrl+P   명령 팔레트
# Ctrl+C   종료
# Tab      포커스 전환 (입력 영역 ↔ 사이드바)
# ↑/↓       입력 기록
# Shift+↑/↓   채팅 영역 스크롤 (또는 마우스 휠)
# PgUp/PgDn   대량 스크롤
```

---

## 명령줄 인터페이스

```
rupoo [OPTIONS] [COMMAND]
```

### 글로벌 옵션

| 옵션 | 설명 |
|------|------|
| `--verbose` | stderr에 디버그 로그 출력 |

### 서브 명령어

| 명령어 | 설명 |
|------|------|
| _(없음)_ | 인터랙티브 TUI 진입 (3단 레이아웃) |
| `run --task <id>` | 저장된 Plan 실행 |
| `demo` | 내장 데모 Plan 실행 |
| `status [--short]` | 시스템 상태 개요 표시 |
| `model [show\|list\|set]` | LLM 제공자 및 모델 확인/전환 |
| `session [list\|show\|resume\|delete\|prune]` | 실행 계획 관리 |
| `skills [list\|show\|run\|install-builtin\|learn]` | 스킬 시스템 관리 |
| `config [set\|get\|list]` | 설정 관리 및 API Keys |
| `git [status\|commit\|pr]` | Git 통합 |
| `doctor [--fix]` | 환경 및 설정 진단 |
| `logs [--follow] [--lines N] [--level LEVEL]` | 실행 로그 확인 |
| `mcp-server` | MCP 프로토콜 서버 시작 (JSON-RPC over stdio) |
| `serve --port <port>` | 서버 모드 |

---

## 아키텍처

```
┌─ CLI (clap) ─────────────────────────────────────────────┐
│  rupoo  →  TUI (ratatui + crossterm)                     │
│         →  서브 명령어 (status/model/session/doctor/logs...) │
└──────────────────────┬───────────────────────────────────┘
                       │
┌──────────────────────▼───────────────────────────────────┐
│  Agent 상태 머신                                           │
│  Think → ToolCall → WaitForInput → Finish                │
│  + Exec / HttpRequest / BrowserAction                     │
├──────────────────────────────────────────────────────────┤
│  LLM Gateway (rig-core)                                   │
│  Anthropic / OpenAI / Ollama 통합 인터페이스                │
├──────────────────────────────────────────────────────────┤
│  Tool Executor Layer                                      │
│  McpToolExecutor → rig_tools (Echo, FileRead/Write, Ls)  │
│  + MCP Server (JSON-RPC stdio)                           │
├──────────────────────────────────────────────────────────┤
│  SafetyContext                                            │
│  path_jail 샌드박스 · 명령어 블랙리스트 · SSRF 보호 · 타임아웃 보호 │
├──────────────────────────────────────────────────────────┤
│  SQLite (WAL + FTS5)                                      │
│  Plan 영속화 · Checkpoint 충돌 복구 · 세션 기록 · 장기 기억     │
└──────────────────────────────────────────────────────────┘
```

### 모듈 설명

| 모듈 | 라인 수 | 역할 |
|------|------|------|
| `main.rs` | 700+ | CLI 진입점, 명령어 디스패치, `build_engine` |
| `agent.rs` | 840+ | Agent 상태 머신, 7가지 Step 유형, 충돌 복구 |
| `db.rs` | 890 | SQLite 계층, Plan CRUD + Checkpoints + FTS5 기억 |
| `llm.rs` | 350 | LLM 게이트웨이, Anthropic/OpenAI/Ollama 통합 |
| `cli/mod.rs` | 680 | TUI 이벤트 루프, Agent 브리지 스레드 |
| `cli/app.rs` | 370 | TUI 애플리케이션 상태, 세션 관리, 메시지 라우팅 |
| `cli/ui.rs` | 420 | TUI 렌더링: 3단 레이아웃, 버블, 코드 블록, 상태 표시줄 |
| `cli/handlers.rs` | 380 | 입력 모드 전략 (Chat/Thinking/Approval/Palette) |
| `safety.rs` | 250 | 보안 샌드박스, path_jail, SSRF, 명령어 블랙리스트 |
| `mcp.rs` | 250+ | MCP Tool 스케줄러 + JSON-RPC 클라이언트 |
| `mcp_server.rs` | 380 | MCP 서버 (McpToolExecutor 재사용) |
| `rig_tools.rs` | 400 | Echo / FileRead / FileWrite / ListDir 도구 |
| `task.rs` | 340 | Step/Plan/Checkpoint 타입 정의 |
| `memory.rs` | 140 | 장기 기억 (FTS5 전문 검색) |
| `skill.rs` | 390 | 스킬 시스템 (JSON 파일 + 자동 학습) |
| `git.rs` | 240 | Git 통합 (git2 + gh CLI) |
| `error.rs` | 34 | 통합 에러 타입 |

### 보안 아키텍처

| 보호 계층 | 구현 |
|--------|------|
| 명령어 블랙리스트 | 20+ 위험 명령어 (sudo, rm, mkfs, dd 등) |
| 파일 경로 샌드박스 | `path_jail` crate, `../../etc/passwd` 및 심볼릭 링크 탈출 방지 |
| SSRF 보호 | localhost/127.0.0.1/0.0.0.0/`[::1]`/169.254.x.x/nip.io 차단 |
| 타임아웃 보호 | 명령어 30s / HTTP 30s / 브라우저 30s |
| 환경 변수 정리 | PATH/HOME/USER/SHELL/LANG/TERM만 유지 |
| 출력 제한 | 명령어 10K / 파일 읽기 4K |
| 다중 경로 보안 | McpToolExecutor + LLM Agent + MCP Server 3중 보호 |

---

## 핵심 기능

### Plan 실행 엔진

7가지 Step 유형 지원:

| 단계 | 설명 |
|------|------|
| Think | LLM 추론, FTS5 기억 검색으로 컨텍스트 제공 |
| ToolCall | 내장 도구 호출 (파일 읽기/쓰기, 디렉토리 목록, Echo) |
| WaitForInput | 사용자 입력 대기 후 계속 |
| Exec | 외부 명령어 실행 (보안 샌드박스 제한 적용) |
| HttpRequest | HTTP GET/POST 요청 (SSRF 보호 적용) |
| BrowserAction | 브라우저 자동화 (Navigate/Screenshot/Click/GetText) |
| Finish | Plan 완료, 스킬 학습 자동 트리거 |

### 충돌 복구

- **하트비트 Checkpoint**: 장시간 작업 전 Running 상태 CP 기록
- **트랜잭션 원자성**: `record_step_completion`이 단일 SQLite 트랜잭션에서 Plan + Checkpoint 업데이트
- **3단계 복구**: `reset_running_plans→get_last_checkpoint→상태에 따라 복구 지점 결정`

### TUI

- **3단 레이아웃**: 왼쪽 세션 목록, 중앙 채팅 영역, 오른쪽 상태 패널
- **메시지 버블**: 사용자/어시스턴트/시스템 3색 구분
- **코드 블록 하이라이트**: 코드 테두리 렌더링 + 사전 줄바꿈
- **입력 기록**: ↑/↓ 로 최근 100개 입력 탐색
- **자동 스크롤**: 새 메시지 도착 시 자동으로 하단 스크롤, 수동 스크롤 후 메시지 전송 시 복원
- **창 자동 크기 조정**: 터미널 크기 변경 시 자동 재배치 및 재줄바꿈

### 스킬 시스템

- **JSON 파일 관리**: `~/.skills/*.json`
- **내장 스킬**: code-review, generate-readme
- **자동 학습**: Plan 실행 완료 후 재사용 가능한 스킬로 자동 추출
- **수동 학습**: `rupoo skills learn <plan_id> <skill_name>`

### 장기 기억

- **FTS5 전문 검색**: BM25 관련성 정렬 지원
- **세션 영속화**: SQLite에 UI 세션 기록 저장
- **컨텍스트 주입**: Think 단계에서 관련 기억 자동 검색

---

## 의존성

| Crate | 용도 |
|-------|------|
| tokio | 비동기 런타임 |
| clap | CLI 파싱 |
| ratatui + crossterm | TUI 프레임워크 |
| rig-core 0.30 | LLM 멀티 제공자 게이트웨이 |
| rusqlite (WAL + FTS5) | SQLite 데이터베이스 |
| git2 | Git 작업 |
| reqwest | HTTP 클라이언트 |
| path_jail | 파일 경로 보안 |
| tui-textarea | TUI 입력 컴포넌트 |
| serde + serde_json | 직렬화 |
| tracing + tracing-subscriber | 로깅 |
| uuid | Plan / Step ID |
| chrono | 타임스탬프 |
| crossbeam-channel | 스레드 간 통신 |

---

## 테스트

```bash
# 전체 테스트
cargo test

# 라이브러리 테스트만
cargo test --lib

# 통합 테스트만
cargo test --test db_test
cargo test --test crash_recovery_test
cargo test --test cli_db_test

# 실행 계획
cargo run --release demo
```

106개 테스트가 다음을 커버합니다:
- 54 단위 테스트 (Agent, DB, LLM, MCP, Safety, Memories, Skills, Git)
- 33 main crate 테스트 (CLI 명령어 + TUI handler)
- 4 CLI-DB 통합 테스트
- 2 충돌 복구 통합 테스트
- 13 DB 통합 테스트

---

## 빌드

```bash
# 개발 빌드
cargo build

# 릴리스 빌드 (권장)
cargo build --release

# GUI 지원 포함
cargo build --release --features gui

# 바이너리 크기
# ~14 MB (release, ARM64)
```

---

## 라이선스

MIT
