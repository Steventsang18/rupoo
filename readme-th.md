# Rupoo — ผู้ช่วย Terminal ขับเคลื่อนด้วย AI

Rupoo คือผู้ช่วย AI ที่ทำงานใน Terminal รองรับการดำเนินการตามแผน (Plan Execution) การจัดการสกิล หน่วยความจำระยะยาว Sandbox ความปลอดภัย Git Integration และ MCP Protocol — ทั้งหมดผ่านภาษาธรรมชาติหรือการโต้ตอบผ่าน TUI

```
Version:  0.2.0        Language: Rust 2021
Tests:    106 ✅       Binary:   ~14 MB (release, ARM64)
TUI:      ratatui      LLM:      Anthropic / OpenAI / DeepSeek / Ollama
DB:       SQLite (FTS5)  Safety:  path_jail Sandbox + SSRF Protection
```

---

## เริ่มต้นใช้งาน

### การติดตั้ง

```bash
# ติดตั้งจากซอร์สโค้ด
cargo install --path .

# หรือรันไบนารีที่คอมไพล์แล้วโดยตรง
cargo run --release
```

### การกำหนดค่า LLM

```bash
# Anthropic Claude
rupoo config set api_key.anthropic sk-ant-xxx
rupoo config set model.anthropic claude-sonnet-4-20250514

# OpenAI / DeepSeek และอินเทอร์เฟซที่เข้ากันได้
rupoo config set api_key.openai sk-xxx
rupoo config set model.openai deepseek-chat
rupoo config set base_url.openai https://api.deepseek.com/v1

# Ollama โมเดลท้องถิ่น
# ไม่ต้องใช้ API Key, Ollama ค่าเริ่มต้น http://localhost:11434
```

### การเริ่มต้น

```bash
# โหมด TUI แบบโต้ตอบ (ค่าเริ่มต้น)
rupoo

# ปุ่มลัด TUI
# Ctrl+P    แผงคำสั่ง
# Ctrl+C    ออก
# Tab       สลับโฟกัส (พื้นที่ป้อนข้อมูล ↔ แถบข้าง)
# ↑/↓       ประวัติการป้อนข้อมูล
# Shift+↑/↓   เลื่อนพื้นที่แชท (หรือ scroll เมาส์)
# PgUp/PgDn   เลื่อนครั้งละมากๆ
```

---

## อินเทอร์เฟซบรรทัดคำสั่ง

```
rupoo [OPTIONS] [COMMAND]
```

### ตัวเลือกระดับโลก

| ตัวเลือก | คำอธิบาย |
|---------|-----------|
| `--verbose` | แสดง log การดีบักบน stderr |

### คำสั่งย่อย

| คำสั่ง | คำอธิบาย |
|-------|-----------|
| _(ไม่มี)_ | เข้าสู่ TUI แบบโต้ตอบ (สามคอลัมน์) |
| `run --task <id>` | ดำเนินการ Plan ที่บันทึกไว้ |
| `demo` | รัน Plan สาธิตในตัว |
| `status [--short]` | แสดงภาพรวมสถานะระบบ |
| `model [show\|list\|set]` | ดู/เปลี่ยนผู้ให้บริการ LLM และโมเดล |
| `session [list\|show\|resume\|delete\|prune]` | จัดการแผนการดำเนินการ |
| `skills [list\|show\|run\|install-builtin\|learn]` | จัดการระบบสกิล |
| `config [set\|get\|list]` | จัดการการกำหนดค่าและ API Keys |
| `git [status\|commit\|pr]` | Git Integration |
| `doctor [--fix]` | วินิจฉัยปัญหาแวดล้อมและการกำหนดค่า |
| `logs [--follow] [--lines N] [--level LEVEL]` | ดูบันทึกการทำงาน |
| `mcp-server` | เริ่มเซิร์ฟเวอร์ MCP Protocol (JSON-RPC over stdio) |
| `serve --port <port>` | โหมดเซิร์ฟเวอร์ |

---

## สถาปัตยกรรม

