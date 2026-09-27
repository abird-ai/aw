# Cross-compilation helpers for the aw release graph.
{
  nixpkgs,
  overlay,
  sourceRoot,
  version,
  mkCraneLib,
}:
let
  lib = nixpkgs.lib;
  inherit (import ./source.nix { inherit lib sourceRoot; }) source;

  releaseCargoArgs = "--locked -p aw-cli --bin aw --features full";
  minimalReleaseCargoArgs = "--locked -p aw-cli --bin aw --no-default-features --features minimal";

  mkTarget =
    {
      targetPkgs,
      target,
      cargoExtraArgs ? releaseCargoArgs,
      rustFlags ? null,
      nativeBuildInputs ? [ ],
      overrideVendorCargoPackage ? null,
      extraArgs ? { },
    }:
    let
      targetCraneLib = mkCraneLib targetPkgs target;
      targetCommon = {
        src = source;
        inherit cargoExtraArgs;
        pname = "aw";
        inherit version;
        strictDeps = true;
        doCheck = false;
        CARGO_BUILD_TARGET = target;
        # AWS-LC uses POSIX threads with GCC MinGW; Clang uses Win32 threads.
        buildInputs = lib.optional targetPkgs.stdenv.hostPlatform.isWindows targetPkgs.windows.pthreads;
      }
      // lib.optionalAttrs (overrideVendorCargoPackage != null) {
        cargoVendorDir = targetCraneLib.vendorCargoDeps {
          src = source;
          inherit overrideVendorCargoPackage;
        };
      }
      // lib.optionalAttrs (rustFlags != null) {
        CARGO_BUILD_RUSTFLAGS = rustFlags;
      }
      // extraArgs;
      targetCargoArtifacts = targetCraneLib.buildDepsOnly targetCommon;
      targetPackage = targetCraneLib.buildPackage (
        targetCommon
        // {
          cargoArtifacts = targetCargoArtifacts;
          inherit nativeBuildInputs;
          doCheck = false;
          meta = {
            description = "aw portable release binary";
            homepage = "https://github.com/abird-ai/aw";
            license = targetPkgs.lib.licenses.mit;
          };
        }
      );
    in
    {
      package = targetPackage;
      cargoArtifacts = targetCargoArtifacts;
    };

  mkCross =
    {
      localSystem,
      crossSystem,
      target,
      cargoExtraArgs ? releaseCargoArgs,
      rustFlags ? null,
      overrideVendorCargoPackage ? null,
    }:
    let
      targetPkgs = import nixpkgs {
        inherit localSystem crossSystem;
        overlays = [ overlay ];
      };
    in
    mkTarget {
      inherit
        targetPkgs
        target
        cargoExtraArgs
        rustFlags
        overrideVendorCargoPackage
        ;
    };
in
{
  inherit
    releaseCargoArgs
    minimalReleaseCargoArgs
    mkTarget
    mkCross
    ;
}
