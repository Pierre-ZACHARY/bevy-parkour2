# Travailler avec Nix et Crane

La flake fournit Linux x86_64, Linux ARM64 et macOS Apple Silicon
(`aarch64-darwin`), avec le même environnement pour les collaborateurs et les
runners GitHub Linux/macOS. Windows conserve son installation native ; utiliser
une distribution WSL2 pour Nix.
Le bundle officiel Jackdaw/SDK n'existe pas pour Linux ARM64 à cette version :
l'éditeur est fourni sur Linux x86_64 et macOS ARM64. Linux ARM64 dispose du jeu,
de Dioxus et des outils CI.

## Parcours collaborateur macOS Apple Silicon (lecture seule)

Prérequis : Nix avec flakes, accès Viewer au projet Infisical build et accès au
sous-module d'assets privés. Aucun accès infrastructure ni droit d'écriture OVH
n'est nécessaire. Depuis la racine du clone :

```sh
# Fournit Infisical sans construire le gros shell interactif.
nix run .#infisical -- login --domain https://eu.infisical.com
export PROJECT_CACHE_ACCESS=read-only
nix run .#nix-cache -- pull --inputs .#devShells.aarch64-darwin.default
nix develop
```

Avec direnv, mettre `export PROJECT_CACHE_ACCESS=read-only` dans `.envrc.local`,
puis `direnv allow` : `.envrc` effectue le pull avant de charger le shell.
Un échec d'authentification n'est plus mémorisé comme une récupération réussie.
Le marqueur d'un pull réussi reste valable jusqu'au changement des entrées Nix ;
après une nouvelle publication CI, `rm .direnv/ovh-pull-*` force une nouvelle tentative.

Dans ce shell, hydrater les assets et récupérer la release :

```sh
git submodule update --init private-assets
git -C private-assets lfs pull --include='assets/**'
pwsh -NoProfile -File tools/setup_assets.ps1
python3 tools/nix-cache.py pull .#game .#game-deps
# Garde-fou : interdit toute construction locale si la release n'est pas publiée.
nix build .#game --max-jobs 0 --option builders '' --option fallback false
BEVY_ASSET_ROOT="$PWD" nix run .#game --max-jobs 0 --option builders ''

# Éditeur fourni par le shell, avec le projet courant.
jackdaw
```

Attendre une publication CI réussie pour **la même révision**, la même plateforme
et les mêmes entrées Nix. Un miss du jeu ne doit pas être suivi aveuglément d'un
build release local : le LTO peut durer une heure. Les modifications de `tools/`
participent aussi au hash de la dérivation du jeu ; une release d'un ancien commit
n'est donc pas automatiquement celle du checkout modifié.

Le binaire de l'éditeur et son SDK sont en cache Nix. Le premier Play In Editor
compile en revanche un autre graphe Cargo dans `.jackdaw/target`, avec ses flags,
features et compilateur SDK : télécharger Jackdaw ne précompile pas ce graphe.
`cargo run` compile le profil dev avec les features par défaut (`asset_watch`,
`editor`, `perf_ui`). Il ne réutilise pas directement les artifacts Crane release.
Les extensions Flow Graph restent une étape séparée, décrite plus bas.

### Mesure du shell vierge, 9 novembre 2026

Révision de référence `c15a174`, macOS ARM64, Nix 2.25. Store de destination vide
`/private/tmp/collab-store` (sur macOS, `/tmp` est un lien symbolique que le store
local Nix refuse). Le store système n'a pas été nettoyé ni utilisé comme source
de la copie. La toolchain du helper et les sources de flake étaient déjà locales ;
le temps ci-dessous n'inclut pas leur installation initiale ni le login Infisical.

```sh
nix run .#nix-cache -- pull --inputs .#devShells.aarch64-darwin.default \
  --to-store /private/tmp/collab-store --report /private/tmp/shell-pull.json
```

