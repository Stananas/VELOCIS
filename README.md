# VELOCIS

Éditeur vidéo nouvelle génération — Web/Desktop, GPU-accéléré.

## Stack

- **UI** — React + TypeScript + Vite
- **Desktop** — Electron
- **Rendu** — Rust → WebAssembly (WebGL2)
- **Décodage** — Navigateur natif (MSE / `<video>`)

## Lancer (dev)

```bash
npm run dev
```

## Builder le module Wasm

```bash
npm run build:wasm
```

## Structure

```
├── src/                ← Code React (UI, Timeline, état)
│   ├── App.tsx
│   ├── components/
│   │   ├── Preview.tsx   ← Canvas WebGL2 piloté par Wasm
│   │   ├── Timeline.tsx   ← Timeline de clips
│   │   └── MediaPanel.tsx ← Panneau médias
│   ├── hooks/
│   │   └── usePreviewRenderer.ts
│   └── lib/
│       ├── wasm-bridge.ts    ← Interface Wasm ↔ React
│       └── wasm/             ← Module Wasm compilé (généré)
├── src-wasm/           ← Code source Rust Wasm
│   ├── Cargo.toml
│   └── src/
│       └── lib.rs      ← Renderer WebGL2 (wasm-bindgen)
└── electron/           ← Wrapper Electron
    └── main.ts
