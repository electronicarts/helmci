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

        # Define wrapper
        helm = pkgs.wrapHelm pkgs.kubernetes-helm {
          plugins = [
            pkgs.kubernetes-helmPlugins.helm-diff
            # pkgs.kubernetes-helmPlugins.helm-secrets
            (pkgs.callPackage ./helm-secrets.nix {})
          ];
        };
        bin = (rustPkgs.workspace.helmci { }).bin;
        wrapper = pkgs.writeShellScriptBin "helmci" ''
          export HELM_PATH=${helm}/bin/helm
          export AWS_PATH=${pkgs.awscli}/bin/aws
          export HELM_SECRETS_VALS_PATH=${pkgs.vals}/bin/vals
          exec ${bin}/bin/helmci "$@"
        '';

        # The workspace defines a development shell with all of the dependencies
        # and environment settings necessary for a regular `cargo build`
        workspaceShell = rustPkgs.workspaceShell {
          # This adds cargo2nix to the project shell via the cargo2nix flake
          packages =
            [ cargo2nix.packages."${system}".cargo2nix helm pkgs.awscli ];
        };

      in rec {
        packages = {
          # replace hello-world with your package name
          helm = helm;
          helmci = wrapper;
          default = packages.helmci;
        };
        devShell = workspaceShell;
      });
}
