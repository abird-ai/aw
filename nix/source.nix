# Shared build-source filesets for aw.
#
# include_str! embeds the web assets, so cleanCargoSource alone would drop
# non-Rust inputs; these filesets keep the exact compile-time inputs while
# excluding historical design/images and local caches from every closure.
{
  lib,
  sourceRoot,
}:
let
  hasCargoLock = builtins.pathExists (sourceRoot + "/Cargo.lock");

  # Filter only the selected paths, never the whole checkout (target and
  # node_modules can be large). Prune cache directories as well as loose Python
  # bytecode before either the Cargo or helper closure is materialized.
  selectedFiles =
    paths:
    lib.fileset.unions (
      map (
        src:
        lib.fileset.fromSource (
          lib.cleanSourceWith {
            inherit src;
            filter =
              path: _type:
              let
                name = baseNameOf path;
              in
              name != "__pycache__" && !lib.hasSuffix ".pyc" name && !lib.hasSuffix ".pyo" name;
          }
        )
      ) paths
    );

  # Keep npm's local cache/node_modules out of all Nix source closures. The
  # committed dist is embedded by Rust; Solid sources + lockfile remain
  # available for explicit host/frontend rebuild checks.
  webFiles = selectedFiles [
    (sourceRoot + "/web/app.js")
    (sourceRoot + "/web/context-flow.js")
    (sourceRoot + "/web/index.html")
    (sourceRoot + "/web/model.js")
    (sourceRoot + "/web/package.json")
    (sourceRoot + "/web/package-lock.json")
    (sourceRoot + "/web/public")
    (sourceRoot + "/web/src")
    (sourceRoot + "/web/settings.js")
    (sourceRoot + "/web/style.css")
    (sourceRoot + "/web/vite.config.js")
    (sourceRoot + "/web/workspace.css")
    (sourceRoot + "/web/workspace.js")
    (sourceRoot + "/web/workspace-model.js")
    (sourceRoot + "/web/workspace-sidebars.js")
    (sourceRoot + "/web/dist")
  ];

  source = lib.fileset.toSource {
    root = sourceRoot;
    fileset = lib.fileset.union webFiles (
      selectedFiles (
        [
          (sourceRoot + "/Cargo.toml")
          (sourceRoot + "/rust-toolchain.toml")
          (sourceRoot + "/.cargo")
          (sourceRoot + "/src")
          (sourceRoot + "/crates")
          (sourceRoot + "/examples")
          (sourceRoot + "/tests")
        ]
        ++ lib.optional hasCargoLock (sourceRoot + "/Cargo.lock")
      )
    );
  };

  helperSource = lib.fileset.toSource {
    root = sourceRoot;
    fileset = lib.fileset.union webFiles (selectedFiles [
      (sourceRoot + "/scripts/build-release-artifacts.sh")
      (sourceRoot + "/scripts/bootstrap.sh")
      (sourceRoot + "/tests")
      (sourceRoot + "/crates/aw-store/migrations")
      (sourceRoot + "/crates/aw-workspace/fixtures")
      (sourceRoot + "/crates/aw-git/fixtures")
      (sourceRoot + "/crates/aw-store/src/journal.rs")
      (sourceRoot + "/crates/aw-store/src/lib.rs")
      (sourceRoot + "/crates/aw-inference/fixtures")
      (sourceRoot + "/crates/aw-identity/fixtures")
    ]);
  };

  requireLock =
    value:
    if hasCargoLock then
      value
    else
      throw ''
        Resolve a real Cargo.lock first:
          nix develop -c bash scripts/bootstrap.sh
        Then run `cargo test --workspace --locked`. Do not invent a lockfile.
      '';
in
{
  inherit
    hasCargoLock
    requireLock
    webFiles
    source
    helperSource
    ;
}
