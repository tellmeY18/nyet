{ flake, host, path }:
let
  f = builtins.getFlake (toString flake);
  cfg = f.nixosConfigurations.${host};
  lib = f.inputs.nixpkgs.lib;

  parts = lib.splitString "." path;
  opt = lib.attrByPath parts null cfg.options;
  isOption = builtins.isAttrs opt && (opt._type or "") == "option";

  simpleTypes = [
    "boolean"
    "signed integer"
    "unsigned integer"
    "string"
    "floating point number"
    "path"
  ];

  sanitize = v:
    if builtins.isNull v then null
    else if builtins.isBool v || builtins.isInt v || builtins.isFloat v || builtins.isString v then v
    else if builtins.isPath v then toString v
    else if builtins.isList v then map sanitize v
    else null;
in
if isOption && lib.elem (opt.type.description or "unspecified") simpleTypes
then sanitize (opt.default or null)
else null
