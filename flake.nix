{
  description = "chip8-rs dev shell";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { nixpkgs, ... }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
      # minifb dlopen()s these at runtime (Wayland first, X11 as fallback),
      # so they must be findable through LD_LIBRARY_PATH, not just linked.
      runtimeLibs = with pkgs; [
        wayland
        libxkbcommon
        libx11
        libxcursor
        libxrandr
        libxi
      ];
    in
    {
      devShells.${system}.default = pkgs.mkShell {
        packages = with pkgs; [
          cargo
          rustc
          clippy
          rustfmt
          rust-analyzer
        ];
        LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath runtimeLibs;
      };
    };
}
