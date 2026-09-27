{
  inputs,
  sourceRoot,
}:
let
  inherit (inputs) nixpkgs crane rust-overlay;
  version = (builtins.fromTOML (builtins.readFile (sourceRoot + "/Cargo.toml"))).package.version;
  systems = [
    "x86_64-linux"
    "aarch64-linux"
  ];
  forAllSystems = f: nixpkgs.lib.genAttrs systems f;
  overlay = import rust-overlay;

  mkCraneLib =
    pkgs: target:
    (crane.mkLib pkgs).overrideToolchain (
      p:
      let
        base = p.rust-bin.fromRustupToolchainFile (sourceRoot + "/rust-toolchain.toml");
      in
      if target == null then base else base.override { targets = [ target ]; }
    );

  native = forAllSystems (
    system:
    import ./native.nix {
      pkgs = import nixpkgs {
        inherit system;
        overlays = [ overlay ];
      };
      inherit sourceRoot version mkCraneLib;
    }
  );

  cross = import ./cross.nix {
    inherit
      nixpkgs
      overlay
      sourceRoot
      version
      mkCraneLib
      ;
  };
  releases = forAllSystems (
    system:
    nixpkgs.lib.optionalAttrs (system == "x86_64-linux") (
      import ./release.nix {
        inherit system nixpkgs version;
        inherit (cross) mkTarget mkCross minimalReleaseCargoArgs;
        inherit (native.${system}) pkgs;
      }
    )
  );
  development = forAllSystems (system: import ./dev.nix { native = native.${system}; });
in
{
  packages = forAllSystems (
    system:
    {
      default = native.${system}.package;
      aw = native.${system}.package;
    }
    // releases.${system}
  );
  apps = forAllSystems (system: {
    default = {
      type = "app";
      program = "${native.${system}.package}/bin/aw";
    };
    aw = {
      type = "app";
      program = "${native.${system}.package}/bin/aw";
    };
  });
  devShells = forAllSystems (system: {
    default = development.${system}.shell;
  });
  formatter = forAllSystems (system: development.${system}.formatter);
}
