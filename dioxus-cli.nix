let
  nixpkgs = builtins.fetchGit { url = "https://github.com/NixOS/nixpkgs.git"; rev = "384b898d18b0044165b23d19cb9a0b8982d82b11"; };
  rustOverlay = builtins.fetchGit { url = "https://github.com/oxalica/rust-overlay.git"; };
  pkgs = import nixpkgs { overlays = [ (import rustOverlay) ]; };
  rust-toolchain = pkgs.rust-bin.stable.latest.default.override {
    extensions = [ "llvm-tools-preview" ];
    targets = [ "thumbv7m-none-eabi" ];
  };
in

pkgs.rustPlatform.buildRustPackage rec {
  pname = "dioxus-cli";
  version = "0.3";
  src = builtins.fetchGit {
    url = https://github.com/DioxusLabs/cli;
    rev = "93c765131238934f4ee421fbff9552a365f1ec84";
  };
  cargoHash = "sha256-StpmgC9p6xLAZhs1BHvU5L31y1d9n4uXbxanY8ItVQ0=";

  nativeBuildInputs = with pkgs; [
    pkg-config
    openssl
    rust-toolchain
  ];

  buildInputs = with pkgs; [
    openssl
  ];

  doCheck = false;
}
