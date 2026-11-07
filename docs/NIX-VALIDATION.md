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
Ces premières mesures locales précèdent la publication de la branche et les
vérifications distantes ci-dessous.

## Complément : cache OVH et GitHub, 7 novembre 2026

Le bucket `yummy-blagg` a fait l'objet d'essais réels avec les identités Infisical
mainteneur et lecteur. Le témoin sccache compile une bibliothèque Rust avec le
compilateur Nix : un miss et une écriture S3 à froid, puis suppression de la
sortie locale et nouveau daemon avec l'identité lecteur. La seconde compilation
obtient un hit Rust, zéro compilation, zéro écriture et zéro erreur de cache.
Les SHA-256 des deux `.rlib` sont identiques. Ce témoin valide le protocole, pas
un gain de performance sur une compilation complète du jeu.

Les sorties Crane `game` et `game-deps` de la validation locale ont été signées
avec la clé privée créée dans Infisical infrastructure, puis publiées par
`tools/nix-cache.py push`. Un store Nix indépendant et initialement vide a
restauré ces sorties avec l'identité lecteur. La vérification du contenu et
d'au moins une signature de confiance a réussi pour 932 chemins. Le SHA-256
du binaire restauré est identique ; l'archive Crane de dépendances conserve
son `narHash`, ses références et sa taille NAR de 659 764 128 octets.
Les fichiers de rapport sont `sccache-ovh-*.json`, `nix-ovh-push.json` et
`nix-ovh-reader-*.json` dans les traces locales ignorées.

La politique lecteur OVH `944578` a été étendue, avec accord explicite, aux
seuls GET des objets Nix. La même politique est suivie dans
`infra/src/cache_access.rs`, et son état Pulumi a été synchronisé par une
[actualisation ciblée](https://app.pulumi.com/Pierre-ZACHARY/bevy-parkour2-infra/dev/updates/22).
La clé publique est suivie dans `nix/cache-public-key` ; aucun secret n'est ajouté
au dépôt. Une modification de politique OVH a présenté un délai de propagation,
observé par des refus temporaires avant réussite des lectures réelles.

L'[exécution GitHub f1b177c](https://github.com/Pierre-ZACHARY/bevy-parkour2/actions/runs/42394275535)
a validé le lint, les dérivations release Crane Linux x64/ARM64, le packaging
Linux/WebGPU/Windows/macOS et les tests Linux ARM64/macOS. La couverture x64 a
révélé l'option `--exclude` invalide dans `llvm-cov report`, puis un manque de
disque pendant Noodle. Les corrections retirent les options de collection de
la phase rapport et autorisent la CI à supprimer chaque build de couverture
après ses rapports, sous verrou. Les SDK inutilisés des runners Linux sont
également retirés. La validation GitHub de ces corrections reste nécessaire.

Le support `aarch64-darwin`, ses outils et ses deux shells passent l'évaluation
Nix. La compilation native macOS avec Nix et les CLI Dioxus/Jackdaw sont vérifiées
par le job macOS modifié ; l'évaluation depuis Linux ne prouve pas leur exécution.

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
