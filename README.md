# manga-sama

Lecteur de scans [Anime-Sama](https://anime-sama.to) dans le terminal.
Les pages défilent comme sur le site, affichées avec le protocole graphique de [kitty](https://sw.kovidgoyal.net/kitty/) (requis).

> Client non officiel, non affilié à Anime-Sama. Le contenu appartient à ses ayants droit.

## Installation

```sh
cargo install --path .
```

## Utilisation

```sh
manga-sama
```

Tape au moins 2 caractères (la recherche part après 350 ms sans frappe), `↑` `↓` pour choisir, Entrée pour ouvrir.
Le lecteur s'ouvre sur la liste des chapitres. Échap revient en arrière.

## Touches du lecteur

`?` affiche l'aide.

| Touche          | Action                              |
|-----------------|-------------------------------------|
| `j` `k` molette | défiler                             |
| `d` `u`         | demi-écran                          |
| `h` `l`         | chapitre précédent / suivant        |
| `F1`            | liste des chapitres                 |
| `F2`            | masquer la barre                    |
| `⌫` `Échap`     | retour à la recherche               |
| `q`             | quitter                             |

## Logs

Une ligne par requête HTTP, seulement si la sortie d'erreur est redirigée :

```sh
manga-sama 2> /tmp/manga-sama.log   # et dans un autre panneau :
tail -f /tmp/manga-sama.log
```
