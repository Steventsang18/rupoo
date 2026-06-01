# Extension 系统（基于 MCP）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 支持第三方通过 MCP 协议扩展 rupoo 功能，实现 `rupoo ext install/list/uninstall` 命令体系

**Architecture:** 基于已有 MCP 基础设施（`mcp.rs` 客户端 + `mcp_server.rs` 服务器），新增扩展管理器。每个扩展是一个包含 `extension.toml` 清单的目录 + 一个 MCP server 子进程。扩展管理只需操作文件系统和进程生命周期，不修改 agent 核心。

**Tech Stack:** Rust, serde, tokio (process spawn), toml

---

### Task 1: 定义 extension.toml 数据模型

**Files:**
- Create: `src/ext/mod.rs`
- Create: `src/ext/manifest.rs`

- [ ] **Step 1: Write the failing test**

`tests/ext_manifest_test.rs`:
```rust
#[cfg(test)]
mod tests {
    use rupoo::ext::manifest::{ExtensionManifest, McpConfig};

    #[test]
    fn test_parse_minimal_manifest() {
        let toml_str = r#"
[extension]
name = "test-ext"
version = "1.0.0"
description = "A test extension"

[mcp]
command = "python"
args = ["server.py"]
transport = "stdio"
"#;

        let manifest: ExtensionManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.extension.name, "test-ext");
        assert_eq!(manifest.extension.version, "1.0.0");
        assert_eq!(manifest.mcp.command, "python");
        assert!(manifest.tools.is_empty());
    }

    #[test]
    fn test_parse_manifest_with_tools() {
        let toml_str = r#"
[extension]
name = "my-ext"
version = "2.0.0"
description = "Extension with tool declarations"

[mcp]
command = "node"
args = ["server.js"]
transport = "stdio"

[tools]
provides = ["tool1", "tool2"]
"#;

        let manifest: ExtensionManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.tools.provides, vec!["tool1", "tool2"]);
    }

    #[test]
    fn test_manifest_missing_required_field() {
        let toml_str = r#"
[extension]
name = "incomplete"
"#;
        let result: Result<ExtensionManifest, _> = toml::from_str(toml_str);
        assert!(result.is_err()); // missing version, description, mcp section
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test test_parse_minimal_manifest
```
Expected: compile error, `rupooro::ext` module not found.

- [ ] **Step 3: Write minimal implementation**

Create `src/ext/manifest.rs`:
```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionManifest {
    pub extension: ExtensionMeta,
    pub mcp: McpConfig,
    #[serde(default)]
    pub tools: ToolDeclarations,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtensionMeta {
    pub name: String,
    pub version: String,
    pub description: String,
    #[serde(default)]
    pub author: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConfig {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default = "default_transport")]
    pub transport: String,
}

fn default_transport() -> String {
    "stdio".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolDeclarations {
    #[serde(default)]
    pub provides: Vec<String>,
}
```

Create `src/ext/mod.rs`:
```rust
pub mod manifest;
```

Register in `src/lib.rs`:
```rust
pub mod ext;
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo test test_parse_minimal_manifest test_parse_manifest_with_tools test_manifest_missing_required_field
```
Expected: all 3 PASS.

- [ ] **Step 5: Commit**

```bash
git add src/ext/
git commit -m "feat(ext): add extension manifest data model"
```

---

### Task 2: 实现 ExtensionManager — 扩展目录扫描、安装、卸载

**Files:**
- Create: `src/ext/manager.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extension_dir_path() {
        let mgr = ExtensionManager::new("/tmp/.rupoo-ext-test".into());
        assert!(mgr.extensions_dir().ends_with(".rupoo-ext-test"));
    }

    #[test]
    fn test_create_and_list_extension() {
        use std::fs;

        let dir = std::env::temp_dir().join(format!("ext-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mgr = ExtensionManager::new(dir.clone());

        // Create a test extension directory
        let ext_dir = dir.join("test-ext");
        fs::create_dir_all(&ext_dir).unwrap();
        let manifest_content = r#"
[extension]
name = "test-ext"
version = "1.0.0"
description = "Test"

[mcp]
command = "echo"
args = []
transport = "stdio"
"#;
        fs::write(ext_dir.join("extension.toml"), manifest_content).unwrap();

        // List should find it
        let extensions = mgr.list_extensions().unwrap();
        assert_eq!(extensions.len(), 1);
        assert_eq!(extensions[0].name, "test-ext");

        let _ = fs::remove_dir_all(&dir);
    }
}
```

