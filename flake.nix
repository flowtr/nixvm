{
	inputs = {
		nixpkgs = {
			url = "github:NixOS/nixpkgs/nixpkgs-unstable";
		};
	};

	outputs = { self, nixpkgs, nix }:
		let
			system = "x86_64-linux";
			pkgs = nixpkgs.legacyPackages."${system}";
		in
		{
			devShells."${system}".default = pkgs.mkShell {
				buildInputs = with pkgs; [
					rustc
					cargo
					rust-analyzer
					rustfmt
				];
			};
		};
}
