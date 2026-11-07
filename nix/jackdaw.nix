{ pkgs, runtimeLibraries }:
pkgs.stdenv.mkDerivation {
  pname = "jackdaw-editor";
  version = "0.20.1-rc.1";
  src = pkgs.fetchurl {
    url = "https://github.com/jbuehler23/jackdaw/releases/download/v0.20.1-rc.1/jackdaw-x86_64-unknown-linux-gnu.tar.zst";
    sha256 = "ab3c9c468b01ae5a59b4a9b6f39163c9ce6c96b1a01c8fa119e7cd0148663815";
  };
  nativeBuildInputs = [
    pkgs.zstd
    pkgs.autoPatchelfHook
    pkgs.makeWrapper
  ];
  buildInputs = runtimeLibraries ++ [
    pkgs.stdenv.cc.cc.lib
    pkgs.openssl
    pkgs.zlib
  ];
  dontBuild = true;
  # Rust's SDK/proc-macro metadata must survive unchanged.
  dontStrip = true;
  installPhase = ''
    runHook preInstall
    mkdir -p "$out/share/jackdaw" "$out/bin"
    cp -a ./. "$out/share/jackdaw/"
    for executable in jackdaw jd jackdaw-rustc-wrapper; do
      makeWrapper "$out/share/jackdaw/$executable" "$out/bin/$executable" \
        --prefix LD_LIBRARY_PATH : "$out/share/jackdaw:${pkgs.lib.makeLibraryPath runtimeLibraries}"
    done
    runHook postInstall
  '';
  preFixup = ''
    addAutoPatchelfSearchPath "$out/share/jackdaw"
    addAutoPatchelfSearchPath "$out/share/jackdaw/sdk/host-deps"
  '';
  meta.mainProgram = "jackdaw";
}
