
{
  description = "Rust Devshell";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-25.11";
  };

  outputs = { self, nixpkgs, ... }:
    let
      supportedSystems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forEachSystem = f: nixpkgs.lib.genAttrs supportedSystems (system: f {
        pkgs = import nixpkgs { inherit system; };
      });
    in
    {
      devShells = forEachSystem ({ pkgs }: {
        default = pkgs.mkShell {
          nativeBuildInputs = [
            pkgs.cargo
            pkgs.rustc
            pkgs.pkg-config
            pkgs.rust-analyzer
            pkgs.clippy
            pkgs.rustfmt
            pkgs.taplo
          ];

          buildInputs = [
          ];

          shellHook = ''
          '';
        };
      });
    };
}
