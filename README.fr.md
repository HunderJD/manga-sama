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
| `Shift+H`             | masquer / afficher la barre                     |
| `Ctrl+L`              | langue                                          |
| `?`                   | aide                                            |
| `Backspace`           | retour à la recherche                           |
| `Échap`               | fermer la popup, sinon retour à la recherche    |
| `q`                   | quitter                                         |

## Logs

Chaque requête HTTP est écrite dans le log, mais seulement si la sortie d'erreur est redirigée :

```sh
manga-sama 2> /tmp/manga-sama.log
tail -f /tmp/manga-sama.log   # dans une autre fenêtre kitty
```

## Contribuer

Les issues et les pull requests sont les bienvenues.

### Carte du code

Un fichier qui finit par `_loader.rs` va chercher des données en fond et n'affiche rien.

```
src/
├── main.rs                point d'entrée : prépare le terminal, puis alterne entre recherche et lecture
├── i18n.rs                les textes de l'app en anglais et en français, depuis locales/*.json intégrés au binaire
├── kitty.rs               affiche les images avec le protocole graphique de kitty, lit la taille du terminal ; sert aux deux écrans
├── search/
│   ├── mod.rs             l'écran de recherche : l'affichage et les touches (recherche en direct, liste ou tuiles, versions)
│   ├── results_loader.rs  le thread de fond qui lance les recherches et renvoie les résultats
│   ├── tiles.rs           les résultats en grille de couvertures (Ctrl+T) : disposition et affichage
│   └── cover_loader.rs    le thread de fond qui télécharge et décode les couvertures, avec leur cache
├── reader/
│   ├── mod.rs             l'écran de lecture : affichage, touches, défilement, demandes de pages au chargeur
│   ├── page_loader.rs     les threads de fond qui téléchargent et décodent les pages, avec leur cache
│   └── scroll.rs          les calculs du défilement en fonctions pures : pages visibles, quoi charger, glisse
└── sources/
    ├── mod.rs             d'où viennent les mangas : un module par site
    └── anime_sama.rs      parle à Anime-Sama : recherche, versions, listes de chapitres, pages et couvertures
```

### À propos de l'IA

Coder avec une IA, pas de souci, je le fais aussi, mais tu dois pouvoir expliquer chaque ligne que tu envoies.
Lis la [politique IA](AI-POLICY.md) (en anglais) avant d'ouvrir une pull request.
