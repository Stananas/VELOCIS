# VELOCIS

**Studio vidéo.** Montage non linéaire, composition multi-pistes et export
accéléré par le GPU. Moteur en Rust, interface web locale.

```bash
cargo build --release
./target/release/velocisd --open     # ouvre l'interface sur http://127.0.0.1:8730
```

Pour découvrir l'application avec un projet déjà monté :

```bash
bash tools/demo.sh
```

## Fonctionnalités

| | |
|---|---|
| **Import** | vidéo, audio, image. Analyse des pistes, vignette, forme d'onde. Dépôt des fichiers n'importe où dans la fenêtre, ou import par chemin |
| **Timeline** | multi-pistes, déplacement, rognage par les poignées, découpe au curseur, duplication, suppression avec fermeture d'espace, aimantation aux bords et au curseur, zoom, indicateurs d'espace |
| **Composition** | position, échelle, rotation, miroir, recadrage, opacité, fondus d'entrée et de sortie, vitesse (hauteur de son préservée), ordre par piste |
| **Effets** | luminosité, contraste, saturation, température, teinte, gamma, flou, netteté, noir et blanc, sépia, inversion, vignette, incrustation couleur |
| **Texte** | piste dédiée, taille, gras et italique, couleur, fond, fondus |
| **Aperçu** | composition **temps réel** dans le navigateur : lecture fluide, déplacement du curseur, réglages visibles à l'instant |
| **Préparation** | à l'import, un proxy à keyframes rapprochées est préparé en arrière-plan (480–540p) : c'est ce qui rend le scrub instantané. L'export continue de lire le fichier d'origine |
| **Export** | 480p → 4K, MP4 (H.264/AAC) et WebM (VP9/Opus), quatre niveaux de qualité, progression et annulation |
| **Encodage** | accélération matérielle détectée par sonde réelle (VAAPI, NVENC), repli automatique sur le processeur |
| **Historique** | annuler et rétablir sur chaque modification, avec le libellé de l'action |
| **Persistance** | sauvegarde automatique après chaque changement, reprise au démarrage |

## Architecture

```text
crates/velocis-engine/   le moteur — aucune dépendance à l'interface
  model.rs               types du domaine (temps en microsecondes entières)
  ops.rs                 opérations de montage, pures et annulables
  render.rs              projet -> graphe de filtres ffmpeg (une *valeur*)
  export.rs              exécution, progression, choix de l'encodeur
  probe.rs               analyse des médias (ffprobe)
  thumbs.rs              vignettes et pics audio
  fonts.rs               résolution des polices via fontconfig

crates/velocisd/         le service local — JSON sur 127.0.0.1
  state.rs               projet ouvert, historique, tâches de rendu
  api.rs                 traduction HTTP -> moteur

ui/                      l'interface, sans framework ni bundler
  index.html             structure
  styles.css             jetons de design et mise en page
  app.js                 rendu et interactions

tools/                   outillage de développement, hors produit
  demo.sh                construit un projet de démonstration via l'API
  cdp.mjs                client CDP minimal (recharger, évaluer, cliquer, glisser)
```

Le détail des décisions et des pièges rencontrés est dans
[`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md).

## Configuration

| Variable | Rôle | Défaut |
|---|---|---|
| `VELOCIS_DATA_DIR` | projets, médias importés, rendus | `$XDG_DATA_HOME/velocis` |
| `VELOCIS_MEDIA_DIR` | dossier des médias et des rendus | `<données>/media` |
| `VELOCIS_UI_DIR` | dossier de l'interface | `ui/` du dépôt |
| `VELOCIS_PORT` | port d'écoute | `8730` |
| `VELOCIS_FFMPEG`, `VELOCIS_FFPROBE` | binaires externes | `ffmpeg`, `ffprobe` |
| `VELOCIS_FORCE_CPU_ENCODER` | désactive l'accélération matérielle | non défini |

## Prérequis

- Rust stable
- `ffmpeg` et `ffprobe` (avec `drawtext`, `xfade`, `chromakey`)
- `fontconfig` pour la résolution des polices (facultatif : sans lui, le nom de
  famille est passé directement à `drawtext`)
- un GPU compatible VAAPI ou NVENC pour l'encodage matériel (facultatif)

## Tests

```bash
cargo test                                             # 45 tests unitaires
cargo clippy --all-targets -- -D warnings
cargo run --release --example smoke -- <video1> <video2>
cargo run --release --example frame -- <projet.json> <sortie.png> <temps_s> [piste_à_masquer]
```

`smoke` fait tourner toute la chaîne sur de vrais fichiers. `frame` rend une image
unique et sait masquer une piste : c'est ainsi qu'une géométrie de composition se
vérifie au pixel près plutôt qu'à l'œil.

## Limites connues

- Pas de transitions enchaînées : le modèle les porte, le rendu ne fait que des
  fondus.
- L'aperçu compose en direct sur un proxy, l'export encode depuis l'original : deux moteurs, donc quelques effets sans équivalent canvas (netteté, incrustation couleur) ne se voient qu'au rendu final.
- Les sources non décodables par le navigateur (Matroska, HEVC) sont lues par segments de 15 s ; au-delà de 20 minutes, aucun proxy complet n'est préparé.
- Pas encore : détachement de l'audio, sous-titres, LUT, suppression
  d'arrière-plan, modes de fusion, cadres clés.
- Le service expose des chemins du système : il écoute sur la boucle locale et ne
  doit jamais être exposé au réseau.

## Feuille de route

1. Transitions : chevauchement explicite sur une piste, puis `xfade` dans le graphe.
2. Aperçu incrémental : ne re-rendre que la fenêtre modifiée, avec cache par empreinte.
3. Cadres clés : la transformation est déjà un point, il suffit d'en faire une piste temporelle.
4. Coquille de bureau native, pour se passer du navigateur.

## Licence

MIT.
