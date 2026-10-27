# barbelo

A terminal multiplexer written in Rust, with windows, split panes and a status bar.

```
cargo run --release
```

## Key bindings

All commands start with the prefix key, `` ` `` (backtick).

| Keys            | Action                                  |
|-----------------|-----------------------------------------|
| `` ` ` ``       | Send a literal backtick                 |
| `` ` c ``       | New window                              |
| `` ` n `` / `` ` p `` | Next / previous window            |
| `` ` 1 ``–`` ` 9 ``, `` ` 0 `` | Select window 1–9, or 10   |
| `` ` % `` / `` ` \| `` | Split side by side               |
| `` ` " `` / `` ` - `` | Split top and bottom              |
| `` ` ←↑↓→ `` / `` ` hjkl `` | Move focus between panes    |
| `` ` o ``       | Focus next pane                         |
| `` ` ; `` / `` ` : `` | Jump back to the last active pane |
| `` ` x ``       | Close pane                              |
| `` ` & ``       | Close window                            |
| `` ` [ `` / `` ` Esc `` | Enter normal mode (scroll through output) |
| `` ` f ``       | Hint mode: copy text by typing its label |
| `` ` ] ``       | Paste the last copied text              |
| `` ` q ``       | Quit                                    |

## Modes

Each pane is in one of three modes, shown at the left of the status bar.

- **INSERT**: the regular terminal; keys go to the program in the pane.
- **NORMAL**: a frozen snapshot of the pane's history (1000 lines), navigated
  with vi keys. The program keeps running; its new output appears once you go
  back to insert mode.
- **VISUAL**: select text to copy. Copied text goes to the system clipboard
  (`wl-copy`, `xclip` or `xsel`, falling back to OSC 52) and to barbelo's
  paste buffer.

### Hint mode

Like [tmux-fingers](https://github.com/Morantron/tmux-fingers): every
interesting string in view gets a short label. Type a label to copy that
string. `Esc` cancels, `Backspace` undoes a typed letter. Works from normal mode too, on whatever part
of the history is in view.

Recognised: URLs, paths (with `:line:col`), files in `git status` output,
UUIDs, IPv4 addresses (with port), hex numbers, git SHAs and other hashes,
and numbers of 4+ digits. Matches nearest the bottom get the shortest labels,
and identical strings share one.

### Normal mode

| Keys                 | Action                                |
|----------------------|---------------------------------------|
| `h` `j` `k` `l` / arrows | Move (accepts a count, e.g. `5j`) |
| `w` `b` `e`          | Next word / previous word / word end  |
| `0` `^` `$`          | Line start / first non-blank / end    |
| `gg` / `G`           | Top / bottom of history (`12G`: line 12) |
| `H` `M` `L`          | Top / middle / bottom of view         |
| `Ctrl-u` / `Ctrl-d`  | Scroll half a page up / down          |
| `Ctrl-b` / `Ctrl-f`, PgUp / PgDn | Scroll a page up / down   |
| `Ctrl-y` / `Ctrl-e`  | Scroll the view one line              |
| `v` / `V`            | Visual mode (characters / lines)      |
| `i` `a` `q`          | Back to insert mode                   |

### Visual mode

All normal-mode motions extend the selection, plus:

| Keys           | Action                                  |
|----------------|-----------------------------------------|
| `y` / Enter    | Copy the selection, back to normal mode |
| `o`            | Jump to the other end of the selection  |
| `Esc`          | Cancel, back to normal mode             |
