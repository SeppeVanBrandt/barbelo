# barbelo

A terminal multiplexer written in Rust, with windows, split panes and a status bar.

```
cargo run --release
```

## Key bindings

All commands start with the prefix key, `` ` `` (backtick) unless
[configured](#configuration) otherwise.

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

## Configuration

barbelo reads `~/.config/barbelo/config.toml` (or
`$XDG_CONFIG_HOME/barbelo/config.toml`) at startup. Every setting is optional;
these are the defaults:

```toml
prefix = "`"                # a single key, or with Ctrl/Alt: "C-a", "M-a"

[status]
show_insert = true          # false hides the INSERT label
color_by_mode = false       # true paints the whole bar in the mode's colors
greek = false               # true shows βαρβηλώ instead of barbelo

[colors]
status_fg = "black"
status_bg = "green"
# insert_fg / insert_bg     # default to the status colors
normal_fg = "white"
normal_bg = "blue"
visual_fg = "white"
visual_bg = "magenta"
hints_fg = "black"
hints_bg = "yellow"
active_border = "green"
inactive_border = "dark-grey"

[hints]
alphabet = "asdfqwerzxcvjklmiuopghtybn"   # label letters, easiest first
label_fg = "black"
label_bg = "yellow"
match_fg = "yellow"
dim_fg = "dark-grey"

[windows]
name = "{process}"          # also {dir} (directory name) and {path} (full directory)
```

Colors are `#rrggbb`, a 256-color index such as `208`, `default`, or a name:
`black`, `red`, `green`, `yellow`, `blue`, `magenta`, `cyan`, `white`,
`grey`, and `dark-red`, `dark-green`, `dark-yellow`, `dark-blue`,
`dark-magenta`, `dark-cyan`, `dark-grey`. `red` through `white` are the
bright variants (ANSI 9–15); the `dark-` ones are the regular colors.
