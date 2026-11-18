{
  description = "Open YouTube links from Firefox or Floorp in Grayjay Desktop";

  inputs = {
    harbor-rs.url = "git+https://github.com/caniko/harbor-rs.git?ref=feat/browser-extension";
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    plinth = {
      url = "git+https://github.com/caniko/plinth.git?ref=refs/heads/trunk";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = {
    self,
    harbor-rs,
    nixpkgs,
    rust-overlay,
    plinth,
    ...
  }: let
    systems = ["x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin"];
    forAllSystems = nixpkgs.lib.genAttrs systems;
    inherit ((fromTOML (builtins.readFile ./Cargo.toml)).package) version;
    extensionId = "open-in-grayjay@tartanoglu.com";
    domain = "open-in-grayjay.tartanoglu.com";
  in {
    packages = forAllSystems (system: let
      pkgs = import nixpkgs {
        inherit system;
        overlays = [rust-overlay.overlays.default];
      };
      inherit (pkgs) lib;
      toolchain = harbor-rs.lib.mkToolchain {
        inherit pkgs;
        toolchainProfile = "stable";
      };
      rustPlatform = pkgs.makeRustPlatform {
        rustc = toolchain.rustToolchain;
        cargo = toolchain.rustToolchain;
      };
      buildCache = harbor-rs.lib.mkBuildCachePolicy {
        inherit pkgs;
        sccachePackage = harbor-rs.packages.${system}.sccache;
        cacheRoot = null;
        namespaceScope = "canix-rust";
        namespaceGeneration = 5;
        # Hosted CI runners have no managed sccache transport.
        ephemeralFallback = true;
      };
      host = buildCache.withRustCache {
        package = rustPlatform.buildRustPackage {
          pname = "open-in-grayjay";
          inherit version;
          src = lib.fileset.toSource {
            root = ./.;
            fileset = lib.fileset.unions [./Cargo.toml ./Cargo.lock ./src];
          };
          cargoLock.lockFile = ./Cargo.lock;
          meta.mainProgram = "open-in-grayjay";
        };
      };
      native-host = harbor-rs.lib.mkNativeMessagingHost {
        inherit pkgs;
        name = "open_in_grayjay";
        description = "Open video links in Grayjay Desktop";
        program = lib.getExe host;
        allowedExtensions = [extensionId];
      };
      addon = harbor-rs.lib.mkBrowserExtension {
        inherit pkgs version extensionId;
        pname = "open-in-grayjay-addon";
        src = ./addon;
      };
      website = plinth.lib.${system}.mkProjectSite {
        pname = "open-in-grayjay-website";
        inherit domain;
        configPath = ./website/plinth-project.toml;
      };
    in {
      default = pkgs.symlinkJoin {
        name = "open-in-grayjay-${version}";
        paths = [host native-host addon];
      };
      inherit host native-host addon website;
      site = website;
    });

    devShells = forAllSystems (system: let
      pkgs = import nixpkgs {
        inherit system;
        overlays = [rust-overlay.overlays.default];
      };
      toolchain = harbor-rs.lib.mkToolchain {
        inherit pkgs;
        toolchainProfile = "stable";
      };
    in {
      default = pkgs.mkShell {packages = [toolchain.rustToolchain pkgs.treefmt pkgs.alejandra];};
    });

    checks = forAllSystems (system: {inherit (self.packages.${system}) host native-host addon;});

    apps = forAllSystems (system: {
      deploy-pages = plinth.lib.${system}.mkDeployPagesApp {inherit domain;};
    });
  };
}
