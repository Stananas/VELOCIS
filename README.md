# VELOCIS

Éditeur vidéo nouvelle génération — natif, GPU-accéléré, écrit en Rust.

## Lancer

```bash
cargo run
```

Première compilation : 5-10 min. Ensuite : 1-3 sec.

## Raccourcis

| Touche | Action |
|---|---|
| `Ctrl+K` | Palette de commandes |
| `Ctrl+Shift+H` | Retour à l'accueil |
| `Ctrl+N` | Nouveau projet |
| `Ctrl+S` | Enregistrer |

## Fonctionnalités

- **Accueil** : grille de projets récents, créer un projet
- **Éditeur** : panneau médias (import + clic → ajout à la timeline), lecteur/preview, panneau effets, timeline
- **Timeline** : 3 pistes (V1 vidéo, A1 audio, T1 texte), clips visuels, tête de lecture, sélection
- **Médias** : import de médias exemples, cliquez pour ajouter à la timeline
- **Police** : Lexend (design system moderne)
- **Palette** : fond `#08090A`, orange `#FF5C00`, bleu `#2F5FEE`, texte `#F3F4F6`

## Structure

```
src/
├── main.rs          ← Entrée, raccourcis clavier
├── app.rs           ← État global, routage Accueil ↔ Éditeur
├── core/
│   ├── project.rs   ← Modèles : Projet, Media, Piste, Clip
│   └── state.rs     ← AppView, Settings, actions
└── ui/
    ├── theme.rs     ← Design system (couleurs)
    ├── home.rs      ← Écran d'accueil
    ├── layout.rs    ← Éditeur (top bar, médias, lecteur, timeline)
    ├── command.rs   ← Palette Ctrl+K
    ├── panels/
    │   └── media.rs ← Panneau médias
    └── timeline/
        └── mod.rs   ← Timeline + clips + tête de lecture
```

## Stack

- **UI** — GPUI (rendu GPU natif)
- **Graphisme** — Wgpu (compute shaders)
- **Média** — GStreamer (décodage/encodage)
- **Police** — Lexend
