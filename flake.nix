{
  description = "DevShell for shanti";
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs =
    {
      nixpkgs,
      rust-overlay,
      flake-utils,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs { inherit system overlays; };
        # Configure your toolchain and include essential extensions
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" ];
        };
      in
      {
        devShells.default =
          with pkgs;
          mkShell rec {
            buildInputs = [
              pkg-config
              rust-bin.nightly.latest.default
              rustToolchain
              gcc
              libc
              libX11
              libXcursor
              libXrandr
              libXi
              libxcb
              libxkbcommon
              vulkan-loader
              wayland
            ];

          RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";

            shellHook = ''
              export LD_LIBRARY_PATH="$LD_LIBRARY_PATH:${builtins.toString (pkgs.lib.makeLibraryPath buildInputs)}";
            '';
          };
      }
    );
}
