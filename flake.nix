{
  description = "tongue — CLI chuyển chế độ gõ vi/en/zh (GoNhanh trên macOS, VKey trên Windows)";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

  outputs = { self, nixpkgs }: let
    # Chỉ darwin: bản Windows phân phối qua CI artifact, Linux ngoài phạm vi v1.
    # x86_64-darwin đã bị nixpkgs 26.11 gỡ hẳn — khai nó ở đây làm
    # `packages.x86_64-darwin` thành lỗi eval cứng, đủ để `nix flake check` chết.
    systems = [ "aarch64-darwin" ];
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

    # Chỉ khai `tongue` trên system mà `packages` thật sự có. Người dùng overlay
    # này thường áp chung một danh sách overlay cho MỌI host của họ, kể cả Linux;
    # khai vô điều kiện thì `pkgs.tongue` trên Linux là một attribute nổ ngay khi
    # bị chạm vào, với lỗi "attribute 'x86_64-linux' missing" chẳng nói lên điều
    # gì. Vắng mặt hẳn là thông điệp đúng: nền tảng này chưa có tongue.
    # `prev`, không phải `final`: điều kiện này quyết định overlay khai ra những
    # TÊN nào, mà tập tên phải biết được trước khi bất kỳ giá trị nào được tính.
    # Hỏi `final.stdenv` ở đây là bắt fixpoint tự tham chiếu -> infinite recursion.
    overlays.default = final: prev:
      nixpkgs.lib.optionalAttrs (builtins.elem prev.stdenv.hostPlatform.system systems) {
        tongue = self.packages.${prev.stdenv.hostPlatform.system}.tongue;
      };

    devShells = forAll (pkgs: {
      default = pkgs.mkShell {
        packages = [ pkgs.cargo pkgs.rustc pkgs.clippy pkgs.rustfmt pkgs.rust-analyzer ];
      };
    });
  };
}
