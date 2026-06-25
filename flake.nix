{
  description = "teemlab — evolutionary simulation engine (Bevy + Avian)";

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

      # `play`: builds the whole workshop (teemlab + record + headless) in the
      # chosen profile, then launches the windowed build. Needed because the
      # recording menu (P3) launches `record` as a subprocess next to the
      # executable: `cargo run --bin teemlab` would only compile `teemlab`, whereas
      # `cargo build` (without --bin) builds *all* the binaries. Lives in the dev
      # shell (inherits LD_LIBRARY_PATH + toolchain); no versioned .sh script.
      play = pkgs.writeShellScriptBin "play" ''
        profile=()
        forward=()
        for arg in "$@"; do
          case "$arg" in
            --release) profile=(--release) ;;
            *) forward+=("$arg") ;;
          esac
        done
        cargo build "''${profile[@]}" && exec cargo run "''${profile[@]}" --bin teemlab -- "''${forward[@]}"
      '';

      # `flame`: flamegraph of the headless sim (cargo-flamegraph + perf) — the
      # "where does runtime go" tool that guides optimization, complementing
      # `cargo bench` (which measures the gain). Uses the `profiling` profile
      # (optimized + debug symbols, Cargo.toml) and a long run for enough samples.
      # perf may need: sudo sysctl -w kernel.perf_event_paranoid=-1
      flame = pkgs.writeShellScriptBin "flame" ''
        scenario="''${1:-scenarios/evolution.ron}"
        export TEEMLAB_TICKS="''${TEEMLAB_TICKS:-20000}"
        exec cargo flamegraph --profile profiling -o outputs/flamegraph.svg \
          --bin headless -- "$scenario"
      '';
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
          # Video encoder of the headless recorder (P3, item 14): `record` pipes its
          # raw frames directly into `ffmpeg`'s stdin.
          ffmpeg
          # `play [--release] [scenario.ron]`: builds the workshop + launches the
          # windowed build (record included). Defined in the `let` above.
          play
          # Performance work: `cargo bench` (throughput A/B between versions) and
          # `flame` (flamegraph of the headless sim, defined in the `let` above).
          cargo-flamegraph
          perf
          flame
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
          echo "  play [--release] [scenario.ron]  # build the workshop + launch the windowed build (record included)"
          echo "  cargo bench                      # throughput benchmark (ticks/sec; A/B via --save-baseline/--baseline)"
          echo "  flame [scenario.ron]             # flamegraph of the headless sim (cargo-flamegraph + perf)"
        '';
      };
    };
}
