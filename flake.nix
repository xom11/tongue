{
  description = "tongue — CLI chuyển chế độ gõ vi/en/zh (GoNhanh trên macOS, VKey trên Windows)";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

  outputs = { self, nixpkgs }: let
    # chỉ darwin: bản Windows phân phối qua CI artifact, Linux ngoài phạm vi v1
    systems = [ "aarch64-darwin" "x86_64-darwin" ];
    forAll = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
  in {
    packages = forAll (pkgs: rec {
      tongue = pkgs.rustPlatform.buildRustPackage {
        pname = "tongue";
        version = "0.1.0";
        src = self;
        cargoLock.lockFile = ./Cargo.lock;
        # link framework Carbon (TIS API) do stdenv darwin lo; nếu thiếu:
        # buildInputs = [ pkgs.apple-sdk ];
      };
      default = tongue;
    });

    overlays.default = final: prev: {
      tongue = self.packages.${final.stdenv.hostPlatform.system}.tongue;
    };

    devShells = forAll (pkgs: {
      default = pkgs.mkShell {
        packages = [ pkgs.cargo pkgs.rustc pkgs.clippy pkgs.rustfmt pkgs.rust-analyzer ];
      };
    });
  };
}
