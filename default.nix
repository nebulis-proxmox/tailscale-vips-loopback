{
  pkgs ? import <nixpkgs> { },
}:
pkgs.rustPlatform.buildRustPackage rec {
  pname = "tailscale-vips-loopback";
  version = "0.1.0";
  cargoLock = {
    lockFile = ./Cargo.lock;
    outputHashes = {
      "aya-0.13.2" = "sha256-8HAQWlQ1ZQyH6uolLny+B7J81FcBqWpjEsNbvyh3NjE=";
    };
  };
  src = pkgs.lib.cleanSource ./.;

  nativeBuildInputs = [
    pkgs.bpf-linker
  ];
}