| Mesure | Résultat |
| --- | --- |
| Entrées directes demandées au cache OVH | 31 hits, 0 miss |
| Copie signée de leurs closures vers le store vide | 93,43 s |
| Chemins dans la closure des entrées du shell | 233 |
| Disponibles sur cache.nixos.org | 204, soit 3 504 344 704 octets NAR |
| Restants disponibles sur OVH | 29, soit 6 049 838 760 octets NAR |
| Absents des deux caches | 0 |
| Audit des narinfo (12 requêtes parallèles) | 44,93 s |

Les tailles NAR sont décompressées, pas des volumes réseau. L'audit essaie d'abord
cache.nixos.org, puis OVH pour les 404 ; les 29 chemins ne représentent donc pas
tout le contenu OVH. La copie chronométrée utilise exclusivement OVH, pas un
mélange optimisé des deux caches. Aucun outil du shell ne doit être recompilé
pour ces entrées : Dioxus, Jackdaw, les deux Rust et les outils sont disponibles.
Le petit environnement du shell et le bootstrap de la façade sccache restent
locaux. Cette mesure est celle des **entrées du shell**, pas de la closure de
construction récursive du jeu ni d'un répertoire Cargo utilisateur prérempli.

### Release et éditeur mesurés

La CI Rust `42628426798`, job `build (macos-15, macos-arm64)`, a publié les sorties
signées le 9 novembre à **14:02:35 UTC** (étape de publication : 12 s).
Le pull lecteur `game` + `game-deps` de `c15a174` donne **2 hits, 0 miss en
38,47 s**. Les sorties étaient déjà valides dans le store système : cette durée
inclut l'évaluation et la vérification des métadonnées, pas un téléchargement neuf.
Une seconde copie vers le store isolé récupère effectivement les sorties :
**2 hits, 0 miss en 124,12 s** ; closure `game` + `game-deps` : **928 chemins
valides, 4 833 863 520 octets NAR**. Cette copie intervient après celle du shell,
donc ses dépendances communes sont déjà présentes dans le store isolé.
Le build avec `--max-jobs 0 --option builders '' --option fallback false`
termine en **6,87 s**, **aucune dérivation construite**.

Les correctifs de ce parcours changent le hash Nix du jeu : pour reproduire la
validation de la publication existante, utiliser la référence propre :

```sh
reference="git+file://$PWD?rev=c15a174fb48a8701afa638b160786d44cefa0a77"
python3 tools/nix-cache.py pull "$reference#game" "$reference#game-deps"
nix build "$reference#game" --no-link --max-jobs 0 --option builders ''
BEVY_ASSET_ROOT="$PWD" nix run "$reference#game" --max-jobs 0 --option builders ''
```

Le test du jeu utilise `BEVY_ASSET_ROOT=/Users/pzachary/bevy-parkour2` sur la machine
partagée : fenêtre Metal, chargement des assets et modèles GLTF, aucune erreur
`Path not found … scene.bsn`, puis arrêt du processus après 25 s de commande.
Ce chemin est propre à la machine de mesure ; utiliser son clone hydraté ailleurs.
Les liens locaux vers `assets/character` et `assets/animations` sont ignorés par Git,
y compris lorsqu'il s'agit de liens symboliques plutôt que de répertoires.

Jackdaw crée sa fenêtre Metal environ **3,1 s après son premier log** et ouvre la
scène du projet, sans compilation de son exécutable. Le test l'arrête après
20 s. Limites : extensions utilisateur déjà présentes dans le profil partagé,
cinq types du jeu absents du schéma sans build du projet, et erreur de loader
`materials/ground.material.bsn`. Ce smoke test valide le démarrage de l'éditeur,
pas une session PIE complète ni un profil utilisateur entièrement vierge.

### Mesures Cargo en lecture seule

Commandes exécutées depuis ce worktree, différent du clone principal, avec
`PROJECT_CACHE_ACCESS=read-only` et quatre jobs, sources Cargo déjà locales :

```sh
cargo build -j 4
cargo xtask test --suites xtask --jobs 4
```

