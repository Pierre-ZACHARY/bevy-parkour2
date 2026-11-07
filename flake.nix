{
  description = "Bevy Parkour 2: Linux development and CI with Crane";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    crane.url = "github:ipetkov/crane";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      crane,
      rust-overlay,
    }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      perSystem = nixpkgs.lib.genAttrs systems (
        system:
        import ./nix/project.nix {
          inherit
            self
            nixpkgs
            crane
            rust-overlay
            system
            ;
        }
      );
    in
    {
      packages = nixpkgs.lib.mapAttrs (_: value: value.packages) perSystem;
      devShells = nixpkgs.lib.mapAttrs (_: value: value.devShells) perSystem;
      checks = nixpkgs.lib.mapAttrs (_: value: value.checks) perSystem;
      formatter = nixpkgs.lib.genAttrs systems (system: nixpkgs.legacyPackages.${system}.nixfmt);
    };
}
