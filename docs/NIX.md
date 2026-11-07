# Travailler avec Nix et Crane

La flake fournit Linux x86_64 et ARM64, avec le même environnement pour les
collaborateurs et les runners GitHub Linux. Windows conserve son installation
native ; utiliser une distribution WSL2 pour Nix. macOS n'est pas exposé par
cette première flake. Les runners macOS et Windows conservent leur setup actuel.
Le bundle officiel Jackdaw/SDK n'existe pas pour Linux ARM64 à cette version :
l'éditeur est fourni sur x86_64 ; ARM64 dispose du jeu, de Dioxus et des outils CI.

## Première utilisation

Installer Nix avec les fonctions `nix-command` et `flakes` activées, puis depuis
la racine du clone :

```sh
nix develop
```

Sur une modification non encore commitée, Nix doit voir les nouveaux fichiers
dans Git : `git add -N flake.nix flake.lock nix tools/xtask/src/toolchain.rs`
permet de les exposer sans en indexer le contenu. Sous WSL, utiliser un clone
Git créé depuis Linux ; un worktree Windows avec un `.git` pointant vers `C:/...`
ne se comporte pas comme un clone Linux.

Le premier démarrage peut être long : Dioxus CLI **0.7.2** est compilé puis
conservé dans le store. Sur x86_64, le bundle officiel Jackdaw/jd/SDK
**0.20.1-rc.1** est téléchargé avec vérification SHA-256 et adapté à NixOS,
avec ses extensions Physics/Navmesh. Le shell
contient Rust **1.97.1**, rustfmt, Clippy, rust-analyzer, rust-src, LLVM coverage,
la cible wasm32, les compilateurs C/C++, Clang **22** pour le WebGPU, les
bibliothèques Bevy Linux, Git/LFS, PowerShell, Node et Java. Nextest, llvm-cov,
wasm-bindgen, wasm-opt et sccache reprennent les versions et SHA-256 du projet.
La cible wasm utilise Clang/llvm-ar directement : le wrapper natif Nix ajoute
des options de durcissement Linux que le backend wasm ne prend pas en charge.
Les exécutables ELF téléchargés sont adaptés à NixOS.

Le bootstrap sccache est automatique. Il utilise le compilateur et le binaire
sccache Nix, sans installer un autre Rust via rustup. Un `RUSTUP_TOOLCHAIN`
explicitement défini conserve la priorité pour les SDK externes.
Le SDK Jackdaw utilise Rust **1.99.0**, également fourni par Nix. Un dispatch
sélectionne ce compilateur quand le SDK le demande, et garde **1.97.1** pour le
jeu. La façade sccache respecte ce choix.

Les assets privés restent soumis à [l'accès au dépôt d'assets](GITHUB.md).
Dans le shell, après obtention de cet accès :

```sh
git submodule update --init private-assets
git -C private-assets lfs pull --include='assets/**'
pwsh -NoProfile -File tools/setup_assets.ps1
```

La flake du jeu contient le binaire, pas les assets licenciés. Pour accéder au
cache sccache OVH, installer le CLI Infisical et effectuer le login décrit dans
[REMOTE-CACHE.md](REMOTE-CACHE.md). Sans login, sccache fonctionne localement.

## Jeu, éditeur et hotpatch

Depuis la racine du projet, dans `nix develop` :

```sh
# Jeu en développement
cargo run

# Jeu release sans éditeur ni overlay de développement
cargo run --release --no-default-features

# Éditeur Jackdaw : JACKDAW_OPEN_PROJECT est défini par le shell
jackdaw

# Dioxus : le lanceur existant conserve le cache malgré son wrapper hotpatch
pwsh -NoProfile -File tools/cached_command.ps1 dx serve --hot-patch --features hotpatch
```

Pour les extensions Flow Graph et du projet, enregistrer le SDK Nix une fois,
puis utiliser le script existant décrit dans le README :

```sh
jd editor locate "$PARKOUR_NIX_EDITOR_DIR"
pwsh -NoProfile -File tools/editor_extension.ps1
```

`BEVY_ASSET_ROOT` désigne le répertoire courant au moment de l'entrée dans le
shell : les assets sont lus et surveillés dans le clone. Entrer dans le shell
depuis la racine. Le shell `ci` omet les gros CLI graphiques :

