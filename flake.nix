{
  description = "aw Phase 14 — durable agent/workspace runtime with direct AwNet QUIC pairing, built with Crane";
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    crane.url = "github:ipetkov/crane/v0.24.0";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };
  outputs =
    inputs:
    import ./nix {
      inherit inputs;
      sourceRoot = ./.;
    };
}
