{
  description = "Automatic helm chart deployment to Kubernetes cluster";

  inputs = {
    rust-overlay.url = "github:oxalica/rust-overlay";
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

    cargo2nix = {
      url = "github:cargo2nix/cargo2nix/unstable";
      inputs.rust-overlay.follows = "rust-overlay";
    };
    flake-utils.follows = "cargo2nix/flake-utils";
  };

  outputs = inputs:
    with inputs;
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ cargo2nix.overlays.default ];
        };

        # create the workspace & dependencies package set
        rustPkgs = pkgs.rustBuilder.makePackageSet {
          rustVersion = "1.68.1";
          packageFun = import ./Cargo.nix;
          extraRustComponents = [ "clippy" "rustfmt" ];
        };

        awscli = pkgs.awscli;
        vals = pkgs.vals;
        gnupg = pkgs.gnupg;

        sops = pkgs.writeShellScriptBin "sops" ''
          export SOPS_GPG_EXEC=${gnupg}/bin/gpg
          exec ${pkgs.sops}/bin/sops "$@"
        '';

        helm = pkgs.wrapHelm pkgs.kubernetes-helm {
          plugins = [
            pkgs.kubernetes-helmPlugins.helm-diff
            # pkgs.kubernetes-helmPlugins.helm-secrets
            (pkgs.callPackage ./helm-secrets.nix { })
          ];
          extraMakeWrapperArgs =
            "--set HELM_SECRETS_SOPS_PATH ${sops}/bin/sops --set HELM_SECRETS_VALS_PATH ${vals}/bin/vals";
        };

        bin = (rustPkgs.workspace.helmci { }).bin;
        helmci = pkgs.writeShellScriptBin "helmci" ''
          export HELM_PATH=${helm}/bin/helm
          export AWS_PATH=${awscli}/bin/aws
          exec ${bin}/bin/helmci "$@"
        '';

        # The workspace defines a development shell with all of the dependencies
        # and environment settings necessary for a regular `cargo build`
        workspaceShell = rustPkgs.workspaceShell {
          # This adds cargo2nix to the project shell via the cargo2nix flake
          packages = [
            cargo2nix.packages."${system}".cargo2nix
            helm
            awscli
            sops
            vals
            gnupg
          ];
        };

      in rec {
        packages = {
          inherit helmci helm awscli sops vals gnupg;
          default = pkgs.runCommand "helmci-all" { } ''
            mkdir -p $out/bin
            ln -s ${helmci}/bin/helmci $out/bin/helmci
            ln -s ${helm}/bin/helm $out/bin/helm
            ln -s ${awscli}/bin/aws $out/bin/aws
            ln -s ${sops}/bin/sops $out/bin/sops
            ln -s ${vals}/bin/vals $out/bin/vals
            ln -s ${gnupg}/bin/gpg $out/bin/gpg
          '';
        };
        devShell = workspaceShell;
      });
}