| Mesure | Temps commande | Hits OVH | Misses OVH | Taux hits/(hits+misses) | Écritures OVH |
| --- | --- | --- | --- | --- | --- |
| Jeu dev, target initialement vide | 974,91 s (16 min 14,91 s) | 291 | 206 | 58,6 % | 0 |
| xtask test, après le build dev | 65,30 s | 48 | 190 | 20,2 % | 0 |
| Démarrage automatique du projet Jackdaw, interrompu avec l'éditeur | pas un build complet | 0 | 11 | 0 % | 0 |

Le premier pilote xtask en dev prend 31,38 s ; la commande complète exécute
32 tests, tous réussis, et en ignore 5. Les 48 hits du delta xtask sont natifs
(C/C++/assembleur), pas Rust. Les suites ci ont donc encore beaucoup à compiler.
Le jeu dev obtient 254 hits Rust et 37 hits natifs ; **206 requêtes cacheables
OVH manquent**, auxquelles s'ajoutent les binaires, proc-macros et autres appels
non cacheables. Ce nombre mesure le manque observé, pas le nombre de crates
que le seul changement de profil pourrait expliquer.

Les compteurs sont cumulés sur les ports sccache de ce worktree, car le démon
change à l'heure UTC et selon le contexte du compilateur. Le démon distinct du
lancement Jackdaw est séparé de la ligne dev ; xtask est mesuré par différence
avant/après, sans compilation Cargo concurrente dans ce worktree. Les autres
builds de la machine partagée continuent : ces temps ne sont pas des benchmarks
sur machine dédiée. Pendant dev, un échec temporaire Infisical provoque aussi
**1 miss et 1 écriture sur disque local**, puis OVH redevient accessible. Ne pas
présenter le total de toutes les écritures locales comme égal à zéro.

Les statistiques JSON des démons OVH confirment `effective_cache_mode=ReadOnly`
et `cache_writes=0`. sccache rapporte également des `cache_write_errors`
(206 pour dev, 190 pour xtask) malgré ce mode ; ces compteurs ne sont pas des
écritures réussies. Les statistiques usuelles après la commande peuvent ne voir
qu'un des démons : collecter leurs JSON via les ports de `.compiler-cache/ports/`
si un build traverse une rotation horaire.

`SCCACHE_BASEDIRS` contient bien le worktree et `/Users/pzachary/.cargo/` ; des
hits Rust existent donc dans ce nouveau chemin. Cette observation ne mesure
pas isolément l'effet de BASEDIRS : aucun second build sans cette variable n'a
été fait et le bucket contient plusieurs producteurs. Les flags/profils, cwd et
environnements distincts restent des causes d'incompatibilité possibles.
Les logs et JSON bruts sont conservés localement dans le répertoire ignoré
`artifacts/collaborator-bootstrap/` ; les résultats ne nécessitent aucun secret
d'infrastructure. Le répertoire `target/debug` occupe environ 8,2 Go avant la fin
du build, indépendamment du store Nix.

### Profils CI et limites de sccache

| Commande | Profil et features | Réutilisation attendue |
| --- | --- | --- |
| `cargo run` / `cargo build` | dev, defaults : asset_watch/editor/perf_ui ; dépendances opt-level 3 | Cache dev compatible requis |
| `cargo xtask test` | pilote xtask en dev ; suites en ci, dépendances opt-level 1 | Ne préchauffe pas tout le jeu dev |
| `cargo xtask coverage` | coverage, opt-level 0, instrumentation | Clés distinctes |
| `cargo xtask build --targets macos-arm64` | release, sans defaults, distribution + macos-vulkan (+ sentry si configuré) | Pas le graphe dev/PIE |
| `nix build .#game` | Crane release, distribution, sans defaults ni sccache | Closure Nix, pas le target Cargo du clone |
| Play In Editor Jackdaw | target `.jackdaw/target`, flags/SDK spécifiques | Cache distinct à mesurer séparément |

