{
  description = "xmd: markdown notes where values have names, with a query CLI and language server";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      # `nix build`, `nix run . -- --help`, or add it to a NixOS configuration.
      packages = forAllSystems (pkgs: {
        default = pkgs.rustPlatform.buildRustPackage {
          pname = "xmd";
          version = "0.1.0";
          src = self;
          cargoLock.lockFile = ./Cargo.lock;
          # The end-to-end suite drives the binary and language server from a
          # temporary workspace; run it with `cargo test` in a checkout instead.
          doCheck = false;
          meta.mainProgram = "xmd";
        };
      });
    };
}
