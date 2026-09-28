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

manga-sama affiche les pages avec le [protocole graphique de kitty](https://sw.kovidgoyal.net/kitty/graphics-protocol/).
D'autres terminaux qui l'implémentent peuvent marcher, mais seul kitty a été testé.

| Terminal                              | État                                        |
|---------------------------------------|---------------------------------------------|
| kitty                                 | ✅ supporté                                 |
| Ghostty                               | ❔ pas testé, implémente le protocole        |
| WezTerm, Konsole                      | ❔ pas testé, support partiel                |
| Alacritty, foot, GNOME Terminal, xterm | ❌ pas de protocole graphique kitty         |
| tmux, en SSH                          | ❌ les images ne passent pas                 |

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

Tape au moins deux lettres, la recherche part dès que tu t'arrêtes de taper.
Le lecteur s'ouvre sur la liste des chapitres.
L'app est en anglais par défaut, `Ctrl+L` pour passer en français.

### Recherche

| Touche   |                  |
|----------|------------------|
| flèches  | se déplacer      |
| `Entrée` | ouvrir           |
| `Ctrl+T` | tuiles           |
| `Ctrl+L` | langue           |
| `?`      | aide             |
| `Échap`  | quitter          |

### Lecteur

| Touche                |                                                 |
|-----------------------|-------------------------------------------------|
| `j` `k`, molette      | défiler                                         |
| `d` `u`               | demi-écran                                      |
| `h` `l`, clic gauche / droit | chapitre précédent / suivant             |
| `F1`                  | liste des chapitres, tape un numéro pour y aller |
| `Shift+H`             | masquer la barre                                |
| `Ctrl+L`              | langue                                          |
| `?`                   | aide                                            |
| `Backspace` `Échap`   | retour à la recherche                           |
| `q`                   | quitter                                         |

## Logs

Chaque requête HTTP est écrite dans le log, mais seulement si la sortie d'erreur est redirigée :

```sh
manga-sama 2> /tmp/manga-sama.log
tail -f /tmp/manga-sama.log   # dans une autre fenêtre kitty
```

## Contribuer

Les issues et les pull requests sont les bienvenues.

### À propos de l'IA

Coder avec une IA, pas de souci, je le fais aussi, mais tu dois pouvoir expliquer chaque ligne que tu envoies.
Lis la [politique IA](AI-POLICY.md) (en anglais) avant d'ouvrir une pull request.

## Licence

[MIT](LICENSE)