La CI actuelle ne préchauffe **pas tout le jeu en profil dev**. Elle compile le
pilote xtask en dev, les suites en ci/coverage et les distributions en release.
La façade fournit automatiquement le checkout et Cargo home dans
`SCCACHE_BASEDIRS`. Cela ne garantit pas des hits Rust entre deux chemins : cwd,
variables Cargo, flags, features, compilateur et profil participent encore aux
clés (voir la mesure de changement de chemin dans [REMOTE-CACHE.md](REMOTE-CACHE.md)).
Des hits dans un nouveau worktree prouvent une réutilisation partielle, pas que
chaque objet publié par GitHub est portable vers ce chemin.

Recommandation : ajouter dans un job macOS de confiance un préchauffage
`nix develop -c cargo build --locked` avec les defaults et le profil dev inchangés,
puis vérifier les hits depuis **un second checkout** avant de promettre un gain
collaborateur. Prévoir séparément le build PIE avec le SDK Jackdaw et ses flags.
Un simple alignement de dev sur ci réduit l'optimisation des dépendances et
change l'expérience interactive : ne pas le faire sans comparer les performances.
Aucun workflow n'a été déclenché pour cette mesure et aucun préchauffage CI
supplémentaire n'est activé par cette documentation.

## Première utilisation

Installer Nix avec les fonctions `nix-command` et `flakes` activées, puis depuis
la racine du clone :

```sh
nix develop
```

### direnv et VS Code

