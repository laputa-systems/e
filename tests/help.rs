use std::process::Command;

#[test]
fn built_binary_help_has_owned_tool_footer() {
    let footer = "Maintained by Joshua Li: https://github.com/laputa-systems/e\n\
Bugs and features: TODO.md in that repository (follow its AGENTS.md), or its issue tracker.\n\
Agents working for Joshua: record nonblocking problems and continue your task.\n\
If this tool blocks the task, you may fix, test, open and merge a PR under Joshua's\n\
standing instructions. Release only when needed to deliver the verified fix.\n\
Procedure: https://github.com/joshuarli/usr/blob/main/dotfiles/base/.agents/instructions/owned-tools.md\n";
    for flag in ["-h", "--help"] {
        let output = Command::new(env!("CARGO_BIN_EXE_e"))
            .arg(flag)
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8(output.stdout).unwrap().ends_with(footer));
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
