{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    systems.url = "github:nix-systems/default-linux";
    flake-utils = {
      url = "github:numtide/flake-utils";
      inputs.systems.follows = "systems";
    };
  };
  outputs =
    {
      nixpkgs,
      flake-utils,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        name = "tailscale-vips-loopback";
        version = "0.1.0";

        hashes = {
          "x86_64-linux" = "04cd6d1271fa248b33cf12882cd3b8faa9b5e52dba32a04785ded4788251b78e";
          "aarch64-linux" = "ca06617c259f0f1f55769bc38964a71e3015ea4d45f1588cc9aec9a81fa7d55e";
        };
      in
      {
        packages.default = nixpkgs.stdenv.mkDerivation {
          inherit version name;

          src = nixpkgs.fetchurl {
            url = "https://github.com/nebulis-proxmox/tailscale-vips-loopback/releases/download/${version}/${name}-${system}";
            sha256 = hashes.${system};
          };

          phases = [
            "installPhase"
            "patchPhase"
          ];

          installPhase = ''
            mkdir -p $out/bin
            cp $src $out/bin/${name}
            chmod +x $out/bin/${name}
          '';
        };
      }
    );
}
