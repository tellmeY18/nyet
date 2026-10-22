{ flake, host, path }:
let
  f = builtins.getFlake (toString flake);
  cfg = f.nixosConfigurations.${host};
  lib = f.inputs.nixpkgs.lib;

  parts = lib.splitString "." path;
  opt = lib.attrByPath parts null cfg.options;
  isOption = builtins.isAttrs opt && (opt._type or "") == "option";

  sanitize = v:
    if builtins.isNull v then null
    else if builtins.isBool v || builtins.isInt v || builtins.isFloat v || builtins.isString v then v
    else if builtins.isPath v then toString v
    else if builtins.isFunction v then null
    else if builtins.isList v then map sanitize v
    else if builtins.isAttrs v then
      if (v.type or "") == "derivation" then { outPath = toString v.outPath; }
      else lib.mapAttrs (_: value: sanitize value) v
    else null;

  defaultTextOf = opt:
    if !(opt ? defaultText) then null
    else if builtins.isString opt.defaultText then opt.defaultText
    else if builtins.isAttrs opt.defaultText then (opt.defaultText.text or null)
    else null;

  nixpkgs = toString f.inputs.nixpkgs;
  definitionFiles =
    if !isOption then [ ]
    else lib.unique (map (definition: toString definition.file)
      (builtins.filter
        (definition:
          definition ? file
          && !(lib.hasPrefix (nixpkgs + "/") (toString definition.file)))
        (opt.definitionsWithLocations or [ ])));

  spec = if isOption then {
    path = path;
    type = opt.type.description or "unspecified";
    description = opt.description or "";
    defaultText = defaultTextOf opt;
    readOnly = opt.readOnly or false;
    internal = opt.internal or false;
    visible =
      let value = opt.visible or true;
      in if builtins.isBool value then value else value == "shallow" || value == "transparent";
    declarations = map toString (opt.declarations or [ ]);
    defined = definitionFiles != [ ];
    inherit definitionFiles;
  } else null;
in
{
  found = isOption;
  spec = spec;
  value = if lib.hasAttrByPath parts cfg.config
    then sanitize (lib.getAttrFromPath parts cfg.config)
    else null;
}
