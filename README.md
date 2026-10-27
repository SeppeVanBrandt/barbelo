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
| `` ` 0 ``–`` ` 9 `` | Select window by number             |
| `` ` % `` / `` ` \| `` | Split side by side               |
| `` ` " `` / `` ` - `` | Split top and bottom              |
| `` ` ←↑↓→ `` / `` ` hjkl `` | Move focus between panes    |
| `` ` o ``       | Focus next pane                         |
| `` ` x ``       | Close pane                              |
| `` ` & ``       | Close window                            |
| `` ` q ``       | Quit                                    |
