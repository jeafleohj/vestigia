{
  description = "Development environment for vestigia.nvim";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
    }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];

      forAllSystems = f: nixpkgs.lib.genAttrs systems f;

      cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
      cargoVersion = cargoToml.workspace.package.version;
      version = cargoVersion;
    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ rust-overlay.overlays.default ];
          };
          vestigia-nvim = pkgs.callPackage {
            inherit version;
            rustPlatform = pkgs.makeRustPlatform {
              cargo = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
              rustc = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
            };
          };
        in
        {
          vestigia-nvim = vestigia-nvim;
          default = vestigia-nvim;
        }
      );

      devShells = forAllSystems (
        system:
        let
          toolchainFile = builtins.fromTOML (builtins.readFile ./rust-toolchain.toml);

          toolchain = toolchainFile.toolchain;

          pkgs = import nixpkgs {
            inherit system;
            overlays = [ rust-overlay.overlays.default ];
          };

          rust = pkgs.rust-bin.fromRustupToolchain (
            toolchain
            // {
              components = pkgs.lib.unique (
                (toolchain.components or [ ])
                ++ [
                  "rust-src"
                  "rust-analyzer"
                ]
              );
            }
          );
        in
        {
          default = pkgs.mkShell {
            buildInputs = [
              rust
            ];
          };
        }
      );
    };
}