Le dépôt contient un `.envrc` pour [nix-direnv](https://github.com/nix-community/nix-direnv)
(3.0.6+, sinon téléchargé avec vérification de hash). Après `direnv allow`, entrer
dans le clone charge `nix develop .#default` ; le résultat est mis en cache dans
`.direnv/` et protégé du GC, donc la réouverture est immédiate. Dans VS Code,
l'extension recommandée `mkhl.direnv` charge le même environnement et redémarre
les extensions (`direnv.restart.automatic`) : rust-analyzer, CodeLLDB, tâches et
terminaux utilisent alors la toolchain Nix.

Quand les entrées Nix changent (flake, `nix/`, toolchain, verrous d'outils),
`.envrc` récupère d'abord, en lecture seule, les entrées du shell déjà publiées
par la CI dans le cache OVH (`nix run .#nix-cache -- pull --inputs ...`), avec le
login Infisical de l'utilisateur. Sans login ou sans entrée publiée, le shell se
construit normalement depuis cache.nixos.org. `PARKOUR_OVH_PULL=0` dans
`.envrc.local` (ignoré par Git) désactive cette étape ; `rm .direnv/ovh-pull-*`
la relance. Premier shell macOS ARM sans cache OVH mesuré : 16 min 37 s.

Sur une modification non encore commitée, Nix doit voir les nouveaux fichiers
dans Git : `git add -N flake.nix flake.lock nix tools/xtask/src/toolchain.rs`
permet de les exposer sans en indexer le contenu. Sous WSL, utiliser un clone
Git créé depuis Linux ; un worktree Windows avec un `.git` pointant vers `C:/...`
ne se comporte pas comme un clone Linux.

Le premier démarrage peut être long : Dioxus CLI **0.7.2** est compilé puis
conservé dans le store. Sur Linux x86_64 et macOS ARM64, le bundle officiel Jackdaw/jd/SDK
**0.20.1-rc.1** est téléchargé avec vérification SHA-256 et adapté au store Nix,
avec ses extensions Physics/Navmesh. Le shell
contient Rust **1.97.1**, rustfmt, Clippy, rust-analyzer, rust-src, LLVM coverage,
la cible wasm32, les compilateurs C/C++, Clang **22** pour le WebGPU, les
bibliothèques Bevy propres à la plateforme, Git/LFS, Infisical, PowerShell, Node
et Java. macOS utilise le SDK Apple de Nixpkgs et un déploiement minimum macOS 13.
Nextest, llvm-cov,
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

Le workflow [Nix Apple Silicon](../.github/workflows/nix-dev.yml) vérifie le shell
complet, les versions des deux compilateurs et le démarrage des CLI sur un
runner macOS ARM, puis construit le jeu et les dépendances Crane et exécute
les tests xtask purs. Il se déclenche sur les changements de la flake et de ses
outils, sans assets privés ni secrets de build.

Les assets privés restent soumis à [l'accès au dépôt d'assets](GITHUB.md).
Dans le shell, après obtention de cet accès :

```sh
git submodule update --init private-assets
git -C private-assets lfs pull --include='assets/**'
pwsh -NoProfile -File tools/setup_assets.ps1
```

La flake du jeu contient le binaire, pas les assets licenciés. Pour accéder au
cache sccache OVH, effectuer le login Infisical décrit dans
[REMOTE-CACHE.md](REMOTE-CACHE.md). Sans login, sccache fonctionne localement.

## Jeu, éditeur et hotpatch

Depuis la racine du projet, dans `nix develop` :

```sh
# Jeu en développement
cargo run

# Jeu release sans éditeur ni overlay de développement
cargo run --release --no-default-features

# Éditeur Jackdaw, extensions et schéma du projet
pwsh -NoProfile -File tools/editor_extension.ps1 -Open

# Dioxus : le lanceur existant conserve le cache malgré son wrapper hotpatch
pwsh -NoProfile -File tools/cached_command.ps1 dx serve --hot-patch --features hotpatch
```

La commande `editor_extension.ps1 -Open` (également utilisée par le lancement
VS Code « Éditeur (Jackdaw) ») enregistre automatiquement le SDK Nix avec
`jd editor locate`, construit et installe Flow Graph et l’extension du projet,
puis ouvre l’éditeur épinglé. Sans `-Open`, elle prépare seulement les extensions.
Les sources inchangées ne sont pas recompilées. Les bundles de développement
sont signés avec `.compiler-cache/jackdaw-publisher.pk8`, une clé locale ignorée
par Git ; conserver une clé de publication distincte pour les releases.

Sur macOS, le script transmet le répertoire de l’éditeur et `sdk/host-deps` dans
`DYLD_FALLBACK_LIBRARY_PATH` aux outils SDK et à l’éditeur. Ne pas les mettre
dans `DYLD_LIBRARY_PATH` : cela remplace aussi le libiconv système et casse
Cargo dans le PIE. Le store reste en lecture seule ; les builds et le schéma
sont dans `.jackdaw/` (également sous chaque crate d’extension), les bundles
installés dans le répertoire utilisateur Jackdaw.

Les composants du jeu (AnimationController, CharacterController, les caméras)
restent des types décrits par `.jackdaw/schema.json`, extrait par `jd build`.
Ils ne doivent pas activer les systèmes de gameplay dans l’éditeur. Au premier
`-Open`, le script construit ce schéma avant d’ouvrir la scène ; ensuite le
build automatique de Jackdaw le rafraîchit. Si le schéma manque, un lancement
direct de `jackdaw` peut annoncer temporairement « type not in the registry ».
L’extension du projet fournit le chargeur `StandardMaterial` des fichiers
`.material.bsn`, en plus du correctif des cheveux du Ninja.

Pour mesurer les deux compilateurs séparément (ils utilisent des daemons
sccache distincts, avec le même bucket OVH) :

```sh
.compiler-cache/bin/sccache --show-stats
RUSTUP_TOOLCHAIN=1.99.0 .compiler-cache/bin/sccache --show-stats
```

Les assets privés doivent déjà être préparés. Pour réutiliser ceux d’un autre
clone local sur Unix, des liens symboliques `assets/character` et
`assets/animations` sont également ignorés par Git ; ne pas les copier ni les
ajouter à Git. Le bouton Play lance un build **debug** puis le jeu avec le
protocole PIE ; le premier build est distinct du cache d’artifacts Crane.

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

Linux ARM64 utilise `--targets linux-arm64`, macOS ARM64 `--targets macos-arm64`.
Pour un checkout public
sans assets, utiliser `--suites xtask,graph,asset-export,noodle` pour les tests et
la couverture. Les rapports se trouvent dans `artifacts/<run>/` : JUnit,
HTML/LCOV/JSON pour la couverture, manifestes et ZIP vérifiés pour le packaging.

La CI utilise `XTASK_COVERAGE_CLEAN_TARGETS=1` pour supprimer le répertoire de
compilation isolé de chaque suite après ses rapports et doctests. Cela limite
le pic disque ; les rapports et logs sont conservés. Les exécutions locales
gardent ces répertoires sans cette option.
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

# Évaluer tous les outputs sans compiler, pour les trois plateformes
nix flake check --all-systems --no-build

# Checks purs : release du jeu et tests unitaires de xtask
nix flake check
```

Le lanceur du package Nix place temporairement l'exécutable à côté d'un lien
vers les assets du clone, car le mode distribution cherche les assets à côté
de son exécutable. Il nettoie ce répertoire à la sortie. Aucun asset licencié
n'est copié dans le store ; le binaire natif se trouve dans `result/libexec/`.
Sur macOS, le lanceur reproduit la structure d'un bundle (`MacOS/` et
`Resources/assets`) que le mode distribution y attend. Le binaire compilé est
la sortie séparée `game-unwrapped` : modifier le lanceur ne reconstruit que ce
petit wrapper, pas la release LTO.

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
Les téléchargements xtask utilisent `.xtask-cache` en CI, via `XTASK_CACHE_DIR`,
pour que le nettoyage du cache Cargo ne supprime pas leurs reçus et exécutables.
En local, le défaut reste `target/xtask-cache` ; `--cache-dir` est prioritaire.

Les jobs disposant de l'écrivain OVH chargent les identifiants avant l'action
Nix, puis exécutent `nix run .#nix-cache -- pull --inputs <shell-ci>` avant le
premier `nix develop`. Ils ne restaurent ni ne sauvegardent le store GitHub.
Un échec de lecture OVH reste visible, mais permet la construction habituelle
avec cache.nixos.org et les sources. Les signatures Nix restent obligatoires.
Les jobs sans écrivain, dont les PR sans secrets, gardent `cache-nix-action` :
clé `nix-v2` partagée par OS/architecture et empreinte des entrées Nix/Cargo,
sans job ni SHA du commit ; seules les exécutions hors PR sauvegardent.
Le seuil de GC de ce repli reste à 8 Gio ; les GC roots restent prioritaires.
Aucune purge des anciennes entrées GitHub n'est déclenchée par cette migration.

Les artifacts Cargo des commandes dans le clone restent dans `rust-cache` :
les closures Crane ne remplacent pas ces répertoires. Le job Apple Silicon,
qui ne construit que des dérivations pures, désactive ce cache Cargo via
`cargo-cache: 'false'`. Le contrôle final des jobs utilise directement les
résultats GitHub et ne prépare plus de compilateur ni de cache.

Les lancements manuels sur une branche nécessitent une autorisation de cette
branche dans l'environnement GitHub `build` pour récupérer les assets privés.
Infisical reste limité à `main` et aux tags `v*` : les autres branches utilisent
sccache local, le cache Cargo de GitHub et le store Nix. L'analyse Sonar avec ses
identifiants dédiés reste réservée à `main`.

## Cache binaire OVH partagé

Le bucket privé `yummy-blagg`, à Paris, héberge les deux caches : sccache sous
`bevy-parkour2/v1/`, et Nix sous `nix-cache-info`, `*.narinfo` et `nar/*`.
Les lecteurs Infisical build `/cache` peuvent lire ces objets ; leur politique
OVH interdit toujours écriture, suppression et listing. Les mainteneurs utilisent
leurs identifiants privés d'infrastructure pour écrire. La clé de signature
`NIX_CACHE_SIGNING_KEY` reste dans le projet Infisical d'infrastructure ; sa clé
publique est suivie dans [nix/cache-public-key](../nix/cache-public-key).

Dans `nix develop`, après `infisical login --domain https://eu.infisical.com` :

```sh
# Collaborateurs : récupérer les résultats existants puis construire les absents.
python3 tools/nix-cache.py pull .#game .#game-deps
nix build .#game .#game-deps

# Mainteneurs : signer et publier les résultats compilés et leur closure.
python3 tools/nix-cache.py push .#game .#game-deps
```

Un chemin `/nix/store/...` ou un lien `./result` est également accepté. Le helper
utilise les variables AWS du processus si elles sont déjà définies, ou un
`AWS_PROFILE` explicite. Le helper vérifie d'abord l'accès à `nix-cache-info` ;
un refus ou un problème réseau à ce stade échoue explicitement. Un 404 de
métadonnées est un cache miss. OVH renvoie aussi 403 pour une clé absente quand
le lecteur n'a pas de droit de listing : cette réponse est signalée comme
« absente ou refusée », puis le build local peut continuer. Seule une copie
réussie compte comme hit. La lecture est effectuée
par le client Nix : aucun fichier d'identifiants AWS n'est fourni au daemon.
La clé privée de signature n'est matérialisée que dans un répertoire temporaire
privé pour `nix store sign`, puis supprimée, même en cas d'échec.

Pour un Nix multi-utilisateur, l'administrateur doit ajouter la clé publique de
`nix/cache-public-key` à `extra-trusted-public-keys` dans `/etc/nix/nix.conf` et
recharger son daemon. Un utilisateur Nix autorisé peut aussi transmettre cette
clé via `--extra-trusted-public-keys`. La vérification des signatures reste active.
Le helper `pull` prépare les sorties avant le build ; l'environnement Nix reste
accessible sans connexion au bucket. `pull --inputs <shell>` récupère les entrées
manquantes d'un shell avant sa construction ; `nix run .#nix-cache -- ...` fournit
le helper, Python/boto3 et Infisical 0.45.4 sans entrer dans le shell.

Les runners GitHub publient les deux caches. Les environnements GitHub `build`
et `release` (main, tags `v*` et branches autorisées par le propriétaire)
reçoivent de Pulumi les secrets `OVH_CACHE_WRITER_ACCESS_KEY_ID`,
`OVH_CACHE_WRITER_SECRET_ACCESS_KEY` et `NIX_CACHE_SIGNING_KEY`. Ces jobs passent
sccache en `READ_WRITE`, restaurent les sorties Crane, puis signent et publient
avec `nix-cache.py push` : profil du shell CI, `game` et `game-deps` ; le job
Apple Silicon publie aussi le profil du shell complet (Dioxus, Jackdaw, SDK) et
les tests xtask. Un cache binaire n'accepte un chemin que si ses références y
sont déjà : les closures complètes sont donc publiées, en zstd ; les chemins
déjà présents ne sont pas renvoyés. Les pull requests ne reçoivent aucun secret.
L'utilisateur OVH `cache-ci-writer` peut lire, écrire et lister le cache, sans
suppression. Les collaborateurs gardent l'identité Infisical en lecture seule.
Les jobs sans écrivain gardent le repli du store GitHub ; les commandes Cargo
dans le clone conservent leur cache d’artifacts GitHub.

Aucun secret ni asset privé ne doit entrer dans une dérivation publiée.
La propagation d'une modification de politique OVH peut être différée : vérifier
un GET réel avant de conclure qu'un nouvel accès fonctionne.

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

### Préchauffage dev : limite de partage des clés

Le profil interactif est `dev` (jeu opt-level 1, dépendances 3, debug complet),
avec `asset_watch,editor,perf_ui` par défaut. CI teste en `ci` (debug 1,
dépendances opt-level 1), couvre en `coverage` (instrumentation et opt-level 0),
et distribue en release sans ces features. Le build Windows de distribution
ajoute `-C target-feature=+crt-static`. Ces unités ne préchauffent donc pas
systématiquement `cargo run`. Le lancement de xtask lui-même utilise déjà dev,
mais sa compilation séparée a une résolution de features différente du jeu.

Commandes qui seraient nécessaires pour un writer dev, séparément (pas avec
`--workspace`, qui peut unifier autrement les features) :

```sh
cargo build --locked -p xtask
cargo build --locked -p bevy_parkour2
```

Il faudrait des jobs indépendants par OS/architecture, sans `needs` dans le gate,
réservés à l'écrivain OVH hors PR ; ils ne doivent pas lancer le jeu. Toutefois,
**aucun job dev supplémentaire n'est ajouté à ce stade**, car sccache 0.19 ne
rend pas ces clés Rust portables vers un checkout quelconque. Changer uniquement
`SCCACHE_BASEDIRS` ne réécrit ni le cwd ni les variables Cargo dans ces clés.
Normaliser aveuglément ces variables peut également changer `env!` et les
chemins de debug embarqués ; ce n'est pas une correction sûre du frontal.

La sonde reproductible suivante utilise une bibliothèque minuscule, deux chemins
et un daemon/cache disque isolé ; elle n'utilise jamais OVH et n'arrête aucun
daemon de compilation du projet :

```sh
direnv exec . python3 tools/cache-key-probe.py
```

Résultat local sur macOS ARM, toolchain Nix du projet : répétition identique =
1 hit ; changement du seul cwd = clé différente et miss ; changement du seul
`CARGO_MANIFEST_DIR` = clé différente et miss. Les deux racines sont pourtant
présentes dans `SCCACHE_BASEDIRS`. `artifacts/cache-key-probe.json` contient les
clés et les statistiques cumulatives, sans secrets. La sonde vérifie aussi
l'ajout de `CARGO_TARGET_DIR`, qui est haché par sccache.

Cela concerne les crates du workspace/vendor (chemins du checkout), et les
crates registry/git quand le chemin Cargo home diffère entre collaborateur et
runner. La liste exacte dépend des unités déjà présentes dans OVH : le taux
58,6 % du jeu ou 20,2 % de xtask ne suffit pas à l'inférer. Le même OS, la même
architecture, le compilateur, les features et les flags restent nécessaires.
Les shells Nix `default` et `ci` utilisent le Rust du projet, mais leurs chemins
Cargo/checkouts ne sont pas garantis identiques sur deux machines.

Jackdaw Play écrit dans `.jackdaw/target/game/debug`. `--out-dir` est exclu de
la clé sccache, mais une variable `CARGO_TARGET_DIR` différente ne l'est pas ;
le chemin de sortie seul ne prouve donc ni partage ni séparation des clés.
Il reste à capturer ses arguments/environnement réels pour comparer au Cargo
dev ordinaire. Les extensions SDK utilisent une autre ABI/toolchain et Dioxus
hotpatch ajoute des features/flags : aucune promesse de préchauffage pour ces
unités. Il faut d'abord une solution de clés Rust relogeables validée, ou un
environnement de chemins contrôlés accepté, avant d'ajouter ce coût CI.

Mesure dans le worktree de cette enquête (9–10 novembre, macOS ARM) :
`PROJECT_CACHE_ACCESS=read-only cargo build --locked -p xtask --timings` réussit
en **29,63 s** ; `PROJECT_CACHE_ACCESS=read-only cargo build --locked
-p bevy_parkour2 --jobs 4 --timings` réussit en **17 min 45 s**, avec les features
par défaut, sans lancer le jeu. Les rapports Cargo sont dans
`target/cargo-timings/` ; le log jeu a été gardé dans
`/tmp/sccache-study/dev-build.log`. Un relevé intermédiaire du daemon commun
aux deux commandes indiquait 111 hits Rust / 119 misses Rust et 25 hits natifs ;
ce n'est **pas** le bilan du jeu ni une comparaison isolée xtask/jeu. Le daemon
avait été renouvelé au relevé final (compteurs vides, mode `ReadOnly` confirmé),
donc aucun taux final n'est annoncé. La machine partage ses ressources avec
d'autres travaux ; ces temps ne sont pas une prédiction de durée CI.

Les quatre tests du frontal passent, ainsi qu'une invocation réelle vérifiant
l'enregistrement d'un extern et l'exclusion d'un jeton de registre sentinelle.
Le parseur PowerShell et la liste de messages autorisés du filtre passent.
`nix flake check --all-systems --no-build` réussit ; actionlint ne retourne que
le SC2016 préexistant de `nix-dev.yml:73`. Aucun push, déclenchement de workflow,
changement d'infrastructure, publication de cache, GC Nix ou modification globale
n'a été réalisé. Le clone de travail de l'utilisateur n'a pas été modifié.
