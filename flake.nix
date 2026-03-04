{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    crane.url = "github:ipetkov/crane";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };
  outputs =
    {
      nixpkgs,
      crane,
      flake-utils,
      rust-overlay,
      ...
    }:
    let
      thenOrNull = condition: value: if condition then value else null;

      overrideBuildTargets = {
        "aarch64-darwin" = {
          target = "aarch64-unknown-linux-musl";
        };
      };
    in
    flake-utils.lib.eachDefaultSystem (
      localSystem:
      let
        crossTarget =
          thenOrNull (builtins.hasAttr localSystem overrideBuildTargets)
            overrideBuildTargets.${localSystem}.target;

        crossSystem = thenOrNull (crossTarget != null) {
          config = crossTarget;
        };

        pkgs = import nixpkgs {
          inherit crossSystem localSystem;
          overlays = [ (import rust-overlay) ];
        };

        # Correspond to rust 1.93.0 in the nightly channel as of 2024-06-05, which is the minimum version required to build aya 0.13.2
        rustToolchainFor =
          p:
          p.rust-bin.nightly."2025-12-05".default.override {
            extensions = [ "rust-src" ];
            targets = if (crossTarget != null) then [ crossTarget ] else [ ];
          };
        rustToolchain = rustToolchainFor pkgs;

        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchainFor;

        src = craneLib.cleanCargoSource ./.;

        crateExpression =
          {
            libiconv,
            lib,
            stdenv,
          }:
          craneLib.buildPackage {
            inherit src;

            pname = "tailscale-vips-loopback";
            version = "0.1.0";
            strictDeps = true;

            # Dependencies which need to be build for the current platform
            # on which we are doing the cross compilation. In this case,
            # pkg-config needs to run on the build platform so that the build
            # script can find the location of openssl. Note that we don't
            # need to specify the rustToolchain here since it was already
            # overridden above.
            nativeBuildInputs = [
              pkgs.bpf-linker
            ]
            ++ lib.optionals stdenv.buildPlatform.isDarwin [
              libiconv
            ];

            # runtime dependencies
            buildInputs = [
            ];

            cargoVendorDir = craneLib.vendorMultipleCargoDeps {
              inherit (craneLib.findCargoFiles src) cargoConfigs;
              cargoLockList = [
                ./Cargo.lock
                "${rustToolchain.passthru.availableComponents.rust-src}/lib/rustlib/src/rust/library/Cargo.lock"
              ];
            };

            CARGO_BUILD_TARGET = crossTarget;
            CARGO_BUILD_RUSTFLAGS = thenOrNull (
              crossTarget != null -> lib.strings.hasSuffix crossTarget "musl"
            ) "-C target-feature=+crt-static";
          };

        my-crate = pkgs.callPackage crateExpression { };
      in
      {
        checks = {
          inherit my-crate;
        };

        packages.default = my-crate;
      }
    );
}