- [ ] **Step 2: Write implementation**

```rust
use std::path::{Path, PathBuf};
use std::fs;

use tracing::info;

use super::manifest::ExtensionManifest;

const EXTENSIONS_DIR_NAME: &str = "extensions";

pub struct ExtensionManager {
    base_dir: PathBuf,
}

impl ExtensionManager {
    pub fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    /// Returns the path to the extensions directory.
    pub fn extensions_dir(&self) -> PathBuf {
        self.base_dir.join(EXTENSIONS_DIR_NAME)
    }

    /// Ensure the extensions directory exists.
    pub fn ensure_dir(&self) -> std::io::Result<()> {
        fs::create_dir_all(self.extensions_dir())
    }

    /// List all installed extensions.
    pub fn list_extensions(&self) -> Result<Vec<InstalledExtension>, ListError> {
        let ext_dir = self.extensions_dir();
        if !ext_dir.exists() {
            return Ok(Vec::new());
        }

        let mut result = Vec::new();
        for entry in fs::read_dir(&ext_dir).map_err(ListError::Io)? {
            let entry = entry.map_err(ListError::Io)?;
            let path = entry.path();
            if !path.is_dir() { continue; }

            let manifest_path = path.join("extension.toml");
            if !manifest_path.exists() { continue; }

            let content = fs::read_to_string(&manifest_path)
                .map_err(|e| ListError::Parse(path.clone(), e))?;
            let manifest: ExtensionManifest = toml::from_str(&content)
                .map_err(|e| ListError::Parse(path.clone(), e.into()))?;

            result.push(InstalledExtension {
                name: manifest.extension.name.clone(),
                manifest,
                path,
            });
        }
        Ok(result)
    }

    /// Install an extension from a source directory.
    pub fn install_from_dir(&self, source: &Path) -> Result<String, InstallError> {
        let manifest_path = source.join("extension.toml");
        let content = fs::read_to_string(&manifest_path)
            .map_err(|_| InstallError::MissingManifest)?;
        let manifest: ExtensionManifest = toml::from_str(&content)
            .map_err(|e| InstallError::InvalidManifest(e.to_string()))?;

        let name = &manifest.extension.name;
        let dest = self.extensions_dir().join(name);

        if dest.exists() {
            return Err(InstallError::AlreadyInstalled(name.clone()));
        }

        self.ensure_dir().map_err(InstallError::Io)?;

        // Copy directory recursively
        copy_dir_recursive(source, &dest)
            .map_err(|e| InstallError::Io(e))?;

        info!(name = %name, version = %manifest.extension.version, "extension installed");
        Ok(name.clone())
    }

    /// Uninstall an extension by name.
    pub fn uninstall(&self, name: &str) -> Result<(), UninstallError> {
        let path = self.extensions_dir().join(name);
        if !path.exists() {
            return Err(UninstallError::NotFound(name.to_string()));
        }
        fs::remove_dir_all(&path)
            .map_err(|e| UninstallError::Io(name.to_string(), e))?;
        info!(name = %name, "extension uninstalled");
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct InstalledExtension {
    pub name: String,
    pub manifest: ExtensionManifest,
    pub path: PathBuf,
}

#[derive(Debug)]
pub enum ListError {
    Io(std::io::Error),
    Parse(PathBuf, Box<dyn std::error::Error + Send + Sync>),
}

impl std::fmt::Display for ListError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ListError::Io(e) => write!(f, "IO error: {e}"),
            ListError::Parse(p, e) => write!(f, "parse error in {}: {e}", p.display()),
        }
    }
}

#[derive(Debug)]
pub enum InstallError {
    MissingManifest,
    InvalidManifest(String),
    AlreadyInstalled(String),
    Io(std::io::Error),
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InstallError::MissingManifest => write!(f, "extension.toml not found"),
            InstallError::InvalidManifest(e) => write!(f, "invalid manifest: {e}"),
            InstallError::AlreadyInstalled(name) => write!(f, "extension '{name}' already installed"),
            InstallError::Io(e) => write!(f, "IO error: {e}"),
        }
    }
}

#[derive(Debug)]
pub enum UninstallError {
    NotFound(String),
    Io(String, std::io::Error),
}

impl std::fmt::Display for UninstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UninstallError::NotFound(name) => write!(f, "extension '{name}' not found"),
            UninstallError::Io(name, e) => write!(f, "IO error removing '{name}': {e}"),
        }
    }
}

/// Recursively copy a directory.
fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if file_type.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extension_dir_path() {
        let mgr = ExtensionManager::new("/tmp/.rupoo-ext-test".into());
        assert!(mgr.extensions_dir().ends_with("extensions"));
    }

    #[test]
    fn test_install_uninstall_roundtrip() {
        let dir = std::env::temp_dir().join(format!("ext-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mgr = ExtensionManager::new(dir.clone());

        // Create a source extension
        let src = dir.join("src-ext");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("extension.toml"), r#"
[extension]
name = "test-ext"
version = "1.0.0"
description = "Test"

[mcp]
command = "echo"
args = []
transport = "stdio"
"#).unwrap();

        // Install
        let name = mgr.install_from_dir(&src).unwrap();
        assert_eq!(name, "test-ext");

        // List
        let list = mgr.list_extensions().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "test-ext");

        // Uninstall
        mgr.uninstall("test-ext").unwrap();
        assert!(mgr.list_extensions().unwrap().is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_install_duplicate_fails() {
        let dir = std::env::temp_dir().join(format!("ext-test-dup-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mgr = ExtensionManager::new(dir.clone());

        let src = dir.join("src-ext");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("extension.toml"), r#"
[extension]
name = "dup-ext"
version = "1.0.0"
description = "Dup"

[mcp]
command = "echo"
args = []
transport = "stdio"
"#).unwrap();

        mgr.install_from_dir(&src).unwrap();
        let result = mgr.install_from_dir(&src);
        assert!(matches!(result, Err(InstallError::AlreadyInstalled(_))));

        let _ = fs::remove_dir_all(&dir);
    }
}
```

