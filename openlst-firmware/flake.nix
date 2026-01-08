{
  description = "Nixos config flake";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = {
		nixpkgs,
		flake-utils,
		...
	}: 
	flake-utils.lib.eachDefaultSystem (
    system:
    let
      pkgs = import nixpkgs { inherit system; };
    in
    {
      devShells.default = pkgs.mkShell {
        buildInputs = with pkgs; [
          python3
					gcc
					libgcc

          sdcc
          cc-tool
          picocom
        ];
				LD_LIBRARY_PATH = "${pkgs.stdenv.cc.cc.lib}/lib";
        shellHook = ''
				  python -m venv .venv
				  source .venv/bin/activate
          python -m pip install -e open-lst/tools

	        echo "Welcome in the OpenLST Shell!"
        '';
      };
    }
  );
}
