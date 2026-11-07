{
  self,
  nixpkgs,
  crane,
  rust-overlay,
  system,
}:
let
  pkgs = import nixpkgs {
    inherit system;
    overlays = [ rust-overlay.overlays.default ];
  };
  inherit (pkgs) lib;
  inherit (pkgs.stdenv.hostPlatform) isLinux isDarwin;
  libraryPathVariable = if isDarwin then "DYLD_FALLBACK_LIBRARY_PATH" else "LD_LIBRARY_PATH";
  # Keep the compiler derivation separate from formatting/lint frontends.
  # Both come from the same pinned rust-toolchain.toml; rustup is SDK fallback.
  lintToolchain = pkgs.rust-bin.fromRustupToolchainFile ../rust-toolchain.toml;
  toolchain = (pkgs.rust-bin.fromRustupToolchainFile ../rust-toolchain.toml).override {
    extensions = [
      "llvm-tools-preview"
      "rust-src"
      "rust-analyzer"
    ];
    targets = [ "wasm32-unknown-unknown" ];
  };
  craneLib = (crane.mkLib pkgs).overrideToolchain toolchain;
  wasmLlvm = pkgs.llvmPackages_22;
  hasEditor = builtins.elem system [
    "x86_64-linux"
    "aarch64-darwin"
  ];
  editorToolchain = pkgs.rust-bin.stable."1.99.0".minimal;
  nativeLibraries =
    with pkgs;
    lib.optionals isLinux [
      alsa-lib
      udev
      vulkan-loader
      libGL
      wayland
      libxkbcommon
      libx11
      libxcursor
      libxi
      libxrandr
      libxcb
      libxrender
      libxext
    ]
    ++ lib.optionals isDarwin [ libiconv ];
  editorLibraries = nativeLibraries ++ lib.optional isLinux pkgs.fontconfig;
  nativeTools = with pkgs; [
    pkg-config
    clang
    lld
    cmake
    makeWrapper
  ];

  # Runtime/licensed assets are read from the checkout, never published with
  # the public binary derivation. Keep non-Rust include_str!/shader inputs.
  src = lib.cleanSourceWith {
    src = self;
    filter =
      path: type:
      let
        rel = lib.removePrefix (toString self + "/") (toString path);
        top = builtins.head (lib.splitString "/" rel);
      in
      builtins.elem top [
        "src"
        "crates"
        "vendor"
        "examples"
        "tools"
      ]
      || builtins.elem rel [
        "Cargo.toml"
        "Cargo.lock"
      ];
  };
  common = {
    inherit src;
    cargoVendorDir = craneLib.vendorCargoDeps { cargoLock = ../Cargo.lock; };
    pname = "bevy-parkour2";
    version = "0.1.0";
    strictDeps = true;
    nativeBuildInputs = nativeTools;
    buildInputs = nativeLibraries;
    # Do not use the checkout's mutable compiler facade in a sandbox.
    CARGO_BUILD_RUSTC = "${toolchain}/bin/rustc";
    CARGO_BUILD_RUSTC_WRAPPER = "";
    RUSTC_WRAPPER = "";
    CARGO_INCREMENTAL = "0";
    cargoExtraArgs = "--locked -p bevy_parkour2 --bin bevy_parkour2 --no-default-features --features distribution";
    doCheck = false;
  }
  // lib.optionalAttrs isDarwin { MACOSX_DEPLOYMENT_TARGET = "13.0"; };
  localVendor = lib.cleanSourceWith {
    src = self;
    name = "parkour-local-vendor";
    filter =
      path: type:
      let
        rel = lib.removePrefix (toString self + "/") (toString path);
      in
      rel == "vendor" || lib.hasPrefix "vendor/" rel;
  };
  cargoArtifacts = craneLib.buildDepsOnly (
    common
    // {
      src = null;
      # Registry crates (e.g. jackdaw_avian_integration) import public symbols
      # from these [patch] crates. Empty dummy libraries cannot satisfy them.
      # Only vendor changes invalidate this input; game source changes do not.
      dummySrc = craneLib.mkDummySrc {
        inherit src;
        extraDummyScript = ''
          rm -rf "$out/vendor"
          cp -r ${localVendor}/vendor "$out/vendor"
        '';
      };
      cargoCheckCommand = "true";
    }
  );
  gameLauncher = pkgs.writeShellScript "parkour-nix-launcher" ''
    set -eu
    asset_root="''${BEVY_ASSET_ROOT:-$PWD}"
    if [ ! -f "$asset_root/assets/scene.bsn" ]; then
      echo 'Run from the project root with hydrated assets, or set BEVY_ASSET_ROOT.' >&2
      exit 1
    fi
    # Distribution builds resolve assets beside current_exe(), even when
    # BEVY_ASSET_ROOT is set. Keep licensed/mutable assets outside the store.
    runtime=$(${pkgs.coreutils}/bin/mktemp -d -t parkour-nix.XXXXXXXX)
    trap '${pkgs.coreutils}/bin/rm -rf -- "$runtime"' EXIT
    ${pkgs.coreutils}/bin/cp --reflink=auto "$PARKOUR_NIX_GAME_BINARY" "$runtime/bevy_parkour2"
    ${pkgs.coreutils}/bin/ln -s "$(${pkgs.coreutils}/bin/realpath "$asset_root/assets")" "$runtime/assets"
    "$runtime/bevy_parkour2" "$@"
  '';
  game = craneLib.buildPackage (
    common
    // {
      inherit cargoArtifacts;
      meta.mainProgram = "bevy_parkour2";
      postFixup = ''
        mkdir -p "$out/libexec"
        mv "$out/bin/bevy_parkour2" "$out/libexec/bevy_parkour2"
        makeWrapper ${gameLauncher} "$out/bin/bevy_parkour2" \
          --set PARKOUR_NIX_GAME_BINARY "$out/libexec/bevy_parkour2" \
          --prefix ${libraryPathVariable} : ${lib.makeLibraryPath editorLibraries}
      '';
    }
  );

  # Reuse the project's checked-in version/checksum pins, while giving ELF
  # tools a Nix interpreter and RPATH (upstream Linux downloads cannot run
  # unmodified on NixOS).
  host = pkgs.stdenv.hostPlatform.rust.rustcTarget;
  pins = builtins.fromJSON (builtins.readFile ../tools/xtask/tools.lock.json);
  pinnedTool =
    id:
    let
      toolHost = if id == "sccache" then lib.replaceStrings [ "-gnu" ] [ "-musl" ] host else host;
      pin = lib.findFirst (p: p.id == id && p.host == toolHost) (throw "No ${id} pin for ${host}") pins;
    in
    pkgs.stdenv.mkDerivation {
      pname = "parkour-${id}";
      version = pin.version;
      src = pkgs.fetchurl { inherit (pin) url sha256; };
      sourceRoot = ".";
      nativeBuildInputs = lib.optional isLinux pkgs.autoPatchelfHook;
      buildInputs = lib.optional isLinux pkgs.stdenv.cc.cc.lib ++ [
        pkgs.openssl
        pkgs.zlib
      ];
      dontBuild = true;
      dontStrip = isDarwin;
      installPhase = ''
        runHook preInstall
        mkdir -p "$out/bin" "$out/lib"
        file=$(find . -type f -name ${lib.escapeShellArg pin.file} -print -quit)
        test -n "$file"
        install -m755 "$file" "$out/bin/${pin.file}"
        find . -type f \( -name '*.so*' -o -name '*.dylib' \) -exec cp -P {} "$out/lib/" \;
        runHook postInstall
      '';
    };
  tools = pkgs.symlinkJoin {
    name = "parkour-xtask-tools";
    paths = map pinnedTool [
      "nextest"
      "llvm-cov"
      "wasm-bindgen"
      "wasm-opt"
    ];
  };
  sccache = pinnedTool "sccache";
  cachePython = pkgs.python3.withPackages (ps: [ ps.boto3 ]);
  # Nixpkgs stops at 0.44.2; the project scripts need 0.45.2+ (`sm secrets
  # export --format`). Official release archives, hashes from the GitHub release.
  infisical =
    let
      version = "0.45.4";
      asset =
        {
          aarch64-darwin = {
            name = "darwin_arm64";
            sha256 = "2c44c652b896d5d845abad46348b0b94af1b7be8846491a2501e92e2da43e257";
          };
          x86_64-linux = {
            name = "linux_amd64";
            sha256 = "107389f4c9a0cc48023a834bb2a5bf1280c0a0e45ea7a98d13807d4def21b0a5";
          };
          aarch64-linux = {
            name = "linux_arm64";
            sha256 = "fcfd91de70efdedc7f8c1b1d90a024b433bb185387fe32467d49625a3bf55379";
          };
        }
        .${system};
    in
    pkgs.stdenv.mkDerivation {
      pname = "infisical";
      inherit version;
      src = pkgs.fetchurl {
        url = "https://github.com/Infisical/cli/releases/download/v${version}/cli_${version}_${asset.name}.tar.gz";
        inherit (asset) sha256;
      };
      sourceRoot = ".";
      nativeBuildInputs = lib.optional isLinux pkgs.autoPatchelfHook;
      dontBuild = true;
      # Keep the release's Apple code signature intact.
      dontStrip = isDarwin;
      installPhase = ''
        runHook preInstall
        install -Dm755 infisical "$out/bin/infisical"
        runHook postInstall
      '';
    };
  # Usable before the dev shell exists (see .envrc): pulls its missing inputs
  # from the private OVH cache with the caller's Infisical login.
  nixCache = pkgs.writeShellApplication {
    name = "nix-cache";
    runtimeInputs = [
      cachePython
      infisical
    ];
    text = ''
      root=$(${pkgs.git}/bin/git rev-parse --show-toplevel)
      exec python3 -I "$root/tools/nix-cache.py" "$@"
    '';
  };

  cli =
    name: version: sha256: extra:
    craneLib.buildPackage (
      {
        pname = name;
        inherit version;
        src = pkgs.fetchurl {
          url = "https://static.crates.io/crates/${name}/${name}-${version}.crate";
          name = "${name}-${version}.tar.gz";
          inherit sha256;
        };
        # These immutable upstream releases are cached as a whole. Splitting their
        # dummy sources would require realizing the archive during evaluation.
        cargoArtifacts = null;
        # Upstream lockfiles copied verbatim from the hash-verified crate archives.
        # Supplying them here avoids import-from-derivation during flake evaluation.
        cargoVendorDir = craneLib.vendorCargoDeps {
          cargoLock = ./cli-locks + "/${name}.lock";
        };
        nativeBuildInputs = nativeTools;
        buildInputs = editorLibraries ++ [ pkgs.openssl ];
        strictDeps = true;
        doCheck = false;
        cargoExtraArgs = "--locked";
        postFixup = ''
          for executable in "$out"/bin/*; do
            wrapProgram "$executable" --prefix ${libraryPathVariable} : ${lib.makeLibraryPath editorLibraries}
          done
        '';
      }
      // extra
    );
  # Hashes are filled from the exact crates.io archives, not a moving latest.
  dioxus =
    cli "dioxus-cli" "0.7.2" "9d46c5c825c7fe8bd79038ecf45d06262ae571aba7e46eb2ee3cacb223bfa874"
      { };
  jackdaw = import ./jackdaw.nix {
    inherit pkgs;
    runtimeLibraries = editorLibraries;
  };
  jd =
    if hasEditor then
      pkgs.writeShellScriptBin "jd" ''
        exec ${jackdaw}/bin/jd "$@"
      ''
    else
      cli "jd" "0.20.1-rc.1" "d42aaaf185ee7851669360df468ca89edc4426e2ae3f102bb0cd3861a1be06aa" { };
  # Jackdaw pins its SDK compiler through RUSTUP_TOOLCHAIN. Real Nix cargo/rustc
  # ignore that variable, so dispatch the SDK requests without downloading Rust.
  editorDispatch = pkgs.symlinkJoin {
    name = "parkour-rust-dispatch";
    paths =
      map
        (
          program:
          pkgs.writeShellScriptBin program ''
            case "''${RUSTUP_TOOLCHAIN:-}" in
              1.99.0|1.99.0-${host}) exec ${editorToolchain}/bin/${program} "$@" ;;
              "") exec ${toolchain}/bin/${program} "$@" ;;
              *) exec ${pkgs.rustup}/bin/${program} "$@" ;;
            esac
          ''
        )
        [
          "cargo"
          "rustc"
        ];
  };

  shell =
    interactive:
    craneLib.devShell (
      {
        inputsFrom = [ game ];
        packages =
          with pkgs;
          [
            toolchain
            lintToolchain
            tools
            sccache
            git
            git-lfs
            curl
            cacert
            gnutar
            gzip
            unzip
            nodejs
            powershell
            jdk_headless
            rustup
            nixfmt
            actionlint
            cachePython
          ]
          ++ lib.optionals interactive [
            dioxus
            jd
            infisical
          ]
          ++ lib.optionals (interactive && hasEditor) [ jackdaw ];
        XTASK_TOOLS_DIR = "${tools}/bin";
        PARKOUR_NIX_RUSTC = "${toolchain}/bin/rustc";
        PARKOUR_NIX_SCCACHE = "${sccache}/bin/sccache";
        LLVM_COV = "${toolchain}/lib/rustlib/${host}/bin/llvm-cov";
        LLVM_PROFDATA = "${toolchain}/lib/rustlib/${host}/bin/llvm-profdata";
        LIBCLANG_PATH = "${wasmLlvm.libclang.lib}/lib";
        # Native cc-wrapper injects Linux hardening flags unsupported by wasm.
        # Keep that wrapper for native code, and use LLVM's raw multi-target tools here.
        CC_wasm32_unknown_unknown = "${wasmLlvm.clang.cc}/bin/clang";
        CXX_wasm32_unknown_unknown = "${wasmLlvm.clang.cc}/bin/clang++";
        AR_wasm32_unknown_unknown = "${wasmLlvm.llvm}/bin/llvm-ar";
        ${libraryPathVariable} = lib.makeLibraryPath editorLibraries;
        shellHook = ''
          export BEVY_ASSET_ROOT="$PWD"
          export JACKDAW_OPEN_PROJECT="$PWD"
          # Also runs for `nix develop -c`; no credentials are needed for setup.
          sh tools/setup_cache.sh || exit 1
        ''
        + lib.optionalString (interactive && hasEditor) ''
          export PATH="${editorDispatch}/bin:$PATH"
        '';
      }
      // lib.optionalAttrs (interactive && hasEditor) {
        PARKOUR_NIX_EDITOR_DIR = "${jackdaw}/share/jackdaw";
        PARKOUR_NIX_EDITOR_RUSTC = "${editorToolchain}/bin/rustc";
        PARKOUR_NIX_EDITOR_TOOLCHAIN = "1.99.0";
        PARKOUR_NIX_EDITOR_TARGET = host;
      }
    );
in
{
  packages = {
    default = game;
    inherit
      game
      tools
      dioxus
      jd
      ;
    game-deps = cargoArtifacts;
    nix-cache = nixCache;
  }
  // lib.optionalAttrs isLinux { mesa = pkgs.mesa; }
  // lib.optionalAttrs hasEditor { inherit jackdaw; };
  devShells = {
    default = shell true;
    ci = shell false;
  };
  checks = {
    inherit game;
    xtask = craneLib.cargoTest {
      inherit src;
      cargoArtifacts = null;
      pname = "parkour-xtask-tests";
      version = "0.1.0";
      cargoExtraArgs = "--locked -p xtask";
      CARGO_PROFILE = "ci";
      nativeBuildInputs = [ pkgs.git ];
      # Cache/download tests serve local fixtures on 127.0.0.1.
      __darwinAllowLocalNetworking = isDarwin;
    };
  };
}
