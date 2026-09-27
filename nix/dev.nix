# Development shell and Nix formatter for aw.
{ native }:
let
  inherit (native) pkgs rustToolchain;
  # Rust/full validation links and executes the no_std consumers. Keep their
  # target and matching linker in development, separate from release compilers.
  validationToolchain = rustToolchain.override (previous: {
    targets = pkgs.lib.unique ((previous.targets or [ ]) ++ [ "wasm32v1-none" ]);
  });
  wasmLinker = pkgs.runCommand "aw-validation-wasm-linker" { } ''
    # The generic driver also accepts rustc's explicit -flavor wasm arguments.
    linker=${validationToolchain}/lib/rustlib/${pkgs.stdenv.hostPlatform.rust.rustcTarget}/bin/rust-lld
    test -x "$linker"
    mkdir -p "$out/bin"
    ln -s "$linker" "$out/bin/wasm-ld"
  '';
in
{
  shell = pkgs.mkShell {
    packages = [
      validationToolchain
      wasmLinker
      pkgs.rust-analyzer
      pkgs.python3
      pkgs.nodejs
      pkgs.git
      pkgs.bash
      pkgs.tmux # Visible-screen validation must not rely on runner image tools.
    ];
    AW_GIT = "${pkgs.git}/bin/git";
    RUST_BACKTRACE = "1";
    AW_BASH = "${pkgs.bash}/bin/bash";
    # No network/login/server work occurs in a shell hook.
  };
  formatter = pkgs.nixfmt-rfc-style;
}
