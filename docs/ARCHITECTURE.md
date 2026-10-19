# Architecture

Ce document explique **pourquoi** le code est fait ainsi. Les décisions qui ont
un coût de lecture doivent avoir une raison écrite, sinon la première refonte les
annule par accident.

## Vue d'ensemble

```text
   interface (ui/)                    service (velocisd)              moteur (velocis-engine)
   ─────────────────                  ──────────────────              ───────────────────────
   rendu, interactions   ── JSON ──▶  état du projet       ──────▶    modèle, opérations
   aucune écriture       ◀─ instantané │ historique                    graphe de rendu
   directe du projet                  │ tâches de rendu    ◀──────    export, sondes
                                      └── fichiers médias
```

Le service est un processus local qui parle JSON sur la boucle locale et sert
l'interface depuis la même origine : pas de CORS, pas de bundler, pas de chaîne
de compilation côté interface.

## Les cinq décisions structurantes

### 1. Le temps est un entier de microsecondes

Les secondes flottantes accumulent de l'erreur à chaque rognage, découpe et
déplacement. Deux chemins de calcul qui devraient donner le même instant finissent
par différer, et l'égalité après un annuler/rétablir devient approximative.

L'entier est aussi sans danger dans le format d'échange : `2^53` microsecondes
représentent environ 285 ans.

### 2. Le rendu est une valeur, pas une suite d'effets de bord

`render::build_plan` transforme un projet en `RenderPlan` : la liste des entrées,
le graphe de filtres, les fichiers texte à écrire, la durée, les avertissements.
Rien n'est exécuté. Trois conséquences :

- l'aperçu et l'export partagent **le même plan**, donc ce qu'on voit est ce
  qu'on obtient ;
- le graphe s'inspecte, se journalise et se teste sans lancer ffmpeg ;
- un fichier de projet suffit à reproduire un rendu.

C'est aussi ce qui rend `examples/frame` possible : rendre une image unique d'un
projet enregistré, avec la possibilité de masquer une piste, pour vérifier une
géométrie de composition au pixel près.

### 3. L'interface n'écrit jamais le projet

Elle demande, le service applique, et l'interface se redessine à partir de la
réponse. Un seul écrivain, donc un historique honnête : annuler restaure
exactement l'état précédent.

L'historique est **par instantanés** plutôt que par commandes inverses. Un projet
pèse quelques kilo-octets de JSON, le coût mémoire est négligeable, et cela
supprime une catégorie entière de bugs : un inverse subtilement faux produit un
document corrompu dont l'utilisateur ne peut pas se sortir. La pile est bornée.

### 4. Les invariants sont tenus par le moteur

- **Un clip ne chevauche jamais un autre.** Déposer un clip sur du temps occupé
  pousse les autres vers la droite, en blocs entiers : rien n'est supprimé et le
  résultat est une fonction pure de l'entrée, donc annulable sans surprise.
