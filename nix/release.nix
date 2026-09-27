# Portable cross-built release artifacts and the aggregate release graph.
{
  pkgs,
  system,
  nixpkgs,
  version,
  mkTarget,
  mkCross,
  minimalReleaseCargoArgs,
}:
let
  mkDist =
    {
      package,
      sourceName,
      assetName,
    }:
    pkgs.runCommand "${assetName}-dist"
      {
        nativeBuildInputs = [ pkgs.coreutils ];
      }
      ''
        mkdir -p "$out"
        cp "${package}/bin/${sourceName}" "$out/${assetName}"
        chmod 0755 "$out/${assetName}"
        (
          cd "$out"
          sha256sum "${assetName}" > "${assetName}.sha256"
        )
        printf '%s\n' '${version}' > "$out/VERSION"
      '';

  # Backport only the MinGW adapter selection from upstream jitterentropy:
  # https://github.com/smuellerDD/jitterentropy-library/commit/eefc37ac7ed3b6b455115f8a8d5246a585ac38f9
  # Keep CPU jitter and the pinned crypto implementation enabled. Exact-version
  # scope and --replace-fail make source drift explicit instead of faking MSVC.
  windowsVendorPackage =
    package: drv:
    if package.name == "aws-lc-sys" && package.version == "0.45.0" then
      drv.overrideAttrs (old: {
        postPatch = (old.postPatch or "") + ''
          substituteInPlace \
            aws-lc/third_party/jitterentropy/jitterentropy-library/jitterentropy.h \
            aws-lc/third_party/jitterentropy/jitterentropy-library/arch/jitterentropy-base-windows.h \
            --replace-fail '#if defined(_MSC_VER)' '#if defined(_MSC_VER) || defined(__MINGW32__)'
        '';
      })
    else
      drv;

  linuxX86_64 = mkCross {
    localSystem = system;
    crossSystem = {
      config = "x86_64-unknown-linux-musl";
      libc = "musl";
    };
    target = "x86_64-unknown-linux-musl";
    rustFlags = "-C target-feature=+crt-static";
  };

  linuxAarch64 = mkCross {
    localSystem = system;
    crossSystem = {
      config = "aarch64-unknown-linux-musl";
      libc = "musl";
    };
    target = "aarch64-unknown-linux-musl";
    rustFlags = "-C target-feature=+crt-static";
  };

  # Rust's Windows GNU target gives us a self-contained PE binary.
  windowsX86_64 = mkCross {
    localSystem = system;
    crossSystem = {
      config = "x86_64-w64-mingw32";
      libc = "msvcrt";
    };
    target = "x86_64-pc-windows-gnu";
    cargoExtraArgs = minimalReleaseCargoArgs;
    overrideVendorCargoPackage = windowsVendorPackage;
  };

  # ARM64 Windows has no Rust GNU target. Match dotlink's reproducible
  # LLVM-MinGW/UCRT + gnullvm route.
  llvmMingwBase = pkgs.callPackage (nixpkgs + "/pkgs/applications/emulators/wine/llvm-mingw.nix") { };
  llvmMingw = llvmMingwBase.overrideAttrs (old: {
    buildInputs = (old.buildInputs or [ ]) ++ [
      pkgs.zstd
      pkgs.libxml2_13
      pkgs.xz
      pkgs.ncurses
    ];
  });
  windowsAarch64 = mkTarget {
    targetPkgs = pkgs;
    target = "aarch64-pc-windows-gnullvm";
    cargoExtraArgs = minimalReleaseCargoArgs;
    # Rust's gnullvm target otherwise imports an unshipped libunwind.dll.
    # Keep unwinding semantics while linking the toolchain's static unwinder.
    rustFlags = "-C target-feature=+crt-static";
    nativeBuildInputs = [ llvmMingw ];
    overrideVendorCargoPackage = windowsVendorPackage;
    extraArgs = {
      CARGO_TARGET_AARCH64_PC_WINDOWS_GNULLVM_LINKER = "${llvmMingw}/bin/aarch64-w64-mingw32-clang";
      CC_aarch64_pc_windows_gnullvm = "${llvmMingw}/bin/aarch64-w64-mingw32-clang";
      CXX_aarch64_pc_windows_gnullvm = "${llvmMingw}/bin/aarch64-w64-mingw32-clang++";
      AR_aarch64_pc_windows_gnullvm = "${llvmMingw}/bin/aarch64-w64-mingw32-ar";
      RANLIB_aarch64_pc_windows_gnullvm = "${llvmMingw}/bin/aarch64-w64-mingw32-ranlib";
    };
  };

  # Cross-build Apple Silicon from Linux with the same pinned SDK/LLVM
  # Mach-O approach already proven in ../dotlink.
  fetchMacosSdk = pkgs.callPackage (nixpkgs + "/pkgs/by-name/ap/apple-sdk/common/fetch-sdk.nix") { };
  macosSdk = fetchMacosSdk {
    urls = [
      "https://swcdn.apple.com/content/downloads/14/48/052-59890-A_I0F5YGAY0Y/p9n40hio7892gou31o1v031ng6fnm9sb3c/CLTools_macOSNMOS_SDK.pkg"
      "https://web.archive.org/web/20250211001355/https://swcdn.apple.com/content/downloads/14/48/052-59890-A_I0F5YGAY0Y/p9n40hio7892gou31o1v031ng6fnm9sb3c/CLTools_macOSNMOS_SDK.pkg"
    ];
    version = "14.4";
    hash = "sha256-QozDiwY0Czc0g45vPD7G4v4Ra+3DujCJbSads3fJjjM=";
  };
  macosCc = pkgs.writeShellScript "aw-aarch64-apple-darwin-cc" ''
    export PATH="${pkgs.lib.makeBinPath [ pkgs.llvmPackages.lld ]}:$PATH"
    exec ${pkgs.llvmPackages.clang-unwrapped}/bin/clang \
      --target=aarch64-apple-darwin \
      -isysroot ${macosSdk} \
      -mmacosx-version-min=11.0 \
      -fuse-ld=lld \
      "$@"
  '';
  macosCxx = pkgs.writeShellScript "aw-aarch64-apple-darwin-cxx" ''
    export PATH="${pkgs.lib.makeBinPath [ pkgs.llvmPackages.lld ]}:$PATH"
    exec ${pkgs.llvmPackages.clang-unwrapped}/bin/clang++ \
      --target=aarch64-apple-darwin \
      -isysroot ${macosSdk} \
      -mmacosx-version-min=11.0 \
      -fuse-ld=lld \
      "$@"
  '';
  macosAarch64Build = mkTarget {
    targetPkgs = pkgs;
    target = "aarch64-apple-darwin";
    # Omit build-path STABS before LLD hashes the UUID. Rust strips these
    # symbols later, after their otherwise variable UUID has been retained.
    rustFlags = "-C link-arg=-Wl,-S";
    nativeBuildInputs = [
      pkgs.llvmPackages.clang-unwrapped
      pkgs.llvmPackages.lld
      pkgs.llvmPackages.llvm
    ];
    extraArgs = {
      CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER = macosCc;
      CC_aarch64_apple_darwin = macosCc;
      CXX_aarch64_apple_darwin = macosCxx;
      AR_aarch64_apple_darwin = "${pkgs.llvmPackages.llvm}/bin/llvm-ar";
      RANLIB_aarch64_apple_darwin = "${pkgs.llvmPackages.llvm}/bin/llvm-ranlib";
      MACOSX_DEPLOYMENT_TARGET = "11.0";
      SDKROOT = macosSdk;
    };
  };
  macosAarch64 = macosAarch64Build // {
    package = macosAarch64Build.package.overrideAttrs (old: {
      # Crane scrubs store references after Rust signs the executable. Refresh
      # the ad-hoc code-page hashes after all stripping/reference mutations.
      postFixup = (old.postFixup or "") + ''
        rustHost="$(rustc -vV | sed -n 's/^host: //p')"
        objcopy="$(rustc --print sysroot)/lib/rustlib/$rustHost/bin/rust-objcopy"
        test -x "$objcopy"
        "$objcopy" "$out/bin/aw"
      '';
    });
  };

  linuxX86_64Dist = mkDist {
    package = linuxX86_64.package;
    sourceName = "aw";
    assetName = "aw-linux-x86_64";
  };
  linuxAarch64Dist = mkDist {
    package = linuxAarch64.package;
    sourceName = "aw";
    assetName = "aw-linux-aarch64";
  };
  windowsX86_64Dist = mkDist {
    package = windowsX86_64.package;
    sourceName = "aw.exe";
    assetName = "aw-windows-x86_64.exe";
  };
  windowsAarch64Dist = mkDist {
    package = windowsAarch64.package;
    sourceName = "aw.exe";
    assetName = "aw-windows-aarch64.exe";
  };
  macosAarch64Dist = mkDist {
    package = macosAarch64.package;
    sourceName = "aw";
    assetName = "aw-macos-aarch64";
  };

  releaseAll =
    pkgs.runCommand "aw-release-all"
      {
        nativeBuildInputs = [
          pkgs.coreutils
          pkgs.python3
        ];
      }
      ''
        mkdir -p "$out"
        cp "${linuxX86_64Dist}"/aw-linux-x86_64* "$out/"
        cp "${linuxAarch64Dist}"/aw-linux-aarch64* "$out/"
        cp "${windowsX86_64Dist}"/aw-windows-x86_64.exe* "$out/"
        cp "${windowsAarch64Dist}"/aw-windows-aarch64.exe* "$out/"
        cp "${macosAarch64Dist}"/aw-macos-aarch64* "$out/"
        printf '%s\n' '${version}' > "$out/VERSION"
        cat > "$out/PLATFORMS.txt" <<'EOF'
        aw-linux-x86_64: Linux x86_64 (static musl, full feature build)
        aw-linux-aarch64: Linux ARM64 (static musl, full feature build)
        aw-windows-x86_64.exe: Windows x86_64 (minimal cross-platform build: inline Ratatui chat, provider config/model flags and stateless execution; database, SecretStore/OIDC, web, fullscreen workspace and distributed roles are compile-time omitted)
        aw-windows-aarch64.exe: Windows ARM64 (same minimal cross-platform build and fail-closed feature boundary)
        aw-macos-aarch64: macOS ARM64 / Apple Silicon (full Cargo feature build; Linux-only same-UID/device/team/file-tool authorities remain fail-closed)
        EOF
        ${pkgs.python3}/bin/python3 - "$out" '${version}' <<'PY'
        import hashlib, json, pathlib, sys
        root = pathlib.Path(sys.argv[1])
        version = sys.argv[2]
        matrix = [
            ("aw-linux-x86_64", "x86_64-unknown-linux-musl", "full"),
            ("aw-linux-aarch64", "aarch64-unknown-linux-musl", "full"),
            ("aw-windows-x86_64.exe", "x86_64-pc-windows-gnu", "minimal"),
            ("aw-windows-aarch64.exe", "aarch64-pc-windows-gnullvm", "minimal"),
            ("aw-macos-aarch64", "aarch64-apple-darwin", "full"),
        ]
        artifacts = []
        for asset, target, profile in matrix:
            data = (root / asset).read_bytes()
            artifacts.append({
                "asset": asset,
                "target": target,
                "feature_profile": profile,
                "sha256": hashlib.sha256(data).hexdigest(),
                "size_bytes": len(data),
            })
        manifest = {
            "schema_version": 1,
            "version": version,
            "artifacts": artifacts,
        }
        (root / "release-manifest.json").write_text(
            json.dumps(manifest, indent=2, separators=(",", ": ")) + "\n"
        )
        PY
      '';
in
{
  cross-linux-x86_64-deps = linuxX86_64.cargoArtifacts;
  cross-linux-x86_64 = linuxX86_64.package;
  dist-linux-x86_64 = linuxX86_64Dist;

  cross-linux-aarch64-deps = linuxAarch64.cargoArtifacts;
  cross-linux-aarch64 = linuxAarch64.package;
  dist-linux-aarch64 = linuxAarch64Dist;

  cross-windows-x86_64-deps = windowsX86_64.cargoArtifacts;
  cross-windows-x86_64 = windowsX86_64.package;
  dist-windows-x86_64 = windowsX86_64Dist;

  cross-windows-aarch64-deps = windowsAarch64.cargoArtifacts;
  cross-windows-aarch64 = windowsAarch64.package;
  dist-windows-aarch64 = windowsAarch64Dist;

  cross-macos-aarch64-deps = macosAarch64.cargoArtifacts;
  cross-macos-aarch64 = macosAarch64.package;
  dist-macos-aarch64 = macosAarch64Dist;

  release-all = releaseAll;
}
