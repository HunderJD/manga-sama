# manga-sama

Read [Anime-Sama](https://anime-sama.to) and [Mangas-Origines](https://mangas-origines.fr) scans in your terminal.
Search a title, pick a chapter, and scroll through the pages like on the website.

🇫🇷 [Version française](README.fr.md)

> [!WARNING]
> **This is not an official Anime-Sama client.**
> I'm not affiliated with Anime-Sama in any way, and the scans belong to their rights holders.
>
> If you want to let the Anime-Sama team know about this project, go ahead, I don't mind.
> And if you want to work on it with me, you're welcome.

## What you need

- [kitty](https://sw.kovidgoyal.net/kitty/). Pages are drawn with its graphics protocol, see [Terminals](#terminals).
- Rust, to build it.

## Terminals

manga-sama draws pages with the [kitty graphics protocol](https://sw.kovidgoyal.net/kitty/graphics-protocol/).
Other terminals that implement it may work, but only kitty has been tested.

| Terminal                              | Status                                    |
|---------------------------------------|-------------------------------------------|
| kitty                                 | ✅ supported                              |
| Ghostty                               | ❔ not tested, implements the protocol     |
| WezTerm, Konsole                      | ❔ not tested, partial support             |
| Alacritty, foot, GNOME Terminal, xterm | ❌ no kitty graphics protocol             |
| tmux, over SSH                        | ❌ images don't get through               |

Tried another one? Open an issue and tell us how it went.

## Install

```sh
git clone https://github.com/HunderJD/manga-sama
cd manga-sama
cargo install --path .
```

## Usage

```sh
manga-sama
```

Type at least two letters, the search starts as soon as you stop typing.
The reader opens on the chapter list.

### Search

| Key      |                  |
|----------|------------------|
| arrows   | move             |
| `Enter`  | open             |
| `Tab`    | source           |
| `Ctrl+T` | cover tiles      |
| `Ctrl+L` | language         |
| `?`      | help             |
| `Esc`    | quit             |

### Reader

| Key                   |                                            |
|-----------------------|--------------------------------------------|
| `j` `k`, mouse wheel  | scroll                                     |
| `d` `u`               | half a screen                              |
| `h` `l`, left / right click | previous / next chapter              |
| `F1`                  | chapter list, type a number to jump to it  |
| `F2`                  | hide the status bar                        |
| `Ctrl+L`              | language                                   |
| `?`                   | help                                       |
| `Backspace` `Esc`     | back to the search                         |
| `q`                   | quit                                       |

## Logs

Every HTTP request is logged, but only when stderr is redirected:

```sh
manga-sama 2> /tmp/manga-sama.log
tail -f /tmp/manga-sama.log   # in another kitty window
```

## Contributing

Issues and pull requests are welcome.

### About AI

Writing code with AI is fine, I do it too, but you should be able to explain every line you send.
Read the [AI policy](AI-POLICY.md) before opening a pull request.

## License

[MIT](LICENSE)
