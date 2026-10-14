# Pilha

Pilha is a local todo list with an AI step. You type a todo as free text, with GitHub PR or ClickUp task links and the priority in your own words, and DeepSeek turns it into a title and a priority. Everything is stored in a local SQLite file. This repository holds the `t` command line tool, with its terminal UI, and a Cinnamon desklet that shows the list on the desktop. The interface text is in Portuguese.

| Part | What it is |
| --- | --- |
| [`src`](src) | The `t` binary: commands, the ratatui TUI, the SQLite store, link parsing and enrichment, and the DeepSeek call. |
| [`desklet/pilha@vinicius`](desklet/pilha@vinicius) | Cinnamon desklet that runs `t` to list, complete and open todos, and opens the TUI in a terminal. |

## Requirements

- Rust **1.85+** (edition 2024)
- A DeepSeek API key, optional: without it, or when the call fails, the todo is still saved, with the text minus the links as the title and the priority guessed from keywords
- `gh` signed in (`gh auth login`), optional, to read PR details with `gh pr view`
- A ClickUp personal token (`pk_...`), optional, to read task details from the ClickUp API
- Cinnamon, for the desklet

## Install

Install the `t` binary into `~/.cargo/bin` and set the keys:

```sh
cargo install --path .
t config
```

The config is `~/.config/pilha/config.toml`, written with mode 600. `DEEPSEEK_API_KEY` and `CLICKUP_TOKEN` override the file. The database is `~/.local/share/pilha/pilha.db`.

## Usage

Add todos, with or without the AI step:

```sh
t add "review https://github.com/org/repo/pull/123 urgente"
t add "https://app.clickup.com/t/86abc123 essa semana, sem pressa"
t add --no-ai "comprar café" -p low
```

Read and change them (`ls` and `show` take `--json` for scripts, `ls -a` includes done todos):

```sh
t
t ls
t show 4
t done 4 5
t undo 4
t open 4
t edit 4 -t "new title" -p high
t rm 4
```

`t` alone opens the TUI:

| Key | Action |
| --- | --- |
| `a` | New todo |
| `j` / `k` | Move the cursor |
| `x` / space / Enter | Complete or reopen |
| `u` | Reopen |
| `p` | Change priority |
| `J` / `K` | Move the todo in the list (crossing a group changes its priority) |
| `o` | Open links |
| `d` | Remove |
| `h` | Show or hide done todos |
| `q` | Quit |

## Desklet

Link the desklet into Cinnamon's folder, then add it with right click on the desktop, *Add desklets*, **Pilha**:

```sh
ln -sfn "$PWD/desklet/pilha@vinicius" ~/.local/share/cinnamon/desklets/pilha@vinicius
```

It refreshes when the database changes. `✓` completes a todo, the badges open its links and `+` opens the TUI in a terminal. The binary path, refresh interval, item count and width are in the desklet settings. After editing the desklet's JavaScript, remove and add it again (or `Alt+F2`, `r`); errors show up in Looking Glass (`Alt+F2`, `lg`).

## Tests

```sh
cargo test
```

`cargo test` covers the parsing of the AI response, the fallback title and priority, link extraction and classification, the SQLite store (insert, ordering, hiding done todos, moving within and across priorities, delete) and the TUI rendering and keys.
