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
| `` ` z ``       | Zoom the active pane to fill the window, or unzoom |
| `` ` r ``       | Resize mode: move the borders between panes |
| `` ` , ``       | Rename the window (empty name: back to automatic) |
| `` ` x ``       | Close pane                              |
| `` ` & ``       | Close window                            |
| `` ` [ `` / `` ` Esc `` | Enter normal mode (scroll through output) |
| `` ` f ``       | Hint mode: copy text by typing its label |
| `` ` ] ``       | Paste the last copied text              |
| `` ` s ``       | Start the screensaver                   |
| `` ` q ``       | Quit                                    |

A zoomed window is marked `*` in the status bar, and the active window is
shown in bold. Moving focus, splitting or
closing a pane unzooms it.

## Resize mode

`` ` r `` marks every node: a point where a vertical and a horizontal border
meet, or the middle of a border that meets no other. The selected node is
highlighted, with the keys that move it shown around it.

| Keys                    | Action                                   |
|-------------------------|------------------------------------------|
| `h` `j` `k` `l` / arrows | Move the node, resizing the panes around it |
| `H` `J` `K` `L`         | Move it 5 cells at a time                |
| `Tab` / `Shift-Tab`     | Select the next / previous node          |
| `=`                     | Give every pane in the window an equal share |
| `Esc` `Enter` `q` `r`   | Leave resize mode                        |

Borders that line up, like the two halves of a `┼`, share a node and move
together.

## Screensaver

[pipes.sh](https://github.com/pipeseroni/pipes.sh) runs full screen with
`` ` s ``, or by itself after `screensaver.timeout` seconds without input.
Any key ends it; that key isn't passed on. pipes.sh needs to be installed
and in `$PATH`, or `screensaver.command` can point at the script.

## Modes

Each pane is in one of three modes. Outside insert mode, the mode's name
replaces barbelo at the left of the status bar.

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
show_insert = false         # true shows INSERT instead of barbelo in insert mode
color_by_mode = false       # true paints the whole bar in the mode's colors
greek = false               # true shows βαρβηλώ instead of barbelo
clock = false               # true shows the time at the right end of the bar
clock_format = "%H:%M"      # strftime codes, e.g. "%a %d %b %H:%M:%S"
window_format = "{number}:{name}{flags}"    # each window's entry, see below
# active_window_format      # the active window's entry; defaults to window_format
in_border = true            # with boxed borders: write the bar into the bottom border

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
resize_fg = "black"         # resize mode: the status label and the keys
resize_bg = "cyan"          # around the selected node, which is drawn in resize_bg
active_border = "green"
inactive_border = "dark-grey"
# window_fg / window_bg                 # window entries; default to the bar colors
# active_window_fg / active_window_bg   # the active window's entry

[hints]
alphabet = "asdfqwerzxcvjklmiuopghtybn"   # label letters, easiest first
label_fg = "black"
label_bg = "yellow"
match_fg = "yellow"
dim_fg = "dark-grey"

[windows]
name = "{process}"          # also {dir} (directory name) and {path} (full directory)
borders = "shared"          # or "boxed": a frame around every pane
border_chars = "light"      # light, heavy, double, rounded, block or solid
# active_border_chars       # for the active pane; defaults to border_chars

[screensaver]
command = "pipes.sh"
timeout = 0                 # seconds idle before it starts; 0: only with ` s
# These go to pipes.sh, which uses its own defaults for those not set:
# pipes = 1                 # number of pipes (-p)
# types = [0]               # pipe types (-t): 0-9, or "c" + 16 characters
# colors = [1, 2, "#ff8800"]  # colors (-c): 256-color indices or "#rrggbb"
# fps = 75                  # frames per second, 20-100 (-f)
# straight = 13             # odds 1 in N of going straight, 5-15 (-s)
# reset = 2000              # clear after N characters, 0 for never (-r)
random_start = false        # random start positions and directions (-R)
bold = true                 # false: no bold (-B)
color = true                # false: no color (-C)
keep_style = false          # keep color and type when crossing edges (-K)
args = []                   # anything else, passed on as is
```

With `borders = "boxed"`, the status bar is written into the panes' bottom
border instead of taking a line of its own:

```
└─ [barbelo] 1:barbelo:bash 2:barbelo:nvim ──────────────── 15:58 ─┘
```

The title, or the mode's name, is then drawn in the mode's color
(`active_border` in insert mode, `normal_bg`, `visual_bg` or `hints_bg`
otherwise), and the active pane's frame takes that same color. The rest of
the text uses the terminal's own colors unless `window_fg` and friends are
set. Set `in_border = false` to keep a separate bar.

Window entries in the status bar can use these placeholders:

| Placeholder | Value |
|-------------|-------|
| `{number}`  | Window number, from 1 |
| `{name}`    | The name given with `` ` , ``, or else `windows.name` filled in |
| `{flags}`   | `*` when the window is zoomed, otherwise a space |
| `{process}` | Foreground process of the active pane |
| `{dir}`     | Name of that process's working directory |
| `{path}`    | The whole working directory, with `~` for home |

For example `window_format = " {number}:{dir}:{process} "` gives
` 2:barbelo:nvim `. `windows.name` can use `{process}`, `{dir}` and `{path}`.

Colors are `#rrggbb`, a 256-color index such as `208`, `default`, or a name:
`black`, `red`, `green`, `yellow`, `blue`, `magenta`, `cyan`, `white`,
`grey`, and `dark-red`, `dark-green`, `dark-yellow`, `dark-blue`,
`dark-magenta`, `dark-cyan`, `dark-grey`. `red` through `white` are the
bright variants (ANSI 9–15); the `dark-` ones are the regular colors.
