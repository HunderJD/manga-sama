# manga-sama

Lis les scans d'[Anime-Sama](https://anime-sama.to) dans ton terminal.
Tu cherches un titre, tu choisis un chapitre, et tu fais défiler les pages comme sur le site.

🇬🇧 [English version](README.md)

> [!WARNING]
> **Ce n'est pas un client officiel d'Anime-Sama.**
> Je n'ai aucun lien avec Anime-Sama, et les scans appartiennent à leurs ayants droit.
>
> Si tu veux prévenir l'équipe d'Anime-Sama que ce projet existe, vas-y, ça ne me dérange pas.
> Et si tu veux bosser dessus avec moi, tu es le bienvenu.

## Ce qu'il te faut

- [kitty](https://sw.kovidgoyal.net/kitty/). Les pages sont affichées avec son protocole graphique, voir [Terminaux](#terminaux).
- Rust, pour compiler.

## Terminaux

manga-sama affiche les pages avec le [protocole graphique de kitty](https://sw.kovidgoyal.net/kitty/graphics-protocol/). Seul kitty a été testé.

![kitty: supporté](https://img.shields.io/badge/kitty-support%C3%A9-brightgreen)

![Ghostty: pas testé](https://img.shields.io/badge/Ghostty-pas%20test%C3%A9-lightgrey) ![WezTerm: pas testé](https://img.shields.io/badge/WezTerm-pas%20test%C3%A9-lightgrey) ![Konsole: pas testé](https://img.shields.io/badge/Konsole-pas%20test%C3%A9-lightgrey)

![Alacritty: non supporté](https://img.shields.io/badge/Alacritty-non%20support%C3%A9-red) ![foot: non supporté](https://img.shields.io/badge/foot-non%20support%C3%A9-red) ![GNOME Terminal: non supporté](https://img.shields.io/badge/GNOME%20Terminal-non%20support%C3%A9-red) ![xterm: non supporté](https://img.shields.io/badge/xterm-non%20support%C3%A9-red)

![tmux: non supporté](https://img.shields.io/badge/tmux-non%20support%C3%A9-red) ![SSH: non supporté](https://img.shields.io/badge/SSH-non%20support%C3%A9-red)

Ghostty implémente le protocole, WezTerm et Konsole seulement en partie. À travers tmux ou SSH, les images n'arrivent jamais au terminal.
Tu en as essayé un autre ? Ouvre une issue pour nous dire ce que ça donne.

## Installation

```sh
git clone https://github.com/HunderJD/manga-sama
cd manga-sama
cargo install --path .
```

## Utilisation

```sh
manga-sama
```

Quatre écrans, l'un après l'autre : **recherche → version → chapitres → lecture**.
Tape au moins deux lettres, la recherche part dès que tu t'arrêtes de taper.
L'écran des versions n'apparaît que si l'œuvre en a plusieurs (par exemple couleur et noir et blanc).
`Backspace` ou `Échap` revient à l'écran précédent. Sur chaque écran, `?` affiche ses touches, `Ctrl+L` change la langue (anglais par défaut) et `Ctrl+C` quitte.

### Recherche

| Touche    |                        |
|-----------|------------------------|
| flèches   | choisir                |
| `Entrée`  | ouvrir                 |
| `Ctrl+T`  | tuiles de couvertures  |
| `Échap`   | quitter                |

### Version et chapitres

| Touche               |                                              |
|----------------------|----------------------------------------------|
| `↑` `↓`, `j` `k`     | choisir                                      |
| `0`-`9`              | chapitres seulement : taper un numéro        |
| `Entrée`             | ouvrir                                       |
| `Backspace` `Échap`  | retour                                       |
| `q`                  | quitter                                      |

### Lecteur

| Touche                        |                                  |
|-------------------------------|----------------------------------|
| `j` `k`, molette              | défiler                          |
| `d` `u`                       | demi-écran                       |
| `h` `l`, clic gauche / droit  | chapitre précédent / suivant     |
| `F1`, `Backspace`, `Échap`    | retour à la liste des chapitres  |
| `Shift+H`                     | masquer / afficher la barre      |
| `q`                           | quitter                          |

## Logs

Chaque requête HTTP est écrite dans le log, mais seulement si la sortie d'erreur est redirigée :

```sh
manga-sama 2> /tmp/manga-sama.log
tail -f /tmp/manga-sama.log   # dans un autre terminal
```

## Contribuer

Les issues et les pull requests sont les bienvenues.

### À propos de l'IA

Coder avec une IA, pas de souci, je le fais aussi, mais tu dois pouvoir expliquer chaque ligne que tu envoies.
Lis la [politique IA](AI-POLICY.md) (en anglais) avant d'ouvrir une pull request.
