{
  description = "Nixos config flake";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
  };

  outputs =
    { self
    , nixpkgs
    }
      @inputs:
    let
      pkgs = nixpkgs.legacyPackages.x86_64-linux;
    in
    {
      devShells.x86_64-linux.default =
        pkgs.mkShell {
            LD_LIBRARY_PATH = "${pkgs.stdenv.cc.cc.lib}/lib";
	    buildInputs = [
            pkgs.sdcc
            pkgs.cc-tool
            #pkgs.stdenv.cc.cc.lib
            pkgs.picocom
          ];
          shellHook = ''
	        echo "Welcome in the OpenLST Shell!"
          '';
        };
    };
}