```sh
nix develop .#ci -c cargo xtask doctor --targets linux-x64,webgpu
# Sur ARM64, utiliser linux-arm64 à la place de linux-x64.
```

Un affichage X11/Wayland et un pilote Vulkan utilisable restent nécessaires.
Sur NixOS, activer les pilotes graphiques dans la configuration système. Sur
une distribution non NixOS, un adaptateur tel que nixGL peut être nécessaire
pour que les bibliothèques Nix accèdent au pilote de l'hôte. WSL nécessite WSLg
et un pilote compatible ; une compilation réussie ne prouve pas le rendu GPU.

Pour un smoke test sans GPU compatible (par exemple si le pilote Vulkan WSL
est trop limité), le même nixpkgs fournit Lavapipe. Ce rendu CPU ne valide ni
les performances ni le pilote matériel :

```sh
nix build .#mesa --out-link result-mesa
export VK_DRIVER_FILES="$(find -L result-mesa/share/vulkan/icd.d -name 'lvp_icd*.json' -print -quit)"
export VK_LOADER_LAYERS_DISABLE='~implicit~'
nix develop
```

## Tests, couverture et packages

Les commandes xtask et leurs rapports restent identiques :

```sh
nix develop .#ci -c cargo xtask lint
nix develop .#ci -c cargo xtask test
nix develop .#ci -c cargo xtask coverage
nix develop .#ci -c cargo xtask build --targets linux-x64,webgpu --profile release
nix develop .#ci -c cargo xtask build --targets linux-x64 --asset-quality optimized
```

Les runners ARM64 utilisent `--targets linux-arm64`. Pour un checkout public
sans assets, utiliser `--suites xtask,graph,asset-export,noodle` pour les tests et
la couverture. Les rapports se trouvent dans `artifacts/<run>/` : JUnit,
HTML/LCOV/JSON pour la couverture, manifestes et ZIP vérifiés pour le packaging.
`--offline` exige aussi que les sources Cargo soient déjà dans le cache local ;
la flake fournit les outils, pas un cache Cargo de travail prérempli.

Les exécutables natifs produits avec les bibliothèques Nix peuvent référencer
`/nix/store`. Le ZIP xtask seul ne transporte pas cette closure. Pour distribuer
hors d'une machine Nix, prévoir un bundle autonome et le tester sur une machine
sans Nix ; ne pas présenter ce ZIP comme un binaire Linux universel.

## Builds purs et cache Crane

```sh
# Compiler la distribution locale en release dans le sandbox Nix
nix build .#game
BEVY_ASSET_ROOT="$PWD" ./result/bin/bevy_parkour2
# Ou directement : nix run .#game

# Compiler seulement les dépendances et les conserver via un GC root
nix build .#game-deps --out-link result-deps

# Évaluer tous les outputs sans compiler, pour les deux architectures
nix flake check --all-systems --no-build

# Checks purs : release du jeu et tests unitaires de xtask
nix flake check
```

Le lanceur du package Nix place temporairement l'exécutable à côté d'un lien
vers les assets du clone, car le mode distribution cherche les assets à côté
de son exécutable. Il nettoie ce répertoire à la sortie. Aucun asset licencié
n'est copié dans le store ; le vrai ELF se trouve dans `result/libexec/`.

Crane sépare les dépendances compilées (`game-deps`) du jeu (`game`). Un
changement dans `src/` reconstruit le jeu en réutilisant les dépendances. Les
crates patchées de `vendor/` conservent leur vrai code dans cette étape, car
des crates du registre importent leur API. Modifier ces patches, les manifests,
le lockfile, la toolchain ou les options de compilation invalide le cache
correspondant. Refaire exactement le même `nix build` réutilise son résultat.

Les deux caches ont des rôles complémentaires :

| Cache | Ce qu'il réutilise | Utilisation |
| --- | --- | --- |
| Nix / Crane | Sources, outils, dépendances compilées et résultats de dérivations | `nix build`, environnement `nix develop` |
| sccache | Invocations de compilateur compatibles | Cargo, xtask, Jackdaw, Dioxus via le lanceur |

