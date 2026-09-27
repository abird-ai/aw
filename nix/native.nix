# Native (host-system) aw package, dependency artifacts and reusable checks.
{
  pkgs,
  sourceRoot,
  version,
  mkCraneLib,
}:
let
  inherit (pkgs) lib;
  rustToolchain = pkgs.rust-bin.fromRustupToolchainFile (sourceRoot + "/rust-toolchain.toml");
  craneLib = mkCraneLib pkgs null;
  inherit (import ./source.nix { inherit lib sourceRoot; })
    hasCargoLock
    requireLock
    source
    helperSource
    ;

  common = {
    src = source;
    pname = "aw";
    inherit version;
    strictDeps = true;
    AW_DEFAULT_BASH = "${pkgs.bash}/bin/bash";
    AW_DEFAULT_GIT = "${pkgs.git}/bin/git";
    AW_GIT = "${pkgs.git}/bin/git";
    AW_BASH = "${pkgs.bash}/bin/bash";
  };

  cargoArtifacts = requireLock (
    craneLib.buildDepsOnly (
      common
      // {
        cargoExtraArgs = "--locked --workspace";
      }
    )
  );

  package = requireLock (
    craneLib.buildPackage (
      common
      // {
        inherit cargoArtifacts;
        cargoExtraArgs = "--locked -p aw-cli --bin aw";
        doCheck = false; # Full workspace tests run in their own check below.
        meta = {
          description = "Rust chat CLI, optional native workspace and embedded loopback web UI";
          mainProgram = "aw";
          license = lib.licenses.mit;
          platforms = [
            "x86_64-linux"
            "aarch64-linux"
          ];
        };
      }
    )
  );
in
{
  inherit
    pkgs
    craneLib
    rustToolchain
    source
    helperSource
    hasCargoLock
    requireLock
    common
    cargoArtifacts
    package
    ;
}