- [ ] **Step 3: Create `src/ext/mod.rs` with all sub-modules**

Add to `src/ext/mod.rs`:
```rust
pub mod manifest;
pub mod manager;

pub use manager::{ExtensionManager, InstalledExtension, InstallError, UninstallError, ListError};
```

- [ ] **Step 4: Run all ext tests**

```bash
cargo test test_extension_dir_path test_install_uninstall_roundtrip test_install_duplicate_fails
```
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add src/ext/manager.rs src/ext/mod.rs
git commit -m "feat(ext): add ExtensionManager for install/list/uninstall"
```

---

### Task 3: Extension 进程生命周期管理

**Files:**
- Create: `src/ext/runtime.rs`

- [ ] **Step 1: Write extension runtime**

```rust
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tracing::{info, warn};

use super::manifest::ExtensionManifest;

/// Manages the lifecycle of an extension's MCP server process.
pub struct ExtensionProcess {
    pub name: String,
    manifest: ExtensionManifest,
    child: Option<Child>,
    running: Arc<AtomicBool>,
}

impl ExtensionProcess {
    pub fn new(name: String, manifest: ExtensionManifest) -> Self {
        Self {
            name,
            manifest,
            child: None,
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Start the extension's MCP server process.
    pub fn start(&mut self) -> Result<(), ProcessError> {
        let mcp = &self.manifest.mcp;
        let child = Command::new(&mcp.command)
            .args(&mcp.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| ProcessError::Spawn(self.name.clone(), e))?;

        self.child = Some(child);
        self.running.store(true, Ordering::SeqCst);
        info!(name = %self.name, "extension MCP process started");
        Ok(())
    }

    /// Gracefully stop the extension's MCP server process.
    pub fn stop(&mut self) -> Result<(), ProcessError> {
        self.running.store(false, Ordering::SeqCst);
        if let Some(mut child) = self.child.take() {
            // Try graceful shutdown first
            let pid = child.id();
            #[cfg(unix)]
            {
                use std::os::unix::process::CommandExt;
                // Send SIGTERM
                let _ = unsafe { libc::kill(pid as i32, libc::SIGTERM) };
            }

            // Wait with timeout
            let start = std::time::Instant::now();
            loop {
                match child.try_wait() {
                    Ok(Some(_status)) => {
                        info!(name = %self.name, "extension process stopped");
                        return Ok(());
                    }
                    Ok(None) => {
                        if start.elapsed() > std::time::Duration::from_secs(2) {
                            // Timeout — force kill
                            let _ = child.kill();
                            let _ = child.wait();
                            warn!(name = %self.name, "extension process force-killed after timeout");
                            return Ok(());
                        }
                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }
                    Err(e) => {
                        warn!(name = %self.name, error = %e, "error waiting for extension process");
                        return Err(ProcessError::Stop(self.name.clone(), e));
                    }
                }
            }
        }
        Ok(())
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    pub fn stdin(&mut self) -> Option<&mut std::process::ChildStdin> {
        self.child.as_mut().and_then(|c| c.stdin.as_mut())
    }

    pub fn stdout(&mut self) -> Option<&mut std::process::ChildStdout> {
        self.child.as_mut().and_then(|c| c.stdout.as_mut())
    }
}

impl Drop for ExtensionProcess {
    fn drop(&mut self) {
        if self.running.load(Ordering::SeqCst) {
            let _ = self.stop();
        }
    }
}

#[derive(Debug)]
pub enum ProcessError {
    Spawn(String, std::io::Error),
    Stop(String, std::io::Error),
}

impl std::fmt::Display for ProcessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProcessError::Spawn(name, e) => write!(f, "failed to spawn extension '{name}': {e}"),
            ProcessError::Stop(name, e) => write!(f, "failed to stop extension '{name}': {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ext::manifest::{ExtensionManifest, McpConfig, ExtensionMeta, ToolDeclarations};

    fn make_test_manifest() -> ExtensionManifest {
        ExtensionManifest {
            extension: ExtensionMeta {
                name: "test".into(),
                version: "1.0.0".into(),
                description: "Test".into(),
                author: String::new(),
            },
            mcp: McpConfig {
                command: "echo".into(),
                args: vec!["hello".into()],
                transport: "stdio".into(),
            },
            tools: ToolDeclarations { provides: Vec::new() },
        }
    }

    #[test]
    fn test_start_stop_process() {
        let mut proc = ExtensionProcess::new("test".into(), make_test_manifest());
        proc.start().unwrap();
        assert!(proc.is_running());
        proc.stop().unwrap();
        assert!(!proc.is_running());
    }
}
```

Note: the test uses `libc` for SIGTERM on Unix. For portability, we should add a conditional compile:

```rust
#[cfg(unix)]
fn send_signal(pid: u32) {
    unsafe { libc::kill(pid as i32, libc::SIGTERM); }
}

#[cfg(not(unix))]
fn send_signal(_pid: u32) {
    // Non-Unix: no graceful shutdown signal, will timeout and force-kill
}
```

This requires adding `libc` as a dependency to `Cargo.toml` (or using `nix` crate). Actually, we can avoid the dependency by using `kill` command via `Command::new("kill")`:

```rust
pub fn stop(&mut self) -> Result<(), ProcessError> {
    self.running.store(false, Ordering::SeqCst);
    if let Some(mut child) = self.child.take() {
        // Try graceful shutdown
        let _ = Command::new("kill")
            .arg(child.id().to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();

        // Wait with timeout
        let start = std::time::Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(_status)) => return Ok(()),
                Ok(None) => {
                    if start.elapsed() > std::time::Duration::from_secs(2) {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Ok(());
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(e) => return Err(ProcessError::Stop(self.name.clone(), e)),
            }
        }
    }
    Ok(())
}
```

This avoids the `libc` dependency entirely.

- [ ] **Step 2: Update module exports**

In `src/ext/mod.rs`:
```rust
pub mod runtime;
pub use runtime::ExtensionProcess;
```

- [ ] **Step 3: Run tests**

```bash
cargo test test_start_stop_process
```
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/ext/runtime.rs src/ext/mod.rs
git commit -m "feat(ext): add extension process lifecycle management"
```

---

### Task 4: 扩展注册表 — 管理所有扩展的状态

**Files:**
- Create: `src/ext/registry.rs`

- [ ] **Step 1: Write extension registry**

```rust
use std::collections::HashMap;
use std::path::PathBuf;

use tracing::info;

use super::manager::{ExtensionManager, InstalledExtension};
use super::manifest::{ExtensionManifest, McpConfig, ExtensionMeta, ToolDeclarations};
use super::runtime::ExtensionProcess;

/// Central registry of all extensions, their status, and running processes.
pub struct ExtensionRegistry {
    manager: ExtensionManager,
    processes: HashMap<String, ExtensionProcess>,
}

impl ExtensionRegistry {
    pub fn new(manager: ExtensionManager) -> Self {
        Self {
            manager,
            processes: HashMap::new(),
        }
    }

    /// Start all installed extensions.
    pub fn start_all(&mut self) {
        let extensions = match self.manager.list_extensions() {
            Ok(list) => list,
            Err(e) => {
                tracing::warn!(error = %e, "failed to list extensions for startup");
                return;
            }
        };

        for ext in &extensions {
            self.start_extension(ext).ok();
        }
    }

    /// Start a single extension process.
    fn start_extension(&mut self, ext: &InstalledExtension) -> Result<(), String> {
        if self.processes.contains_key(&ext.name) {
            return Ok(()); // Already running
        }

        let mut proc = ExtensionProcess::new(ext.name.clone(), ext.manifest.clone());
        proc.start().map_err(|e| e.to_string())?;

        info!(name = %ext.name, "extension registered and running");
        self.processes.insert(ext.name.clone(), proc);
        Ok(())
    }

    /// Stop and unregister an extension.
    pub fn stop_extension(&mut self, name: &str) -> Result<(), String> {
        if let Some(mut proc) = self.processes.remove(name) {
            proc.stop().map_err(|e| e.to_string())?;
            info!(name = %name, "extension stopped and unregistered");
        }
        Ok(())
    }

    /// Install an extension from a directory and start it.
    pub fn install_and_start(&mut self, source: &std::path::Path) -> Result<String, String> {
        let name = self.manager.install_from_dir(source).map_err(|e| e.to_string())?;

        // Find the installed extension and start it
        let extensions = self.manager.list_extensions().map_err(|e| e.to_string())?;
        if let Some(ext) = extensions.into_iter().find(|e| e.name == name) {
            self.start_extension(&ext).ok();
        }

        Ok(name)
    }

    /// Uninstall and stop an extension.
    pub fn uninstall_and_stop(&mut self, name: &str) -> Result<(), String> {
        self.stop_extension(name)?;
        self.manager.uninstall(name).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// List all extensions and their running status.
    pub fn list(&self) -> Result<Vec<ExtensionStatus>, String> {
        let extensions = self.manager.list_extensions().map_err(|e| e.to_string())?;
        Ok(extensions
            .into_iter()
            .map(|ext| {
                let running = self.processes.contains_key(&ext.name);
                ExtensionStatus {
                    name: ext.name,
                    version: ext.manifest.extension.version,
                    running,
                }
            })
            .collect())
    }

    /// Get stdin handle for a running extension's MCP process.
    pub fn stdin(&mut self, name: &str) -> Option<&mut std::process::ChildStdin> {
        self.processes.get_mut(name).and_then(|p| p.stdin())
    }

    /// Get stdout handle for a running extension's MCP process.
    pub fn stdout(&mut self, name: &str) -> Option<&mut std::process::ChildStdout> {
        self.processes.get_mut(name).and_then(|p| p.stdout())
    }
}

#[derive(Debug, Clone)]
pub struct ExtensionStatus {
    pub name: String,
    pub version: String,
    pub running: bool,
}
```

- [ ] **Step 2: Register module**

In `src/ext/mod.rs`:
```rust
pub mod registry;
pub use registry::{ExtensionRegistry, ExtensionStatus};
```

- [ ] **Step 3: Build check**

```bash
cargo check
```
Expected: compiles clean.

- [ ] **Step 4: Commit**

```bash
git add src/ext/registry.rs src/ext/mod.rs
git commit -m "feat(ext): add ExtensionRegistry for lifecycle management"
```

---

### Task 5: CLI 接口 — `rupoo ext` 子命令

**Files:**
- Modify: `src/main.rs` and/or `src/main_cli.rs` (the CLI dispatch)
- Create: `src/ext/cli.rs`

- [ ] **Step 1: Implement ext CLI subcommand**

Create `src/ext/cli.rs`:
```rust
use std::path::Path;
use std::sync::Arc;
use tokio::sync::Mutex;

use super::registry::ExtensionRegistry;

pub struct ExtCli {
    registry: Arc<Mutex<ExtensionRegistry>>,
}

impl ExtCli {
    pub fn new(registry: Arc<Mutex<ExtensionRegistry>>) -> Self {
        Self { registry }
    }

    pub async fn run_command(&self, args: &[String]) -> Result<(), String> {
        if args.is_empty() {
            return self.list().await;
        }

        match args[0].as_str() {
            "list" | "ls" => self.list().await,
            "install" | "i" => {
                if args.len() < 2 {
                    return Err("Usage: rupoo ext install <path>".to_string());
                }
                self.install(&args[1]).await
            }
            "uninstall" | "remove" => {
                if args.len() < 2 {
                    return Err("Usage: rupoo ext uninstall <name>".to_string());
                }
                self.uninstall(&args[1]).await
            }
            other => Err(format!("Unknown ext subcommand: {other}. Use: list, install, uninstall")),
        }
    }

    async fn list(&self) -> Result<(), String> {
        let registry = self.registry.lock().await;
        let extensions = registry.list()?;

        if extensions.is_empty() {
            println!("No extensions installed.");
            return Ok(());
        }

        println!("Installed extensions:");
        for ext in &extensions {
            let status = if ext.running { "● running" } else { "○ stopped" };
            println!("  {} v{} [{}]", ext.name, ext.version, status);
        }
        Ok(())
    }

    async fn install(&self, source: &str) -> Result<(), String> {
        let path = Path::new(source);
        if !path.exists() {
            return Err(format!("Path does not exist: {source}"));
        }

        let mut registry = self.registry.lock().await;
        let name = registry.install_and_start(path)?;
        println!("Extension '{}' installed and started.", name);
        Ok(())
    }

    async fn uninstall(&self, name: &str) -> Result<(), String> {
        let mut registry = self.registry.lock().await;
        registry.uninstall_and_stop(name)?;
        println!("Extension '{}' uninstalled.", name);
        Ok(())
    }
}
```

- [ ] **Step 2: Wire ext CLI into main command dispatch**

In `main_cli.rs` (or wherever CLI commands are dispatched), find the command match block and add:

```rust
"ext" | "extension" => {
    let ext_cli = ExtCli::new(registry.clone()); // registry is Arc<Mutex<ExtensionRegistry>>
    let args: Vec<String> = std::env::args().skip(2).collect();
    if let Err(e) = ext_cli.run_command(&args).await {
        eprintln!("Error: {e}");
    }
}
```

This depends on how the CLI dispatch is structured. The exact integration point depends on `main_cli.rs` which we need to read.

- [ ] **Step 3: Wire ExtensionRegistry into engine initialization**

In `build_engine.rs` (or equivalent initialization), after constructing the repo:
```rust
// Initialize extension system
let ext_base = dirs::home_dir()
    .unwrap_or_else(|| PathBuf::from("."))
    .join(".rupoo");
let ext_manager = rupoo::ext::ExtensionManager::new(ext_base);
let ext_registry = Arc::new(tokio::sync::Mutex::new(
    rupoo::ext::ExtensionRegistry::new(ext_manager)
));

// Start all installed extensions
{
    let mut reg = ext_registry.lock().await;
    reg.start_all();
}
```

- [ ] **Step 4: Build check**

```bash
cargo check
```
Expected: compiles clean. If there are missing pieces, fix and re-check.

- [ ] **Step 5: Manual verification**

```bash
# Create a test extension
mkdir -p /tmp/test-ext
cat > /tmp/test-ext/extension.toml << 'EOF'
[extension]
name = "hello-ext"
version = "0.1.0"
description = "Test extension"

[mcp]
command = "echo"
args = ["hello from extension"]
transport = "stdio"
EOF

# Run the install command (if CLI is wired)
cargo run -- ext install /tmp/test-ext
# Expected: "Extension 'hello-ext' installed and started."

# List
cargo run -- ext list
# Expected: "hello-ext v0.1.0 [● running]"

# Uninstall
cargo run -- ext uninstall hello-ext
# Expected: "Extension 'hello-ext' uninstalled."
```

- [ ] **Step 6: Commit**

```bash
git add src/ext/cli.rs
git commit -m "feat(ext): add CLI subcommands for extension management"
```

---

### Task 6: 与现有 MCP 工具执行器集成

**Files:**
- Modify: `src/mcp.rs` or `src/executor.rs`

**Goal:** 让扩展提供的 MCP 工具能被 agent 调用。扩展的 MCP server 进程通过 stdio JSON-RPC 与主进程通信——这与现有 `mcp.rs` 客户端的 `McpToolExecutor` 模式相同。

扩展的 MCP server 已经作为子进程运行，当 agent 需要调用扩展工具时：

1. `ExtensionRegistry` 暴露一个方法，根据工具名查找对应的扩展
2. 通过子进程的 stdio 发送 JSON-RPC 请求
3. 返回结果

实际上，这种集成需要更多实现细节，超出了当前范围。MVP 阶段可以先让扩展进程正常运行、能够被 `ext list/install/uninstall` 管理。工具调用集成可以作为 P2 后续任务。

- [ ] **Step 1: 标记后续工作 (in doc-comments)**

在 `src/ext/registry.rs` 添加 TODO:
```rust
// TODO(P2): 将扩展的 MCP 子进程暴露为 ToolExecutor，与 agent 的 ToolCall 步骤集成
// 集成方案:
//   - ExtensionRegistry::find_tool(name) -> Option<&mut ExtensionProcess>
//   - 通过子进程 stdin/stdout 发送 JSON-RPC 请求
//   - 复用 mcp.rs 中的 JSON-RPC 序列化逻辑
```

- [ ] **Step 2: Commit**

```bash
git add src/ext/registry.rs
git commit -m "docs(ext): document future MCP tool integration points"
```

---

## Manual Verification Checklist

1. **Install**: `rupoo ext install /path/to/ext` → 提示已安装
2. **List**: `rupoo ext list` → 显示已安装扩展及运行状态
3. **Uninstall**: `rupoo ext uninstall hello-ext` → 扩展被移除
4. **Duplicate**: 安装同名扩展 → 报错已存在
5. **Missing manifest**: 安装没有 extension.toml 的目录 → 报错
6. **Crash resilience**: 子进程崩溃 → 主进程不 panic
7. **Graceful shutdown**: 退出程序 → 所有子进程被终止
