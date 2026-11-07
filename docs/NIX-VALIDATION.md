# Validation Nix / Crane

Validation locale du 7 novembre 2026, dans Ubuntu 24.04 sous WSL2, sur
`x86_64-unknown-linux-gnu`. Le checkout Windows est resté la source des changements ;
compilations et essais ont utilisé une copie Linux isolée. Les assets de runtime
correspondent au gitlink privé `3228b3bca3c5159017fea810d347926dbd593737`.

## Résultats observés

| Vérification | Résultat |
| --- | --- |
| Évaluation de la flake x86_64 et ARM64 | PASS : `nix flake check --all-systems --no-build` |
| Release native Crane | PASS : dépendances puis jeu, sans accès réseau pendant la compilation |
| Deuxième build Nix identique | PASS : 3,21 s sur l'état final, même output, aucune compilation |
| Invalidation du cache | PASS : une modification de `src/main.rs` change la dérivation du jeu et conserve celle des dépendances ; source restaurée |
| Tests purs Crane de xtask | PASS : 31 réussis, 5 ignorés |
| Tests du jeu via xtask | PASS : 64 réussis, 3 ignorés |
| Ensemble des suites xtask | PASS : 262 réussis, 8 ignorés, doctests inclus séparément |
| Couverture xtask | PASS : HTML, LCOV et LLVM JSON produits, 31 tests réussis |
| Packaging xtask Linux x64 | PASS : compilation release, export binaire des assets, inventaire/hashes et ZIP vérifiés |
| Packaging xtask WebGPU | PASS : compilation wasm, bindgen, wasm-opt, export des assets, manifeste et ZIP vérifiés |
| Démarrage du jeu Crane | Fenêtre WSLg créée, scène et textures Ninja chargées avec Lavapipe |
| Jackdaw officiel + SDK | Fenêtre créée, projet ouvert, extension Physics chargée, ABI Rust 1.99.0 reconnue |
| Dispatch Rust | PASS : jeu 1.97.1, SDK 1.99.0 ; mise à jour atomique des façades pendant une compilation Dioxus |
| Dioxus 0.7.2 | PASS : `dx serve --hot-patch --features hotpatch`, compilation puis fenêtre Linux créée ; événement hotpatch traité |
| Formats et lint | PASS : nixfmt, actionlint, fmt/Clippy xtask, 4 tests des façades de cache |

Le premier build Dioxus a duré 791,16 s. Un événement sur
`tools/compiler-cache.rs` a ensuite été traité en hotpatch en 6,46 s. Ce dernier
log ne constitue pas une preuve de changement de comportement du jeu.

Le bundle officiel Jackdaw remplace la crate Cargo seule : celle-ci ouvrait une
fenêtre mais n'incluait pas les extensions Physics ni le SDK nécessaires au projet.
Le bundle x86_64 vérifié conserve ses métadonnées Rust, fournit Physics/Navmesh
et utilise le compilateur ABI 1.99.0. La compilation des extensions propres au
projet n'a pas été exécutée ; leur procédure existante figure dans le guide.

Le ZIP Linux validé fait **224 186 302 octets**. Son exécutable fait
**195 672 936 octets**. Le run xtask a mesuré 804,89 s pour la compilation et
139,98 s pour l'export binaire des assets. Ces temps correspondent à cette
machine et à ce cache ; ils ne sont pas une promesse de performance CI.

Le ZIP WebGPU fait **159 611 542 octets**. Une vérification indépendante avec
Python `zipfile` a confirmé les CRC et les hashes du manifeste pour les **28**
fichiers Linux et les **34** fichiers WebGPU. Les **19** fichiers privés du
runtime sont restés byte-identiques à leur copie source.

## Limites de la preuve graphique

Le pilote D3D12 Vulkan de cet hôte WSL annonce Vulkan 1.1 et n'a pas fourni
d'adaptateur utilisable par le jeu. Lavapipe de Mesa 26.2.6, issu du nixpkgs
verrouillé, a permis de créer la fenêtre et de charger les assets. Ce test
utilise le CPU et ne valide ni le pilote matériel, ni les FPS, ni le son.

L'outil de capture Windows a échoué deux fois pour la fenêtre WSLg avec
`IGraphicsCaptureItemInterop.CreateForMonitor`, erreur `0x80070057`. Il n'y a
donc pas de preuve visuelle par capture d'écran. Les logs du démarrage corrigé
ne contiennent plus les erreurs de chemin d'assets dans `/nix/store`.

ARM64 a été évalué, sans compilation ni exécution sur une machine ARM64.
L'éditeur officiel 0.20.1-rc.1 n'est pas publié pour Linux ARM64 et n'est donc
pas exposé sur cette architecture. Le shell CI et Dioxus y sont exposés.
Les workflows GitHub ont été modifiés localement ; aucun push ou lancement
de workflow distant n'est inclus dans cette validation.

## Traces locales

Les logs, mesures JSON et scripts de validation sont conservés dans
`artifacts/nix-validation/` du worktree Windows. La copie Linux et les rapports
xtask complets sont dans
`/home/pzachary/.cache/parkour-nix-validation-20261107/` sous Ubuntu-24.04.
Ces chemins sont des artifacts locaux ignorés par Git, pas des fichiers requis
par la flake.

Le [guide d'utilisation](NIX.md) décrit les commandes et la distinction entre
store Nix, cache Cargo et sccache, ainsi que les limites de distribution des
binaires natifs liés à `/nix/store`.