Les compilations Cargo exécutées dans le clone ne réutilisent pas automatiquement
le répertoire `target` privé de Crane. Les flags, profils et sources doivent
coïncider pour partager des objets ; le shell ne copie pas d'artifacts dans les
répertoires de couverture ou de distribution. Les builds purs n'ont pas besoin
des identifiants sccache et n'effectuent pas d'accès réseau pendant la compilation.

Le cache d'artifacts Cargo de GitHub est également conservé pour les commandes
xtask : remplacer ce cache par le seul store Nix ferait recompiler leurs
dépendances, puisque leurs répertoires `target` sont distincts de ceux de Crane.
Sa clé Linux inclut `flake.lock` pour suivre les bibliothèques et compilateurs Nix.

GitHub restaure et sauvegarde `/nix` avec `cache-nix-action`, par architecture,
job et empreinte des fichiers de dépendances. Les PR peuvent lire le cache de
leur branche de base ; seules les exécutions hors PR le sauvegardent. Le job
Linux de build compile aussi `game` et `game-deps` avec Crane. Un GC est tenté
au-delà de 8 Gio avant sauvegarde ; les GC roots restent prioritaires, et les
limites de stockage du service continuent de s'appliquer. Les commandes xtask
continuent à utiliser sccache et à produire les rapports habituels.

Les lancements manuels sur une branche nécessitent une autorisation de cette
branche dans l'environnement GitHub `build` pour récupérer les assets privés.
Infisical reste limité à `main` et aux tags `v*` : les autres branches utilisent
sccache local, le cache Cargo de GitHub et le store Nix. L'analyse Sonar avec ses
identifiants dédiés reste réservée à `main`.

## Cache binaire OVH partagé : option ultérieure

Le cache GitHub ne nécessite pas de nouveau bucket. Le bucket existant
`yummy-blagg`, à Paris, peut aussi héberger un cache binaire Nix. Il n'est pas
activé par cette flake : les lecteurs sccache existants n'ont accès qu'au
préfixe `bevy-parkour2/v1/`, tandis que Nix utilise notamment `nix-cache-info`,
`*.narinfo` et `nar/*`. Réutiliser les identifiants readers actuels échouerait.

Pour l'activer : provisionner des lecteurs limités aux objets Nix et un writer
réservé aux jobs de confiance, conserver une clé de signature dans Infisical,
et transmettre uniquement sa clé publique aux collaborateurs. Exemple de
publication avec un profil AWS dédié et une clé hors du dépôt :

```sh
nix build .#game .#game-deps
nix store sign --recursive --key-file "$NIX_CACHE_SIGNING_KEY_FILE" ./result ./result-1
nix copy --to 's3://yummy-blagg?region=eu-west-par&scheme=https&endpoint=s3.eu-west-par.io.cloud.ovh.net&profile=parkour-nix-writer' ./result ./result-1
```

Ajouter ensuite ce store aux `extra-substituters` et sa clé publique aux
`extra-trusted-public-keys` de Nix. Un daemon multi-utilisateur doit lui-même
pouvoir lire le profil AWS de lecture. Ne pas désactiver la vérification des
signatures. Aucun secret, aucun asset privé et aucune configuration Infisical
ne doivent entrer dans une dérivation publiée. Le chemin S3 et l'aller-retour
signé devront être validés avant d'activer ce cache dans les jobs.

## Maintenance

`flake.lock` verrouille nixpkgs, Crane et rust-overlay. Le Rust du projet reste
défini dans `rust-toolchain.toml`.

```sh
nix flake update
nix fmt -- flake.nix nix/*.nix
nix flake check --all-systems --no-build
```

Les fichiers `nix/cli-locks/*.lock` sont copiés sans modification des
archives crates.io dont les SHA-256 sont épinglés dans `nix/project.nix`. Ils
évitent de télécharger/compiler une dérivation pendant l'évaluation de la flake.
Lors d'une mise à jour CLI, mettre à jour ensemble version, SHA-256 et lockfile
upstream. Leurs dépendances sont vendorizées et vérifiées par Crane.

Références : [Crane](https://crane.dev/),
[buildDepsOnly](https://crane.dev/API.html#cranelibbuilddepsonly),
[cache Nix GitHub](https://github.com/nix-community/cache-nix-action),
[cache S3 Nix](https://nix.dev/manual/nix/latest/store/types/s3-binary-cache-store.html).
