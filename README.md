# manga-sama

Read [Anime-Sama](https://anime-sama.to) scans in your terminal.
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

manga-sama draws pages with the [kitty graphics protocol](https://sw.kovidgoyal.net/kitty/graphics-protocol/). Only kitty has been tested.

![kitty: supported](https://img.shields.io/badge/kitty-supported-brightgreen)

![Ghostty: not tested](https://img.shields.io/badge/Ghostty-not%20tested-lightgrey) ![WezTerm: not tested](https://img.shields.io/badge/WezTerm-not%20tested-lightgrey) ![Konsole: not tested](https://img.shields.io/badge/Konsole-not%20tested-lightgrey)

![Alacritty: unsupported](https://img.shields.io/badge/Alacritty-unsupported-red) ![foot: unsupported](https://img.shields.io/badge/foot-unsupported-red) ![GNOME Terminal: unsupported](https://img.shields.io/badge/GNOME%20Terminal-unsupported-red) ![xterm: unsupported](https://img.shields.io/badge/xterm-unsupported-red)

![tmux: unsupported](https://img.shields.io/badge/tmux-unsupported-red) ![SSH: unsupported](https://img.shields.io/badge/SSH-unsupported-red)

Ghostty implements the protocol, WezTerm and Konsole only partly. Through tmux or SSH the images never reach the terminal.
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
| `Shift+H`             | hide / show the status bar                 |
| `Ctrl+L`              | language                                   |
| `?`                   | help                                       |
| `Backspace`           | back to the search                         |
| `Esc`                 | close the popup, otherwise back to the search |
| `q`                   | quit                                       |

## Logs

Every HTTP request is logged, but only when stderr is redirected:

```sh
manga-sama 2> /tmp/manga-sama.log
tail -f /tmp/manga-sama.log   # in another kitty window
```

## Contributing

Issues and pull requests are welcome.

### Code map

A file ending in `_loader.rs` fetches in the background and draws nothing.

```
src/
├── main.rs                entry point: sets up the terminal, then goes back and forth between search and reading
├── i18n.rs                the app's texts in English and French, from locales/*.json built into the binary
├── kitty.rs               draws images with the kitty graphics protocol, reads the terminal size; used by both screens
├── search/
│   ├── mod.rs             the search screen: what you see and the keys (live search, list or tiles, version picker)
│   ├── results_loader.rs  background thread that runs the searches and returns their results
│   ├── tiles.rs           results as a grid of covers (Ctrl+T): layout and drawing
│   └── cover_loader.rs    background thread that downloads and decodes the covers, with their cache
├── reader/
│   ├── mod.rs             the reading screen: what you see, keys, scrolling, asking the page loader for pages
│   ├── page_loader.rs     background threads that download and decode the pages, with their cache
│   └── scroll.rs          scrolling math as pure functions: visible pages, what to load, easing
└── sources/
    ├── mod.rs             where manga come from: one module per website
    └── anime_sama.rs      talks to Anime-Sama: search, scan versions, chapter lists, page and cover images
```

### About AI

Writing code with AI is fine, I do it too, but you should be able to explain every line you send.
Read the [AI policy](AI-POLICY.md) before opening a pull request.