```
┌─ CLI (clap) ─────────────────────────────────────────────┐
│  rupoo  →  TUI (ratatui + crossterm)                     │
│         →  คำสั่งย่อย (status/model/session/doctor/logs...)│
└──────────────────────┬───────────────────────────────────┘
                       │
┌──────────────────────▼───────────────────────────────────┐
│  Agent State Machine                                     │
│  Think → ToolCall → WaitForInput → Finish               │
│  + Exec / HttpRequest / BrowserAction                    │
├──────────────────────────────────────────────────────────┤
│  LLM Gateway (rig-core)                                  │
│  อินเทอร์เฟซรวม Anthropic / OpenAI / Ollama              │
├──────────────────────────────────────────────────────────┤
│  Tool Executor Layer                                     │
│  McpToolExecutor → rig_tools (Echo, FileRead/Write, Ls) │
│  + MCP Server (JSON-RPC stdio)                          │
├──────────────────────────────────────────────────────────┤
│  SafetyContext                                           │
│  path_jail Sandbox · รายการคำสั่งต้องห้าม · SSRF Protection │
├──────────────────────────────────────────────────────────┤
│  SQLite (WAL + FTS5)                                     │
│  การคงอยู่ของ Plan · การกู้คืน Checkpoint · ประวัติเซสชัน  │
└──────────────────────────────────────────────────────────┘
```

### คำอธิบายโมดูล

| โมดูล | จำนวนบรรทัด | หน้าที่ |
|-------|------------|--------|
| `main.rs` | 700+ | จุดเข้า CLI, การกระจายคำสั่ง, `build_engine` |
| `agent.rs` | 840+ | Agent State Machine, 7 ประเภท Step, การกู้คืนจากข้อผิดพลาด |
| `db.rs` | 890 | ชั้น SQLite, Plan CRUD + Checkpoints + FTS5 Memory |
| `llm.rs` | 350 | เกตเวย์ LLM, รวม Anthropic/OpenAI/Ollama |
| `cli/mod.rs` | 680 | Event Loop TUI, เธรดเชื่อมต่อ Agent |
| `cli/app.rs` | 370 | สถานะแอปพลิเคชัน TUI, การจัดการเซสชัน, การจัดเส้นทางข้อความ |
| `cli/ui.rs` | 420 | การเรนเดอร์ TUI: สามคอลัมน์, บอลลูน, บล็อกโค้ด, แถบสถานะ |
| `cli/handlers.rs` | 380 | กลยุทธ์โหมดอินพุต (Chat/Thinking/Approval/Palette) |
| `safety.rs` | 250 | Sandbox ความปลอดภัย, path_jail, SSRF, รายการคำสั่งต้องห้าม |
| `mcp.rs` | 250+ | MCP Tool Scheduler + JSON-RPC Client |
| `mcp_server.rs` | 380 | MCP Server (ใช้ McpToolExecutor ซ้ำ) |
| `rig_tools.rs` | 400 | เครื่องมือ Echo / FileRead / FileWrite / ListDir |
| `task.rs` | 340 | นิยามประเภท Step/Plan/Checkpoint |
| `memory.rs` | 140 | หน่วยความจำระยะยาว (FTS5 Full-text Search) |
| `skill.rs` | 390 | ระบบสกิล (ไฟล์ JSON + การเรียนรู้อัตโนมัติ) |
| `git.rs` | 240 | Git Integration (git2 + gh CLI) |
| `error.rs` | 34 | ประเภทข้อผิดพลาดรวมศูนย์ |

### สถาปัตยกรรมความปลอดภัย

| ชั้นป้องกัน | การนำไปใช้ |
|------------|-----------|
| รายการคำสั่งต้องห้าม | 20+ คำสั่งอันตราย (sudo, rm, mkfs, dd ฯลฯ) |
| Sandbox พาธไฟล์ | Crate `path_jail`, ป้องกัน `../../etc/passwd`, การหลบหนี symlink |
| SSRF Protection | ปิดกั้น localhost/127.0.0.1/0.0.0.0/`[::1]`/169.254.x.x/nip.io |
| Timeout Protection | คำสั่ง 30s / HTTP 30s / เบราว์เซอร์ 30s |
| การล้างตัวแปรสภาพแวดล้อม | เก็บเฉพาะ PATH/HOME/USER/SHELL/LANG/TERM |
| การตัดเอาต์พุต | คำสั่ง 10K / การอ่านไฟล์ 4K |
| ความปลอดภัยหลายพาธ | การป้องกันสามชั้น McpToolExecutor + LLM Agent + MCP Server |

---

## คุณสมบัติหลัก

### Plan Execution Engine

รองรับ 7 ประเภทขั้นตอน:

| ขั้นตอน | คำอธิบาย |
|--------|-----------|
| Think | การอนุมานของ LLM พร้อมการดึงข้อมูลหน่วยความจำ FTS5 เป็นบริบท |
| ToolCall | เรียกใช้เครื่องมือในตัว (อ่าน/เขียนไฟล์, รายการไดเรกทอรี, Echo) |
| WaitForInput | รอรับอินพุตจากผู้ใช้ก่อนดำเนินการต่อ |
| Exec | ดำเนินการคำสั่งภายนอก (ภายใต้ข้อจำกัดของ Sandbox ความปลอดภัย) |
| HttpRequest | คำขอ HTTP GET/POST (พร้อม SSRF Protection) |
| BrowserAction | อัตโนมัติเบราว์เซอร์ (Navigate/Screenshot/Click/GetText) |
| Finish | เสร็จสิ้นแผน, เรียกใช้การเรียนรู้สกิลอัตโนมัติ |

