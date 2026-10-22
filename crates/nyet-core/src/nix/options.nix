{ flake, host }:
let
  f = builtins.getFlake (toString flake);
  cfg = f.nixosConfigurations.${host};
  lib = f.inputs.nixpkgs.lib;

  defaultTextOf = opt:
    if !(opt ? defaultText) then null
    else if builtins.isString opt.defaultText then opt.defaultText
    else if builtins.isAttrs opt.defaultText then (opt.defaultText.text or null)
    else null;

  render = path: opt: {
    path = lib.concatStringsSep "." path;
    type = opt.type.description or "unspecified";
    description = opt.description or "";
    defaultText = defaultTextOf opt;
    readOnly = opt.readOnly or false;
    internal = opt.internal or false;
    visible =
      let value = opt.visible or true;
      in if builtins.isBool value then value else value == "shallow" || value == "transparent";
    declarations = map toString (opt.declarations or [ ]);
  };

  walk = prefix: attrs:
    lib.concatLists (lib.mapAttrsToList (name: value:
      if builtins.isAttrs value && (value._type or "") == "option"
      then [ (render (prefix ++ [ name ]) value) ]
      else if builtins.isAttrs value
      then walk (prefix ++ [ name ]) value
      else [ ]
    ) attrs);
in
walk [ ] cfg.options
