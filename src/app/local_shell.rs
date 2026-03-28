// SPDX-License-Identifier: MIT OR Apache-2.0

use std::collections::HashSet;
use std::env;
use std::path::Path;

use crate::app::LocalShellOption;

pub fn detect_local_shells() -> Vec<LocalShellOption> {
    let mut shells = Vec::new();
    let mut dedup = HashSet::new();

    let mut push_shell = |name: &str, program: String, args: Vec<String>| {
        let key = program.to_lowercase();
        if dedup.insert(key) {
            shells.push(LocalShellOption {
                name: name.to_string(),
                program,
                args,
            });
        }
    };

    if let Ok(comspec) = env::var("COMSPEC") {
        if Path::new(&comspec).exists() {
            push_shell("Command Prompt (COMSPEC)", comspec, vec![]);
        }
    }

    let candidates = [
        ("PowerShell 7", "pwsh.exe", vec!["-NoLogo", "-NoExit"]),
        ("Windows PowerShell", "powershell.exe", vec!["-NoLogo", "-NoExit"]),
        ("Command Prompt", "cmd.exe", vec![]),
        ("Bash", "bash.exe", vec!["--login", "-i"]),
    ];

    for (name, exe, args) in candidates {
        if let Some(program) = resolve_executable(exe) {
            push_shell(name, program, args.iter().map(|s| s.to_string()).collect());
        }
    }

    if shells.is_empty() {
        shells.push(LocalShellOption {
            name: "Windows PowerShell (fallback)".to_string(),
            program: "powershell.exe".to_string(),
            args: vec!["-NoLogo".to_string(), "-NoExit".to_string()],
        });
    }

    shells
}

fn resolve_executable(exe_name: &str) -> Option<String> {
    let candidate = Path::new(exe_name);
    if candidate.is_absolute() && candidate.exists() {
        return Some(exe_name.to_string());
    }

    if let Ok(path_var) = env::var("PATH") {
        for dir in env::split_paths(&path_var) {
            let full = dir.join(exe_name);
            if full.exists() {
                return Some(full.to_string_lossy().into_owned());
            }
        }
    }

    None
}