use std::fs;
use std::path::PathBuf;
use anyhow::Result;
use console::style;

pub async fn run(follow: bool, lines: usize, level: Option<&str>, prev: bool) -> Result<()> {
    let path = log_path(prev);

    if !path.exists() {
        println!(
            "{} No log file found at {}",
            style("ℹ").yellow(),
            path.display()
        );
        return Ok(());
    }

    if !follow {
        println!(
            "{} Showing last {} lines from {}",
            style("→").dim(),
            lines,
            path.display()
        );
    }
    println!("{}", style("─".repeat(60)).dim());

    let content = fs::read_to_string(&path)?;
    let all_lines: Vec<&str> = content.lines().collect();
    let tail = if lines >= all_lines.len() {
        &all_lines[..]
    } else {
        &all_lines[all_lines.len() - lines..]
    };
    let filtered = filter_lines(tail, level);

    for line in &filtered {
        println!("{line}");
    }

    if follow {
        follow_file(&path, all_lines.len(), level).await?;
    }

    Ok(())
}

fn log_path(prev: bool) -> PathBuf {
    let dir = crate::tracing_setup::data_dir();
    if prev {
        dir.join("rupoo.prev.log")
    } else {
        dir.join("rupoo.log")
    }
}

fn line_has_level(line: &str, target: &str) -> bool {
    let lu = line.to_uppercase();
    let padded = format!(" {} ", target.to_uppercase());
    let start = format!("{} ", target.to_uppercase());
    lu.contains(&padded) || lu.starts_with(&start)
}

fn filter_lines<'a>(lines: &[&'a str], level: Option<&str>) -> Vec<&'a str> {
    match level {
        Some(lvl) => {
            let upper = lvl.to_uppercase();
            lines
                .iter()
                .filter(|l| match upper.as_str() {
                    "ERROR" => line_has_level(l, "ERROR"),
                    "WARN" => line_has_level(l, "WARN") || line_has_level(l, "ERROR"),
                    _ => l.to_uppercase().contains(&upper),
                })
                .copied()
                .collect()
        }
        None => lines.to_vec(),
    }
}

async fn follow_file(path: &PathBuf, start_line_count: usize, level: Option<&str>) -> Result<()> {
    let mut last_len = start_line_count;
    loop {
        if let Ok(content) = fs::read_to_string(path) {
            let all: Vec<&str> = content.lines().collect();
            if all.len() > last_len {
                let new_lines = &all[last_len..];
                let filtered = filter_lines(new_lines, level);
                for line in &filtered {
                    println!("{line}");
                }
                last_len = all.len();
            }
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_lines_none() {
        let lines = vec!["INFO  test: msg", "WARN  test: warn"];
        let result = filter_lines(&lines, None);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_filter_lines_warn() {
        let lines = vec![
            "INFO  test: msg",
            "WARN  test: warn msg",
            "ERROR test: error msg",
        ];
        let filtered = filter_lines(&lines, Some("WARN"));
        assert_eq!(filtered.len(), 2);
        assert!(filtered[0].contains("WARN"));
        assert!(filtered[1].contains("ERROR"));
    }

    #[test]
    fn test_filter_lines_error() {
        let lines = vec!["INFO  test: msg", "ERROR test: err"];
        let filtered = filter_lines(&lines, Some("ERROR"));
        assert_eq!(filtered.len(), 1);
        assert!(filtered[0].contains("ERROR"));
    }

    #[test]
    fn test_filter_lines_no_match() {
        let lines = vec!["INFO  test: msg"];
        let filtered = filter_lines(&lines, Some("WARN"));
        assert_eq!(filtered.len(), 0);
    }
}
