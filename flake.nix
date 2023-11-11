{
  description = "A rust project";
  inputs = {
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    utils.url = "github:numtide/flake-utils";
    cargo2nix = {
      url = "github:cargo2nix/cargo2nix/release-0.11.0";
      inputs.rust-overlay.follows = "rust-overlay";
    };
  };
  outputs = {
    self,
    nixpkgs,
    utils,
    rust-overlay,
    cargo2nix,
  }: let
    buildShell = pkgs: anyRustToolchain: pkgs.mkShell {buildInputs = with pkgs; [anyRustToolchain];};
    rustOverwrite = anyRustToolchain: anyRustToolchain.override {extensions = ["rust-src" "rust-analyzer-preview"];};
    buildRustPkgs = pkgs:
      pkgs.rustBuilder.makePackageSet {
        rustVersion = "1.73.0";
        packageFun = import ./Cargo.nix;
        workspaceSrc = ./.;
        ignoreLockHash = false;
      };
    buildForSystem = system: let
      overlays = [rust-overlay.overlays.default cargo2nix.overlays.default self.overlays.default];
      pkgs = import nixpkgs {inherit system overlays;};
    in {
      devShells = rec {
        default = nightly;
        stable = buildShell pkgs (rustOverwrite pkgs.rust-bin.stable.latest.default);
        nightly = buildShell pkgs (rustOverwrite pkgs.rust-bin.nightly.latest.default);
      };
      packages = {
        default = pkgs.map-sprite-packer;
      };
      apps = {
        default = {
          type = "app";
          program = "${self.apps.${system}.default}/bin/map-sprite-packer";
        };
      };
    };
  in
    (utils.lib.eachDefaultSystem buildForSystem)
    // {
      overlays.default = final: prev: {
        map-sprite-packer = ((buildRustPkgs prev).workspace.map-sprite-packer {}).bin;
      };
    };
}
