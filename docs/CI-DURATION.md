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

## Enquête Windows du 9 novembre : éviction et clés Rust

Comparaison en lecture seule des jobs de
[42645303778](https://github.com/Pierre-ZACHARY/bevy-parkour2/actions/runs/42645303778)
et [42696335380](https://github.com/Pierre-ZACHARY/bevy-parkour2/actions/runs/42696335380),
tous deux sur **5e4e49b87b82ab52c18822e5958a1c065a453f48**. Les logs bruts
ont été conservés localement dans `/tmp/sccache-study`, pas dans Git.

| Job Windows, sans cache Cargo | Hits Rust | Misses Rust | Hits C/C++ / misses | Appels non cachables |
| --- | ---: | ---: | ---: | ---: |
| game (129429064939) | 458 | 75 | 52 / 0 | 134 |
| tooling (129429064888) | 592 | 133 | 32 / 0 | 190 |
| build (129429064871) | 623 | 97 | 52 / 0 | 156 |

Chaque job indique `No cache found`, sccache 0.19.0, rustc 1.97.1
(8bab26f4f), zéro erreur de lecture S3 et les mêmes bases Windows
`d:/a/bevy-parkour2/bevy-parkour2/`, `c:/users/runneradmin/.cargo/`.
Les C/C++ servis par cc-rs ne sont donc pas la source des misses observés.
Les appels non cachables sont comptés séparément : respectivement 46/56/62
proc-macros et 62/83/79 binaires, dont les build scripts. Leur coût subsiste
même avec un cache OVH entièrement chaud.

Avec le cache Cargo restauré, les mêmes groupes n'exécutent que 12/3/10
requêtes cachables (toutes hits), contre 585/757/772 sans restauration.
Comparer ces pourcentages ne permet pas d'identifier les crates qui manquent
lors d'une reconstruction complète. Les logs actuels n'associent pas les
75/133/97 misses à des crates ou à des clés. **La cause précise de ces misses
Windows reste à établir**, sans normalisation spéculative des variables.

Le code upstream fournit des pistes vérifiables :
[sccache Rust 0.19](https://github.com/mozilla/sccache/blob/v0.19.0/src/compiler/rust.rs)
hache les fichiers sources, le contenu des `--extern` (donc aussi les DLL de
proc-macros), les bibliothèques natives, les arguments filtrés, les dépendances
d'environnement rustc, les variables `CARGO_*` sauf exceptions explicites, le
cwd et le compilateur. Une DLL MSVC régénérée avec un contenu différent peut
invalider les crates consommatrices, puis leurs dépendants : **hypothèse**, pas
preuve de timestamps/PDB/GUID non déterministes. Un build script générant un
contenu différent produit aussi ce résultat. Changer seulement les bases ne
corrige pas ces différences.

### Diagnostic au prochain run Windows

Les jobs tests/build activent des modules debug sccache ciblés. Les artifacts
existants contiennent `.compiler-cache/diagnostics/` pendant sept jours :

- `sccache.log` : uniquement nom d'unité, clé et hit/miss/résultat ; le log debug
  brut n'est jamais téléversé. Pas de trace HTTP/S3 ni de dump d'environnement.
- `inputs-<pid>.txt` : compilateur, cwd, arguments, empreintes des valeurs
  `CARGO_*`/`OUT_DIR` et du contenu de chaque `--extern`. Les variables de registre
  Cargo sont exclues. Ces empreintes Rust servent à comparer deux exécutions
  de la même toolchain, pas à authentifier des fichiers.
- `generated-inputs.txt` : SHA256 des sources générées, archives natives et
  sorties de build scripts sous `target/**/build/`, sans leur contenu.

Comparer **deux reconstructions sans target préservé**, même commit/toolchain/
profil/features ; une CI restaurée par Cargo ne repasse pas dans le compilateur
et ne produit pas la même couverture diagnostique. Aucun cache n'est effacé
automatiquement. Depuis `sccache.log`, chercher les unités en miss, retrouver
leur `--crate-name` et `-C extra-filename` dans les fichiers inputs, puis :

1. Comparer compilateur/cwd/arguments et les empreintes d'environnement par nom.
   Exclure `CARGO_MAKEFLAGS`, ignoré par sccache ; `CARGO_TARGET_DIR`, lui, compte.
2. Si ces entrées correspondent, chercher le premier `extern` différent ; une
   DLL de proc-macro différente oriente vers MSVC/édition de liens, un rlib vers
   ses propres entrées. Ne pas supprimer ces fichiers du calcul de clé.
3. Comparer les SHA256 sous `build/` pour les sources/archives générées. Si tout
   correspond, il reste les env-deps rustc et sources non générées ; reproduire
   seulement l'unité suspecte, puis ajouter une trace ciblée pour celle-ci.

L'instrumentation ajoute de la lecture disque des externs et des petits fichiers
par invocation. Son coût doit être mesuré puis elle doit être retirée après
identification. Les compilations contournant sccache pour la limite de longueur
Windows restent identifiables dans le log Cargo, sans clé sccache.

### Budget Cargo GitHub et options

L'API caches au moment de cette enquête retourne 11,73 Go décimaux : Windows
**6,15 Go** (game 1,897 ; tooling 2,215 ; build 2,033), Cargo Nix **5,59 Go**
(Linux x64 2,731 ; macOS ARM 2,856). C'est un instantané pendant les évictions,
pas une taille durable garantie. Le quota de 10 Go ne peut pas retenir cet
ensemble : dépassement d'environ **1,73 Go**, avant nouvelles clés/branches.

Le workflow limite désormais la sauvegarde Windows à `main`. Les PR/tags/autres
branches peuvent restaurer le cache de la branche par défaut, sans créer leur
propre copie. Gain sur les trois entrées `main` : **0 Go** ; jusqu'à **6,15 Go
par copie complète de branche évitée**, aux tailles observées. Cela ne suffit
pas à résoudre le dépassement de l'inventaire `main`.

Options à mesurer après le diagnostic, sans suppression effectuée ici :

- Réserver les caches target GitHub à Windows, et ne conserver que les sources/
  outils Cargo sur les Nix writers : au plus 5,59 Go de renouvellement évité,
  moins la taille des sources/outils conservés. Budget Windows restant 6,15 Go,
  donc environ 3,85 Go pour ces sources et le reste. Linux/macOS ont déjà des
  hits sccache complets ; proc-macros/liens restent néanmoins à reconstruire.
- Un cache commun aux deux groupes de tests Windows remplace 4,11 Go par une
  union de dépendances. Borne théorique du gain : **0 à 1,90 Go**, selon leur
  intersection et la compression, à mesurer. Un simple `shared-key` ne construit
  pas l'union : les caches sont immuables et seul le premier writer gagne.
  Il faut un producteur compilant les deux groupes et deux consommateurs ;
  cela ajoute une dépendance et peut rallonger le chemin critique. Les profils
  release/ci et le CRT statique du build justifient de garder build séparé.
- `cache-targets: false` sur Windows économiserait une partie des 6,15 Go mais
  remettrait précisément les builds à 33–47 minutes aujourd'hui. Ne pas
  l'activer avant d'avoir expliqué et corrigé les misses Rust.

Aucun gain de compilation n'est attribué à l'instrumentation elle-même. Le
budget à récupérer, si une correction rend les reconstructions indépendantes
de l'éviction, est l'écart observé de **20 à 43 minutes par job** ; les appels
non cachables et les liens empêchent de promettre les 4–13 minutes restaurées.
