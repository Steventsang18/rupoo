//! 内部 smoke harness（P1，报告 §5.2 第 3 步）。
//!
//! 用 rupoo 自身的工具（`file_edit` / `code_search` / `run_tests`）跑通
//! 「定位 bug → 精确编辑 → 编译测试通过」的最小闭环，**不依赖 LLM**，
//! 因此可每日在 CI 中零外部依赖地持续验证「编辑→构建→测试」主干，并防止
//! P0 新增工具回归。

use rig::tool::Tool;
use rupoo::rig_tools::{CodeSearchArgs, CodeSearchTool, FileEditArgs, FileEditTool};
use rupoo::tools::verify::{RunTestsArgs, RunTestsTool};
use std::path::Path;

fn write_fixture(dir: &Path) {
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"smoke_fixture\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    // 故意写错的断言：add(2,2) 应为 4，错写成 5。
    std::fs::write(
        dir.join("src/lib.rs"),
        "pub fn add(a: i32, b: i32) -> i32 { a + b }\n\n\
         #[cfg(test)]\nmod tests {\n    use super::*;\n    #[test]\n    fn test_add() {\n        \
         assert_eq!(add(2, 2), 5);\n    }\n}\n",
    )
    .unwrap();
}

#[tokio::test]
async fn smoke_edit_build_test_loop() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write_fixture(root);

    // 1) code_search：定位失败的断言（模拟「找 bug」）。
    let search = CodeSearchTool::with_jail(root.to_path_buf());
    let found = search
        .call(CodeSearchArgs {
            pattern: "assert_eq!".into(),
            path: Some(".".into()),
            file_glob: None,
            ignore_case: None,
            max_results: None,
        })
        .await
        .unwrap();
    assert!(found.success);
    assert!(
        found.match_count >= 1,
        "code_search 应至少找到一处 assert_eq!"
    );

    // 2) file_edit：精确替换错误断言（模拟「精确编辑」）。
    let edit = FileEditTool::with_jail(root.to_path_buf());
    let edited = edit
        .call(FileEditArgs {
            path: "src/lib.rs".into(),
            old_string: "assert_eq!(add(2, 2), 5);".into(),
            new_string: "assert_eq!(add(2, 2), 4);".into(),
            replace_all: None,
        })
        .await
        .unwrap();
    assert!(edited.success, "file_edit 应成功：{:?}", edited.error);
    assert_eq!(edited.replacements, 1);

    // 3) run_tests：编译并运行测试，应当通过（模拟「构建→测试」）。
    let test = RunTestsTool;
    let result = test
        .call(RunTestsArgs {
            path: Some(root.to_string_lossy().to_string()),
        })
        .await
        .unwrap();
    assert!(
        result.success,
        "run_tests 应成功（测试应通过）：{}",
        result.output
    );
}
