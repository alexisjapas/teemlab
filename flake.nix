{
  description = "teemlab — moteur de simulation évolutive (Bevy + Avian)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  };

  outputs = { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };

      # Libraries Bevy / wgpu / winit dlopen at *runtime*. They must be on
      # LD_LIBRARY_PATH because they are loaded dynamically, not linked.
      runtimeLibs = with pkgs; [
        vulkan-loader
        libxkbcommon
        wayland
        libx11
        libxcursor
        libxrandr
        libxi
        alsa-lib
        systemdLibs # libudev (gamepad/input enumeration)
      ];
    in
    {
      devShells.${system}.default = pkgs.mkShell {
        # Build-time tools + dev utilities.
        nativeBuildInputs = with pkgs; [
          rustc
          cargo
          clippy
          rustfmt
          rust-analyzer
          pkg-config
          # Vulkan/OpenGL HUD overlay: `mangohud cargo run --bin teemlab`
          # to watch FPS / frame times while tuning the simulation.
          mangohud
          # Encodeur vidéo de l'enregistreur headless (P3, item 14) : `record`
          # pipe ses frames brutes directement sur le stdin de `ffmpeg`.
          ffmpeg
        ];

        # Things pkg-config must find at build time (the wayland feature links
        # libwayland-client, so its .pc file must be discoverable).
        buildInputs = with pkgs; [
          alsa-lib
          systemdLibs
          vulkan-loader
          wayland
          libxkbcommon
        ];

        LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath runtimeLibs;

        shellHook = ''
          echo "teemlab dev shell — $(rustc --version)"
        '';
      };
    };
}
