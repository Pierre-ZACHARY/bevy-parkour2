{ pkgs, runtimeLibraries }:
let
  inherit (pkgs) lib;
  inherit (pkgs.stdenv.hostPlatform) isDarwin isLinux;
  libraryPathVariable = if isDarwin then "DYLD_LIBRARY_PATH" else "LD_LIBRARY_PATH";
in
pkgs.stdenv.mkDerivation {
  pname = "jackdaw-editor";
  version = "0.20.1-rc.1";
  src = pkgs.fetchurl {
    url = "https://github.com/jbuehler23/jackdaw/releases/download/v0.20.1-rc.1/jackdaw-${
      if isDarwin then "aarch64-apple-darwin.zip" else "x86_64-unknown-linux-gnu.tar.zst"
    }";
    sha256 =
      if isDarwin then
        "acd1c7cd4bab3d19dcf8d1759beb3f9da0b41689d82d935d45707be6ce819ae4"
      else
        "ab3c9c468b01ae5a59b4a9b6f39163c9ce6c96b1a01c8fa119e7cd0148663815";
  };
  nativeBuildInputs = [
    pkgs.makeWrapper
  ]
  ++ lib.optionals isLinux [
    pkgs.zstd
    pkgs.autoPatchelfHook
  ]
  ++ lib.optional isDarwin pkgs.unzip;
  buildInputs =
    runtimeLibraries
    ++ [
      pkgs.openssl
      pkgs.zlib
    ]
    ++ lib.optional isLinux pkgs.stdenv.cc.cc.lib;
  dontBuild = true;
  # Rust's SDK/proc-macro metadata must survive unchanged.
  dontStrip = true;
  installPhase = ''
    runHook preInstall
    mkdir -p "$out/share/jackdaw" "$out/bin"
    cp -a ./. "$out/share/jackdaw/"
    for executable in jackdaw jd jackdaw-rustc-wrapper; do
      makeWrapper "$out/share/jackdaw/$executable" "$out/bin/$executable" \
        --prefix ${libraryPathVariable} : "$out/share/jackdaw:$out/share/jackdaw/sdk/host-deps:${lib.makeLibraryPath runtimeLibraries}"
    done
    runHook postInstall
  '';
  preFixup = lib.optionalString isLinux ''
    addAutoPatchelfSearchPath "$out/share/jackdaw"
    addAutoPatchelfSearchPath "$out/share/jackdaw/sdk/host-deps"
  '';
  meta.mainProgram = "jackdaw";
}
