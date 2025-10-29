{
  inputs,
  targetSystem,
}: let
  pkgs = inputs.nixpkgs.legacyPackages.${targetSystem};
  inherit (pkgs) lib;

  rustPackages = inputs.fenix.packages.${pkgs.system}.stable;

  craneLib = (inputs.crane.mkLib pkgs).overrideToolchain rustPackages.toolchain;

  src = lib.cleanSourceWith {
    src = lib.cleanSource ../.;
    filter = path: type:
      craneLib.filterCargoSources path type
      || lib.hasSuffix ".scale" path
      || lib.hasSuffix ".graphql" path
      || lib.hasSuffix "NODE_VERSION" path;
    name = "source";
  };

  commonArgs =
    {
      pname = "midnight-indexer";
      inherit src;
      strictDeps = true;

      nativeBuildInputs =
        [
          pkgs.gnum4
          pkgs.protobuf
        ]
        ++ lib.optionals pkgs.stdenv.isLinux [
          pkgs.pkg-config
        ];
      buildInputs =
        lib.optionals pkgs.stdenv.isLinux [
          pkgs.openssl
        ]
        ++ lib.optionals pkgs.stdenv.isDarwin [
          pkgs.libiconv
          pkgs.darwin.apple_sdk_12_3.frameworks.SystemConfiguration
          pkgs.darwin.apple_sdk_12_3.frameworks.Security
          pkgs.darwin.apple_sdk_12_3.frameworks.CoreFoundation
        ];
    }
    // lib.optionalAttrs pkgs.stdenv.isLinux {
      # The linker bundled with Fenix has wrong interpreter path, and it fails with ENOENT, so:
      RUSTFLAGS = "-Clink-arg=-fuse-ld=bfd";
    }
    // lib.optionalAttrs pkgs.stdenv.isDarwin {
      # for bindgen
      LIBCLANG_PATH = "${lib.getLib pkgs.llvmPackages.libclang}/lib";
    };

  cargoArtifacts = craneLib.buildDepsOnly commonArgs;

  packagesCloud = craneLib.buildPackage (commonArgs
    // {
      inherit cargoArtifacts;
      pname = commonArgs.pname + "-cloud";
      doCheck = false; # we run tests elsewhere
      cargoExtraArgs = "--features cloud";
    });

  packagesStandalone = craneLib.buildPackage (commonArgs
    // {
      inherit cargoArtifacts;
      pname = commonArgs.pname + "-standalone";
      doCheck = false; # we run tests elsewhere
      cargoExtraArgs = "-p indexer-standalone --features standalone";
    });

  packages = pkgs.stdenv.mkDerivation {
    inherit (packagesCloud) pname version;
    buildCommand = ''
      mkdir -p $out
      cp -vr ${packagesCloud}/bin $out/
      chmod -R +w $out
      cp -vf ${packagesStandalone}/bin/indexer-standalone $out/bin/
    '';
    meta.mainProgram = "indexer-standalone";
  };
in
  packages
