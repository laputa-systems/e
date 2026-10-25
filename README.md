# e

e is a minimalist terminal text editor for macOS and Linux. It edits one file at a time.

## Install

Download a pre-release binary for macOS arm64 or Linux x86_64/aarch64 from [Releases](https://github.com/laputa-systems/e/releases). Linux binaries are statically linked. Extract the archive and put the executable on your `PATH`.

## Run

Run e in an interactive terminal:

```sh
e [file]
```

With no file argument, e opens an unnamed buffer. It can also read a document from standard input:

```sh
cat notes.txt | e
```

Standard input also opens an unnamed buffer, so `Ctrl+S` prompts for a filename. e asks before opening files larger than 5 MB or files it detects as likely binary.

Use `e --version` to print the version.

`Ctrl+S` saves. An unnamed buffer prompts for a filename. Saving removes trailing spaces and tabs on each line and adds a final newline. If the file changed on disk since it was opened, e refuses to overwrite it. `Ctrl+Q` quits; if the buffer has unsaved changes, e asks whether to save or discard them.

`Ctrl+P` opens the command palette. Commands include `save [file]`, `quit`, `goto <line>`, `find <pattern>`, `replaceall <pattern> <replacement>`, `trim`, `tabstospaces`, and `spacestotabs`.

Common keys include `Ctrl+Z`/`Ctrl+Y` for undo/redo, `Ctrl+A` to select all, `Ctrl+C`/`Ctrl+X`/`Ctrl+V` for copy/cut/paste, and `Ctrl+F` to find. Clipboard commands use the system clipboard when available and an internal clipboard otherwise.

## Configuration

Create `$HOME/.config/e/keybindings.ini` to override Ctrl-key bindings. For example:

```ini
[keybindings]
ctrl+s = save
ctrl+q = quit
ctrl+f = find
```

Keys use `ctrl+` followed by one character. Supported actions are `save`, `quit`, `undo`, `redo`, `selectall`, `copy`, `cut`, `paste`, `killline`, `gototop`, `gotoend`, `toggleruler`, `commandpalette`, `gotoline`, `find`, `ctrlbackspace`, `togglecomment`, `duplicateline`, and `selectword`.

e stores key bindings, file locks, undo history, and the last cursor position under `$HOME/.config/e`. `HOME` must be set for these files; e has no shared temporary-directory fallback.

## Help and license

Report problems at [GitHub Issues](https://github.com/laputa-systems/e/issues). e is licensed under the [MIT License](LICENSE).
