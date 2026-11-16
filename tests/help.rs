use std::process::Command;

#[test]
fn built_binary_help_has_owned_tool_footer() {
    let footer = "Maintained by Joshua Li: https://github.com/laputa-systems/e\n\
Bugs and features: TODO.md in that repository (follow its AGENTS.md), or its issue tracker.\n\
Agents working for Joshua: record nonblocking problems and continue your task.\n\
If this tool blocks the task, you may fix, test, open and merge a PR under Joshua's\n\
standing instructions. Release only when needed to deliver the verified fix.\n\
Procedure: https://github.com/joshuarli/usr/blob/main/dotfiles/base/.agents/instructions/owned-tools.md\n";
    let mut reference = None;
    for flag in ["-h", "--help"] {
        let output = Command::new(env!("CARGO_BIN_EXE_e"))
            .arg(flag)
            .env_remove("HOME")
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        let help = String::from_utf8(output.stdout).unwrap();
        assert!(help.starts_with("Usage: e [file]\n"));
        for part in ["Arguments:", "[file]", "Options:", "-h, --help", "-V, --version", "piped standard input", "HOME is required", "Examples:\n  e notes.txt\n  e --version\n"] {
            assert!(help.contains(part), "missing {part:?} in {help}");
        }
        assert_eq!(help.matches(footer).count(), 1);
        assert_eq!(help.matches("Maintained by Joshua Li:").count(), 1);
        assert!(help.ends_with(footer));
        if let Some(previous) = &reference {
            assert_eq!(previous, &help);
        }
        reference = Some(help);
    }
    for args in [&["--version"][..], &["one", "two"][..]] {
        let output = Command::new(env!("CARGO_BIN_EXE_e"))
            .args(args)
            .output()
            .unwrap();
        assert!(!String::from_utf8_lossy(&output.stdout).contains("Maintained by Joshua Li:"));
        assert!(!String::from_utf8_lossy(&output.stderr).contains("Maintained by Joshua Li:"));
    }
}
