{ lib, bootvineModule }:
let
  sampleUnits = import ./sample-units.nix;

  machine = {
    imports = [
      bootvineModule
      ./machine.nix
    ];
    _module.args = { inherit sampleUnits; };
  };
in
{
  inherit sampleUnits machine;

  nixosSystemFor =
    system:
    lib.nixosSystem {
      inherit system;
      modules = [
        machine
        ./interactive-vm.nix
      ];
    };
}
