let
  nixpkgs = builtins.fetchGit { url = "https://github.com/NixOS/nixpkgs.git"; rev = "384b898d18b0044165b23d19cb9a0b8982d82b11"; };
  rustOverlay = builtins.fetchGit { url = "https://github.com/oxalica/rust-overlay.git"; };
  pkgs = import nixpkgs { overlays = [ (import rustOverlay) ]; };
  rust-toolchain = pkgs.rust-bin.stable.latest.default.override {
    extensions = [ "llvm-tools-preview" ];
    targets = [ "thumbv7m-none-eabi" ];
  };
  python = pkgs.python3.withPackages (p: [ p.pyserial ]);

  dioxusCli = import ./dioxus-cli.nix;
in
pkgs.mkShell {
  nativeBuildInputs = with pkgs; [
    pkg-config
    udev
    rust-toolchain
    rust-analyzer
    flip-link
    probe-run
    cargo-generate
    cargo-binutils
    openocd
    gdb
    python
    potrace
    imagemagick
    webkitgtk
    gtk3
    libayatana-appindicator
    dioxusCli
    nodejs
    nodePackages.typescript-language-server
    nodePackages.svelte-language-server
  ];

  # FIXME: this is for cargo-tauri, installed with `cargo install`
  PATH = "$HOME/.cargo/bin:$PATH";
  XDG_DATA_DIRS = "${pkgs.gtk3}/share/gsettings-schemas/${pkgs.gtk3.name}";
}
