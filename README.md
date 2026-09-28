# manga-sama

Lecteur de scans [Anime-Sama](https://anime-sama.to) dans le terminal.
Les images s'affichent avec le protocole graphique du terminal (kitty, sixel, iTerm2), en demi-blocs sinon.

> Client non officiel, non affilié à Anime-Sama. Le contenu appartient à ses ayants droit.

## Installation

```sh
cargo install --path .
```

## Utilisation

```sh
manga-sama one piece   # recherche directe
manga-sama             # demande quoi chercher
```

Choisis le titre (tape pour filtrer), puis la version de scans s'il y en a plusieurs.
Le lecteur s'ouvre sur la liste des chapitres. Entrée vide ou Échap pour quitter.

## Touches

| Touche                                | Action                                    |
|---------------------------------------|-------------------------------------------|
| `j` `↓` molette bas `Espace`          | page suivante (puis chapitre suivant)     |
| `k` `↑` molette haut                  | page précédente (puis chapitre précédent) |
| `l` `→`                               | chapitre suivant                          |
| `h` `←`                               | chapitre précédent                        |
| `F1`                                  | liste des chapitres (`j`/`k`, Entrée, `F1`/Échap pour fermer) |
| `F2`                                  | masquer/afficher la barre d'état          |
| `q` `Échap`                           | quitter                                   |
