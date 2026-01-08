{
  description = "Nixos config flake";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    nixpkgs-python.url = "github:cachix/nixpkgs-python";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = {
		nixpkgs,
		nixpkgs-python,
		flake-utils,
		...
	}: 
	flake-utils.lib.eachDefaultSystem (
    system:
    let
      pythonVersion = "3.7.0";
      pkgs = import nixpkgs { inherit system; };
      buildPython = nixpkgs-python.packages.${system}.${pythonVersion};
    in
    {
      devShells.default = pkgs.mkShell {
        buildInputs = [
          buildPython
					pkgs.gcc
					pkgs.libgcc

          pkgs.sdcc
          pkgs.cc-tool
          pkgs.picocom
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
