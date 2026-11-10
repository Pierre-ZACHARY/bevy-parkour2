# Mesures et réduction de durée CI

Mesures du 9 novembre 2026, avant modification. Sources : métadonnées des jobs,
logs et artifacts `summary.json` de GitHub, lus avec `gh run view --json jobs`,
`gh run view --log` et `gh run download` ; aucune CI déclenchée pour cette étude.
Les durées de jobs incluent leurs étapes post-action. Elles ne s'additionnent
pas entre jobs parallèles et ne comprennent pas leur attente en file.

- [Rust froid 42416727793](https://github.com/Pierre-ZACHARY/bevy-parkour2/actions/runs/42416727793).
- [Rust chaud 42487062435](https://github.com/Pierre-ZACHARY/bevy-parkour2/actions/runs/42487062435).
- [Apple Silicon 42529884915](https://github.com/Pierre-ZACHARY/bevy-parkour2/actions/runs/42529884915).
- Les runs 42628426798 et 42628430476 étaient encore en cours lors du relevé :
  leurs durées incomplètes ne servent pas à extrapoler les gains.

Durées en minutes:secondes. « Travail » désigne l'étape xtask principale ;
les builds Crane et leur transfert sont donc séparés.

| Job Rust | Total froid | Total chaud | Action Nix chaude | Travail chaud | Post-cache chaud |
| --- | ---: | ---: | ---: | ---: | ---: |
| tests (windows-2022, windows-x64, test) | 99:47 | 53:58 | — | 50:10 | 2:48 |
| tests (ubuntu-24.04-arm, linux-arm64, test) | 56:00 | 42:10 | 24:06 | 15:58 | 1:46 |
| tests (ubuntu-24.04, linux-x64, coverage) | 94:26 | 31:45 | 5:08 | 23:08 | 3:10 |
| build (ubuntu-24.04, linux-x64,webgpu) | 82:18 | 32:05 | 5:22 | 23:17 | 2:00 |
| build (windows-2022, windows-x64) | 64:13 | 42:45 | — | 39:12 | 2:30 |
| build (ubuntu-24.04-arm, linux-arm64) | 60:08 | 42:51 | 25:50 | 10:56 | 2:15 |
| build (macos-15, macos-arm64) | 113:39 | 42:05 | 6:42 | 32:31 | 1:52 |
| lint | 25:15 | 14:45 | 5:09 | 7:29 | 1:47 |
| tests (macos-15, macos-arm64, test) | 72:19 | 32:16 | 5:56 | 21:53 | 3:58 |
| All required checks | 7:55 | 6:37 | 3:15 | 2:32 | 0:46 |

| Étape Apple Silicon | Avant |
| --- | ---: |
| Action Nix | 4:19 |
| Validation du shell complet | 25:06 |
| Crane release + checks xtask | 33:01 |
| Publication OVH | 1:05 |
| Post-cache GitHub | 4:18 |
| Total | 68:10 |

## Goulots et changements

1. **Identifiants trop tardifs.** Les shells CI étaient préparés avant les
   identifiants OVH. Infisical OIDC, avec le contrôle d'isolation conservé,
   précède maintenant l'écrivain d'environnement puis l'action Nix. Le lecteur
   Infisical ne remplace donc pas l'écrivain. Tous les jobs Nix qui ont des clés
   AWS peuvent tirer les entrées du shell avant son premier `nix develop`.
   Sans identifiants, le repli public reste fonctionnel. `pull --inputs` normalise
   aussi les listes `inputDrvs` de Nix 2.25 et les objets du JSON Nix 2.35.
   Sur ARM Linux : potentiel de 19–23 minutes par job si les 24–26 minutes de
   préparation deviennent 1–5 minutes de téléchargement. Ce budget reste une
   hypothèse à mesurer, pas un résultat après modification.
2. **Store GitHub redondant.** Les écrivains OVH ne restaurent/sauvegardent plus
   ce store. Le repli utilise une clé stable par OS/architecture et dépendances,
   sans job ni SHA ; les PR ne sauvegardent pas. Aucun seuil GC n'est abaissé :
   le profil retient une grosse closure et un GC plus agressif ne garantit pas
   un cache plus petit. Les post-actions Nix/Cargo prenaient 1:46–3:58 ; seule
   leur fraction Nix disparaît, pas toute cette durée combinée.
3. **Shell et Crane Apple Silicon.** Le pull du shell complet existant est gardé,
   et `game`, `game-deps` et le check xtask sont désormais tirés avant `nix build`.
   Avec des sorties correspondantes publiées, potentiel de 20–24 minutes sur
   le shell et 30–32 minutes sur Crane (comparaison avec les pulls Crane Rust
   de 20–198 secondes). Un changement de source invalide le jeu et réduit ce
   gain. Le premier run de cette branche peut donc devoir le recompiler.
4. **Contrôle final coûteux.** L'expression GitHub vérifie directement `needs`
   avec `always()`, sans checkout ni Nix/Cargo. Même politique que `xtask gate` :
   lint/tests obligatoires, build ignoré seulement en PR, Sonar/Sentry ignorables
   mais jamais en échec ou annulés. Budget attendu : quelques secondes contre
   6:37 sur le run chaud, soit environ 6:30 gagnées sur le chemin critique.
5. **Windows séquentiel.** Les artifacts détaillent jeu 19:49, xtask 1:02,
   graph 9:37, asset-export 2:35, noodle 13:33, plus quelques secondes de
   doctests. La suite jeu compile pendant 19:01, puis exécute ses tests en
   5 secondes : augmenter le parallélisme nextest ne résout pas ce coût.
   Deux groupes de matrice Windows exécutent maintenant jeu et tooling en
   parallèle, avec clés Cargo et noms d'artifacts distincts. En PR, seul le
   groupe tooling lance des tests ; les autres plateformes gardent leur liste
   publique. Estimation : 27–30 minutes au lieu de 50:10 pour les tests Windows,
   si deux runners sont disponibles. Le build Windows à 42:45 devient alors
   le candidat au chemin critique. À durées égales ailleurs, objectif Rust
   chaud autour de 43–46 minutes contre environ une heure, soit 15–18 minutes
   de gain total, contrôle final compris. Ne pas additionner à cela les gains
   des jobs Linux qui ne sont pas sur ce chemin critique.

## Quota GitHub

Le relevé API instantané contient 4 caches, soit **8,93 Go décimaux** : store
Apple 3,92 Go, store required 2,81 Go, Cargo Apple 1,04 Go, Cargo required
1,16 Go. Il diffère du relevé antérieur à 12,6 Go : les évictions ont continué.
Ces quatre entrées ne seront plus recréées sur les jobs correspondants avec
écrivain : le job dev-shell désactive `rust-cache` (builds Nix purs), et required
ne prépare plus aucun cache. Cela représente 8,93 Go de renouvellement évité
sur cet inventaire, **pas 8,93 Go immédiatement libérés** : aucune suppression
n'a été effectuée. Les caches Cargo des vrais builds/tests restent nécessaires
pour les exécutables et proc-macros non cachables par sccache. Le nouveau groupe
Windows ajoute un cache Cargo : son volume et les évictions doivent être suivis.
La somme des closures de plusieurs architectures peut encore dépasser le quota
si de nombreux jobs sans écrivain sauvegardent le repli GitHub.

## Misses Windows et couverture

Les tests Windows ont 783 hits / 173 misses (81,90 %), dont 715/173 en Rust et
68/0 en C/C++. Le build a 165/21 (88,71 %), sans erreur de lecture/écriture S3.
Les deux jobs annoncent rustc 1.97.1 (8bab26f4f). Il existe aussi 231 appels non
cachables dans les tests, dont 97 binaires et 62 proc-macros, distincts des
173 misses. `SCCACHE_BASEDIRS` contient déjà le checkout et Cargo home. Les stats
agrégées ne donnent ni la crate ni le motif de chaque miss ; elles ne prouvent
pas une anomalie de chemin. Aucun changement spéculatif de compilateur, de
normalisation ou de clé sccache n'est appliqué. Pour aller plus loin : reproduire
une invocation identique sur deux runners Windows, avec traces des clés de
cache et comparaison des fichiers générés/flags, en protégeant les identifiants.

La couverture Linux chaude passe 5:55 dans game-nextest, 3:16 dans game-doctests,
6:24 dans noodle-nextest ; les exports HTML/LCOV/JSON sont sous une seconde
chacun sauf l'HTML noodle à 0,94 seconde. Elle prend 23:07 de travail, avec
100 % de hits sccache : liaison, build scripts et doctests restent coûteux.
Elle n'est plus le chemin critique du run chaud. La découper maintenant ajoute
runners, caches et fusion des artifacts Sonar pour un gain mural incertain ;
conserver l'isolation et le nettoyage par suite qui protègent l'espace disque.

## Validation et limites

- `nix develop .#ci -c actionlint .github/workflows/*.yml` : seul SC2016
  préexistant dans nix-dev.yml, aucune autre erreur.
- `nix flake check --all-systems --no-build` : réussi sur les trois systèmes.
- Six tests Python hors réseau : JSON Nix 2.25/2.35, entrée déjà présente,
  shell sans entrée, code de sortie 1 pour entrée absente et erreur du store. Commande : `nix develop .#ci -c python3 -m unittest discover
  -s tools -p test_nix_cache.py`.
- Expression du contrôle final comparée à la politique xtask sur 9 375
  combinaisons d'événement/statuts, y compris statut absent et annulation.

Aucun push, déclenchement CI, changement infra, écriture OVH, GC Nix, changement
de configuration globale ou build release local n'a été effectué.
Un run réel doit vérifier les hits du shell avant préparation, les signatures,
les replis sans secrets et en panne OVH, l'ordre OIDC/écrivain, les noms des
artifacts Windows, le regroupement des cinq suites, le gate après échec/annulation,
la disponibilité des runners et l'évolution du quota. Le transfert OVH, son
bootstrap Python et les clés de cache froides peuvent réduire les gains estimés.
La branche ne dispose pas de l'environnement build : ces mesures après changement
restent à réaliser par le propriétaire.

## Packaging release : run chaud du 9 novembre, `5e4e49b`

Source : [run 42696335380](https://github.com/Pierre-ZACHARY/bevy-parkour2/actions/runs/42696335380),
API des jobs et artifacts `builds-*` (`summary.json`, `summary.md`, `logs/`).
Tous les jobs sont verts. Durée murale approximative : 56 minutes.

| Job build | Total | Action Nix | Pull Crane OVH | Build Crane | Publication | Commande xtask |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| macOS ARM64 | 55:50 | 7:17 | 8:47 | 0:03 | 0:06 | 39:06 |
| Linux x64 + WebGPU | 21:52 | 6:31 | 3:36 | 0:01 | 0:05 | 11:19 |
| Linux ARM64 | 19:19 | 4:04 | 3:01 | 0:01 | 0:05 | 11:16 |
| Windows x64 | 39:56 | — | — | — | — | 36:53 |

| Sous-étape xtask (artifacts) | macOS | Linux x64 | WebGPU | Linux ARM64 | Windows |
| --- | ---: | ---: | ---: | ---: | ---: |
| Cargo du jeu | 38:04,88 | 5:10,53 | 3:12,00 | 7:36,52 | 28:55,37 |
| Bindgen / wasm-opt | — | — | 0:12,77 / 1:56,90 | — | — |
| Cargo + export assets | 0:16,12 | 0:07,47 | 0:10,87 | 1:34,88 | 3:58,01 |

Sur macOS, le launcher prend 1,50 s et les trois signatures 2,30 s cumulées.
Il reste environ 41 s dans la commande globale (pilote xtask, outils, copies,
inventaires et ZIP non chronométrés auparavant). Les logs du jeu finissent par
`Finished release ... in 38m 04s` : les assets ne sont pas le goulot. Les logs
ne permettent pas de séparer précisément codegen, thin LTO et édition de liens.
`cargo build --timings` fournit désormais un HTML par cible dans
`artifacts/<run>/timings/`, téléversé avec les rapports. La durée du packaging
(inventaire, manifeste, ZIP et hash) remplace le zéro fictif ; l'étape `stage`
mesure les copies et la préparation du bundle, **incluant** les sous-étapes
launcher/signatures et la publication des symboles quand elles existent.
Il ne faut donc pas additionner `stage` avec ses sous-étapes.

### Pourquoi ne pas copier directement `game-unwrapped` ?

| Contrat | Crane | xtask release |
| --- | --- | --- |
| Features natives | `--no-default-features --features distribution` | identique sur Linux, `distribution,macos-vulkan` sur macOS |
| Profil natif | `release` | `release` (thin LTO, codegen-units=1) |
| Cible Cargo | native implicite | triple natif explicite, répertoire Cargo séparé |
| macOS minimum | 13.0 | 13.0 |
| Canal officiel | aucune configuration Sentry | feature `sentry`, DSN/release publics, debug=2, strip=none, symboles publiés |
| Runtime Nix | références et closure Nix | bundle xtask ; bibliothèques Nix possibles également |

`game` est un wrapper de lancement, pas le binaire à mettre dans une archive.
Même `game-unwrapped` ne respecte pas le contrat macOS Vulkan ni celui des tags.
Contrôle local en lecture seule du binaire Crane de `5e4e49b` déjà présent :
`otool -L` référence `/nix/store/jaixg82zxmqjbn2s6grpd31jq0yygzc1-libiconv-115.100.1/lib/libiconv.2.dylib`.
Le problème de closure est donc aussi concret sur macOS.
Sur Linux local, les features correspondent mais cela ne prouve pas la
portabilité : l'interpréteur ELF/RPATH et les bibliothèques doivent être audités
et embarqués ou réécrits, puis testés sans Nix. La mise en garde de NIX.md
s'applique déjà aux paquets xtask construits dans le shell Nix ; cette modification
ne résout pas cette limitation existante. Ajouter seulement `--prebuilt-binary`
permettrait de contourner ces garanties sans preuve de provenance/configuration.
Cette option n'est donc pas ajoutée.

Les sources locales recompilent après restauration du cache Cargo ; sccache ne
cache pas la crate finale de type binaire. Un taux de hits des requêtes cacheables
ne mesure pas ce travail. Le dernier relevé sccache du job macOS indique même
0 hit/0 miss et un appel `crate-type (bin)` : ne pas présenter « 100 % » comme
une mesure de couverture de l'ensemble de la compilation.

### Changement et gains attendus

Les trois plateformes Crane passent dans une matrice `crane` indépendante des
quatre jobs de packaging. Elle garde restauration, construction et publication
signée du jeu, des dépendances et du profil CI. Elle ne récupère ni assets privés
ni cache Cargo du checkout. `required` exige son succès ; elle peut être ignorée
uniquement en PR, comme `build`. La finalisation Sentry attend **build et crane**,
comme lorsque ces phases étaient dans le même job. La commande de packaging,
les profils, features, configurations Sentry, assets et signatures restent identiques.

Le packaging macOS évite ainsi 8:56 de travail préalable ; Linux x64 3:42,
Linux ARM64 3:07. Projection chaude sans attente supplémentaire de runners :
macOS **46:54**, Linux x64 **18:10**, ARM64 **16:12**. Les nouveaux jobs Crane
chauds devraient rester autour de 8–17 minutes avec leur préparation Nix ;
ils ne seraient plus limitants. Les tests Windows tooling duraient déjà 47:15 :
le workflow entier pourrait passer d'environ 56 à **47–48 minutes** (8–9 minutes,
14–16 %), sans additionner les gains Linux. Ce sont des projections, pas des
mesures après modification. Trois runners supplémentaires et trois préparations
de shell augmentent le coût et peuvent créer une file d'attente. Un miss Crane
ou un build froid peut encore devenir le chemin critique ; les changements de
Rust/xtask peuvent invalider la dérivation existante dès le premier run.

### Autres pistes, sans modifier les livrables

- Le pull macOS copie **865 chemins**, dont **856 `cargo-package-*`**.
  Première copie à 20:55:37, dernière copie démarrée (`game-deps`) à 21:00:09,
  retour des hits à 21:04:09. Les quatre dernières minutes comprennent donc
  notamment le gros transfert/import final, sans permettre d'isoler son débit.
  Ne tirer que `game` suffirait à exécuter un jeu Nix déjà en cache, mais ici
  `game-deps` reste nécessaire au contrat de construction/publication Crane.
  La séparation le retire entièrement du runner de packaging.
- Le helper utilise déjà un seul `nix copy` pour toutes les closures. Le Nix
  local annonce `max-substitution-jobs=16` et `http-connections=25` ; les petits
  chemins paraissant séquentiels ne prouvent pas qu'augmenter ces valeurs
  accélérera les imports. Mesurer débit réseau, décompression et écritures store
  avant de changer ce parallélisme ; aucun réglage global n'est modifié.
- Sur Windows l'export prend 3:58, dont **3:55 de compilation** d'asset-export.
  Garder son cache Cargo devient plus utile que paralléliser la conversion des
  assets. Son éviction GitHub et le coût de liaison du jeu restent à traiter.
  Ajouter un gros cache complet concurrent augmenterait la pression sur le quota.
- Supprimer thin LTO ou augmenter codegen-units pour `local` changerait le binaire
  livré et ses performances. Ce serait un profil rapide explicite à évaluer
  séparément (`--profile ci` existe déjà), pas une substitution silencieuse du
  profil release. Les tags conservent toutes leurs garanties actuelles.

Un run réel doit confirmer les durées, la disponibilité des runners, les hits
Crane avec les nouvelles sources, la publication des trois closures, le gate
après échec/annulation/PR et l'attente de Crane avant finalisation Sentry. Les
HTML Cargo permettront d'attribuer le coût à la crate finale ; ils ne séparent
pas toutes les passes internes LLVM. Aucun build release local du jeu n'est
nécessaire pour ces validations de workflow et de rapports.

Validation locale de ce changement : `nix flake check --all-systems --no-build`
réussi ; `actionlint .github/workflows/ci.yml` réussi et contrôle de tous les
workflows limité au SC2016 préexistant de nix-dev.yml ; `cargo test --locked -p
xtask --profile ci --jobs 2` : 33 réussis, 5 tests d'intégration ignorés ;
9 tests Python du helper cache réussis. Expression du gate GitHub comparée à
la politique attendue sur 46 875 combinaisons d'événement/statuts, y compris
résultat absent. Le chemin du HTML `--timings` a été vérifié avec un build de
xtask uniquement, sans compiler le jeu en release.
`cargo xtask lint --jobs 2` est également réussi sur les cinq suites (jeu,
xtask, graph, asset-export et noodle : formatage et Clippy). Les assets privés
ont uniquement été lus via les deux liens symboliques ignorés autorisés dans
ce worktree. Aucun push Git, déclenchement de workflow, changement d'infrastructure,
publication OVH, GC Nix, changement de configuration globale ou écriture dans
le clone principal n'a été effectué.
