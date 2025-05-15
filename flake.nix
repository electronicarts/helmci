{
  description = "Automatic helm chart deployment to Kubernetes cluster";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-24.11";
    nixpkgs-unstable.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
    crane.url = "github:ipetkov/crane";
  };

  outputs =
    {
      nixpkgs,
      nixpkgs-unstable,
      flake-utils,
      rust-overlay,
      crane,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };
        pkgs-unstable = nixpkgs-unstable.legacyPackages.${system};

        osxlibs = pkgs.lib.lists.optionals pkgs.stdenv.isDarwin [
          pkgs.darwin.apple_sdk.frameworks.Security
          pkgs.darwin.apple_sdk.frameworks.Foundation
        ];

        src = ./.;

        rustPlatform = pkgs.rust-bin.stable.latest.default;

        craneLib = (crane.mkLib pkgs).overrideToolchain rustPlatform;

        # Build *just* the cargo dependencies, so we can reuse
        # all of that work (e.g. via cachix) when running in CI
        cargoArtifacts = craneLib.buildDepsOnly { inherit src; };

        # Run clippy (and deny all warnings) on the crate source.
        clippy = craneLib.cargoClippy {
          inherit cargoArtifacts src;
          cargoClippyExtraArgs = "-- --deny warnings";
        };

        # Next, we want to run the tests and collect code-coverage, _but only if
        # the clippy checks pass_ so we do not waste any extra cycles.
        coverage = craneLib.cargoTarpaulin {
          inherit src;
          cargoArtifacts = clippy;
        };

        # create the workspace & dependencies package set
        pkg = craneLib.buildPackage {
          inherit src;
          inherit cargoArtifacts;
          buildInputs = osxlibs;

          # Add extra inputs here or any other derivation settings
          doCheck = true;
        };

        awscli = pkgs.awscli2;
        vals = pkgs.vals;
        gnupg = pkgs.gnupg;

        sops = pkgs.writeShellScriptBin "sops" ''
          export SOPS_GPG_EXEC=${gnupg}/bin/gpg
          exec ${pkgs.sops}/bin/sops "$@"
        '';

        helm = pkgs.wrapHelm pkgs.kubernetes-helm {
          plugins = [
            pkgs.kubernetes-helmPlugins.helm-diff
            pkgs.kubernetes-helmPlugins.helm-secrets
          ];
          extraMakeWrapperArgs = "--set HELM_SECRETS_SOPS_PATH ${sops}/bin/sops --set HELM_SECRETS_VALS_PATH ${vals}/bin/vals";
        };

        helmci = pkgs.writeShellScriptBin "helmci" ''
          export HELM_PATH=${helm}/bin/helm
          export AWS_PATH=${awscli}/bin/aws
          exec ${pkg}/bin/helmci "$@"
        '';

        # The workspace defines a development shell with all of the dependencies
        # and environment settings necessary for a regular `cargo build`
        rustSrcPlatform = rustPlatform.override { extensions = [ "rust-src" ]; };
        workspaceShell = pkgs.mkShell {
          buildInputs = [
            pkgs-unstable.rust-analyzer
            rustSrcPlatform
            helm
            awscli
            sops
            vals
            gnupg
          ] ++ osxlibs;
        };
      in
      {
        checks = { inherit clippy coverage pkg; };
        packages = {
          inherit
            helmci
            helm
            awscli
            sops
            vals
            gnupg
            ;
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
        devShells.default = workspaceShell;
      }
    );
}