- **Le texte vit sur sa propre piste**, au-dessus des médias (l'ordre des pistes
  est l'ordre de composition).
- **Une piste verrouillée refuse les modifications**, au niveau du moteur, pas de
  l'interface.
- **Un rognage ne peut pas sortir du média** : la durée demandée est bornée par
  ce que la source contient réellement.

### 5. Le moteur pilote ffmpeg, il ne s'y lie pas

`velocis-engine` ne se lie pas à `libav*` : il lance `ffmpeg` et `ffprobe` en
sous-processus. Cela garde la compilation simple, laisse le système choisir
l'accélération matérielle par les mécanismes normaux de ffmpeg, et rend l'étape de
rendu reproductible à partir du seul plan.

Le prix est un format d'échange à maîtriser : c'est l'objet de la section
suivante.

## Les pièges du graphe de filtres

Chacun de ces points a coûté du temps et est désormais verrouillé par un test.

**ffmpeg refuse un `-filter_complex` multiligne.** Il répond
« Trailing garbage after a filter », message qui ne dit rien de la cause. Les
chaînes sont donc séparées par `;` et jamais par un retour à la ligne.

**L'échappement du graphe est incohérent.** `\:` fonctionne, mais `%` exige
`\\%` parce que le parseur du graphe consomme un niveau d'échappement avant que
`drawtext` n'applique le sien. Plutôt que d'empiler les règles, le texte
utilisateur ne passe jamais dans le graphe : il est écrit dans un fichier
référencé par `textfile=`. Seul `%` reste à échapper, au niveau de `drawtext`.

**`fade` refuse un temps de départ négatif.** Les fondus sont donc exprimés dans
le temps local du clip, où ils sont toujours positifs, puis le flux est ramené
dans le temps de sortie. Corollaire important : **un fondu appartient au clip, pas
à la fenêtre de rendu.** Rendre deux secondes au milieu d'un clip ne doit pas
inventer des fondus aux bords de la fenêtre, ni changer leur vitesse.

**`setpts` après `tpad` décale le clip.** `tpad` matérialise des images
transparentes avant le clip pour que son flux d'incrustation existe sur toute la
durée ; réinitialiser les horodatages ensuite fait glisser le clip dans le temps
et la porte `enable=between(...)` montre les mauvaises images.

**Le choix de l'encodeur et le graphe sont couplés.** Un encodage VAAPI exige un
`hwupload` **dans** le graphe, parce que ffmpeg interdit de combiner `-vf` avec un
flux issu de `-filter_complex`. `ExportSettings::new` porte donc le couple
(réglages, encodeur), et `run` refuse de démarrer s'ils sont incohérents — plutôt
que de laisser ffmpeg produire une erreur incompréhensible.

**La sonde d'encodeur doit tester une taille réaliste.** L'encodeur H.264 de VAAPI
refuse en dessous de 128×128 ; sonder en 128×64 désactivait silencieusement un GPU
parfaitement fonctionnel. La sonde encode réellement une image en 320×240.

**L'extension du fichier de sortie n'est pas une fin de chaîne.** ffmpeg choisit
son muxer d'après l'extension et refuse de deviner. Comparer les suffixes ne suffit
pas : `test-webm` se termine par `webm` sans avoir d'extension. La comparaison
porte sur une vraie extension, et un test couvre le cas.

## Côté interface

**Ne jamais reconstruire la timeline pendant un glisser.** Remplacer le nœud
qu'on déplace le détache du document, et les événements de pointeur cessent de
remonter jusqu'au point de délégation : le geste se coupe au premier mouvement. La
sélection se reflète donc sur place, et le rendu complet attend la fin du geste.

**La capture de pointeur est une optimisation, pas une exigence.** Un pointeur
déjà relâché fait lever `setPointerCapture` : l'échec doit être absorbé, sinon le
glisser ne démarre pas.

**`hidden` doit gagner sur les règles de mise en page.** Un état vide en
`display: grid` reste visible par-dessus la vidéo qu'il est censé laisser place.
La feuille de style commence donc par une règle `[hidden]` explicite.

**Une grille sans `grid-template-rows` fait grandir sa ligne avec le contenu.**
La ligne centrale a dû être fixée à `minmax(0, 1fr)` : sans cela, la scène
poussait la barre de lecture hors de la fenêtre.

**Les jetons de design suivent une échelle, pas des valeurs au hasard.** Rayons
2/4/6/8/12/16, rythme d'espacement de 4, corps de texte de 10 à 24, durées
d'animation de 50 à 500 ms. C'est ce qui donne à une interface dense une
impression de calme, et cela se tient sans avoir à redécider chaque valeur.

## Pistes écartées

**Tout faire dans le navigateur** (rendu WebGL, décodage WebCodecs) : séduisant
pour la portabilité, mais l'export et l'accès aux codecs matériels y sont bien
plus fragiles, et l'application ne peut alors plus manipuler de fichiers locaux
sans détour.

**Un toolkit d'interface natif en Rust.** Le rendu et la densité d'information
d'un monteur demandent une mise en page fine, des animations et une typographie
soignée : le web y arrive mieux et plus vite. Rust apporte sa valeur dans le
moteur, pas dans les pixels.

**Un moteur lié à `libav*`.** Plus rapide sur le papier, mais il faut alors
reconstruire la sélection de matériel, la négociation de formats et le muxage que
ffmpeg fait déjà correctement en ligne de commande.
