{
  inputs = {
    naersk.url = "github:nix-community/naersk/master";
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, utils, naersk }:
    utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        naersk-lib = pkgs.callPackage naersk { stdenv = pkgs.clangStdenv; };
        nativeBuildInputs = with pkgs; [pkg-config];
        buildInputs = with pkgs; [clangStdenv.cc.libc alsa-lib pipewire jack2];
        LIBCLANG_PATH = with pkgs; "${llvmPackages.libclang.lib}/lib";
      in
      {
        packages = rec {
		  interflow = naersk-lib.buildPackage {
		    pname = "interflow";
		    version = "0.1.0";
		    src = ./.;
		    inherit nativeBuildInputs buildInputs LIBCLANG_PATH;
		  };
		  default = interflow;
        };
        devShells.default = pkgs.clangStdenv.mkDerivation {
          name = "interflow-devshell";
          buildInputs = buildInputs ++ nativeBuildInputs;
          inherit LIBCLANG_PATH;
        };
      }
    );
}