### การกู้คืนจากข้อผิดพลาด

- **Heartbeat Checkpoint**: เขียน CP สถานะ Running ก่อนดำเนินการที่ใช้เวลานาน
- **Atomic Transaction**: `record_step_completion` อัปเดต Plan + Checkpoint ในธุรกรรม SQLite เดียว
- **การกู้คืนสามชั้น**: `reset_running_plans→get_last_checkpoint→ตัดสินใจจุดกู้คืนตามสถานะ`

### TUI

- **เค้าโครงสามคอลัมน์**: รายการเซสชันซ้าย, พื้นที่แชทกลาง, แผงสถานะขวา
- **บอลลูนข้อความ**: สามสีแยกผู้ใช้/ผู้ช่วย/ระบบ
- **การเน้นบล็อกโค้ด**: เส้นขอบโค้ด + การตัดบรรทัดล่วงหน้า
- **ประวัติอินพุต**: นำทาง ↑/↓ ใน 100 รายการล่าสุด
- **เลื่อนอัตโนมัติ**: ข้อความใหม่เลื่อนลงล่างอัตโนมัติ, ส่งข้อความแล้วกลับมาเลื่อนอัตโนมัติหลังเลื่อนด้วยมือ
- **ปรับขนาดหน้าต่างอัตโนมัติ**: จัดเรียงและตัดบรรทัดใหม่เมื่อขนาด Terminal เปลี่ยน

### ระบบสกิล

- **จัดการไฟล์ JSON**: `~/.skills/*.json`
- **สกิลในตัว**: code-review, generate-readme
- **การเรียนรู้อัตโนมัติ**: หลัง Plan เสร็จสิ้น สกัดเป็นสกิลที่ใช้ซ้ำได้อัตโนมัติ
- **การเรียนรู้ด้วยตนเอง**: `rupoo skills learn <plan_id> <skill_name>`

### หน่วยความจำระยะยาว

- **FTS5 Full-text Search**: รองรับการจัดอันดับความเกี่ยวข้อง BM25
- **การคงอยู่ของเซสชัน**: SQLite เก็บประวัติเซสชัน UI
- **การแทรกบริบท**: ขั้นตอน Think ดึงข้อมูลหน่วยความจำที่เกี่ยวข้องอัตโนมัติ

---

## ไลบรารีที่ใช้

| Crate | การใช้งาน |
|-------|---------|
| tokio | รันไทม์แบบอะซิงโครนัส |
| clap | การแยกวิเคราะห์ CLI |
| ratatui + crossterm | เฟรมเวิร์ก TUI |
| rig-core 0.30 | เกตเวย์ผู้ให้บริการ LLM หลายราย |
| rusqlite (WAL + FTS5) | ฐานข้อมูล SQLite |
| git2 | การดำเนินการ Git |
| reqwest | HTTP Client |
| path_jail | ความปลอดภัยพาธไฟล์ |
| tui-textarea | ส่วนประกอบอินพุต TUI |
| serde + serde_json | การทำให้เป็นอนุกรม |
| tracing + tracing-subscriber | บันทึก |
| uuid | Plan / Step ID |
| chrono | การประทับเวลา |
| crossbeam-channel | การสื่อสารข้ามเธรด |

---

## การทดสอบ

```bash
# ทดสอบทั้งหมด
cargo test

# ทดสอบเฉพาะไลบรารี
cargo test --lib

# ทดสอบเฉพาะ Integration
cargo test --test db_test
cargo test --test crash_recovery_test
cargo test --test cli_db_test

# รัน Plan
cargo run --release demo
```

106 รายการทดสอบครอบคลุม:
- 54 Unit Tests (Agent, DB, LLM, MCP, Safety, Memories, Skills, Git)
- 33 Main Crate Tests (CLI Commands + TUI Handler)
- 4 CLI-DB Integration Tests
- 2 Crash Recovery Integration Tests
- 13 DB Integration Tests

---

## การสร้าง

```bash
# Development Build
cargo build

# Release Build (แนะนำ)
cargo build --release

# พร้อม GUI Support
cargo build --release --features gui

# ขนาดไบนารี
# ~14 MB (release, ARM64)
```

---

## สัญญาอนุญาต

MIT
