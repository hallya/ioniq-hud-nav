#!/usr/bin/env python3
"""Write the Bluelink EU app constants JSON from upstream's source files.

Upstream is Hyundai-Kia-Connect/hyundai_kia_connect_api (MIT). This script
reads four of its files as text with `ast` and evaluates only the literal
parts it needs (strings, `+`, `str()`, dict and tuple literals, f-strings of
known names, `if BRANDS[self.brand] == BRAND_HYUNDAI` branches). It never
imports or runs upstream code.

Output has the shape of fixtures/bluelink_eu/constants_synthetic.json, read by
the core's `AppConstants::from_json`. It holds real values: it goes into the
app bundle (ios/Generated, not committed), never into the repo.

    python3 -I scripts/app-constants.py UPSTREAM_DIR --upstream-ref REF --out FILE
    python3 -I scripts/app-constants.py UPSTREAM_DIR --check EXPECTED.json

UPSTREAM_DIR holds const.py, ApiImplType1.py, KiaUvoApiEU.py and
cci_fingerprint.py. --check compares the result with EXPECTED (CI runs it on
the synthetic copies) and prints only the names of fields that differ.
Python 3.9+ (the Mac's /usr/bin/python3), stdlib only.
"""

from __future__ import annotations

import argparse
import ast
import base64
import binascii
import json
import re
import sys
from pathlib import Path

FILES = ("const.py", "ApiImplType1.py", "KiaUvoApiEU.py", "cci_fingerprint.py")
MAX_VALUE_LEN = 128  # same limit as the core's upstream override
UUID = re.compile(r"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$")


class Missing(Exception):
    """A value the app needs could not be found or evaluated."""


class B64:
    """`base64.b64decode("…")`: kept as the base64 text."""

    def __init__(self, text: str) -> None:
        self.text = text


class Unknown(Exception):
    pass


def evaluate(node: ast.AST, env: dict):
    """Value of a literal-ish expression, or raise Unknown."""
    if isinstance(node, ast.Constant) and isinstance(node.value, (str, int)):
        return node.value
    if isinstance(node, ast.Name):
        if node.id in env:
            return env[node.id]
        raise Unknown(node.id)
    if isinstance(node, ast.Attribute) and isinstance(node.value, ast.Name):
        key = f"{node.value.id}.{node.attr}"
        if key in env:
            return env[key]
        raise Unknown(key)
    if isinstance(node, ast.BinOp) and isinstance(node.op, ast.Add):
        left, right = evaluate(node.left, env), evaluate(node.right, env)
        if type(left) is not type(right) or not isinstance(left, (str, int)):
            raise Unknown("+")
        return left + right
    if isinstance(node, ast.Call) and not node.keywords and len(node.args) == 1:
        func = ast.unparse(node.func) if hasattr(ast, "unparse") else ""
        if func == "str":
            value = evaluate(node.args[0], env)
            if isinstance(value, (str, int)):
                return str(value)
        if func == "base64.b64decode":
            value = evaluate(node.args[0], env)
            if isinstance(value, str):
                return B64(value)
        raise Unknown(func or "call")
    if isinstance(node, ast.Tuple):
        return tuple(evaluate(e, env) for e in node.elts)
    if isinstance(node, ast.Dict):
        # Entries that do not evaluate (a new brand, a `**spread`) are left
        # out; callers look up the one entry they need and fail if absent.
        out = {}
        for k, v in zip(node.keys, node.values):
            try:
                if k is not None:
                    out[evaluate(k, env)] = evaluate(v, env)
            except Unknown:
                continue
        return out
    if isinstance(node, ast.Subscript):
        container = evaluate(node.value, env)
        index = node.slice.value if isinstance(node.slice, getattr(ast, "Index", ())) else node.slice
        key = evaluate(index, env)
        if isinstance(container, dict) and key in container:
            return container[key]
        raise Unknown("[]")
    if isinstance(node, ast.Compare) and len(node.ops) == 1 and isinstance(node.ops[0], ast.Eq):
        return evaluate(node.left, env) == evaluate(node.comparators[0], env)
    if isinstance(node, ast.IfExp):
        return evaluate(node.body if evaluate(node.test, env) else node.orelse, env)
    if isinstance(node, ast.JoinedStr):
        out = []
        for part in node.values:
            if isinstance(part, ast.Constant):
                out.append(part.value)
            elif isinstance(part, ast.FormattedValue) and part.conversion == -1 and part.format_spec is None:
                value = evaluate(part.value, env)
                if not isinstance(value, str):
                    raise Unknown("f-string")
                out.append(value)
            else:
                raise Unknown("f-string")
        return "".join(out)
    raise Unknown(type(node).__name__)


def module_env(tree: ast.Module, env: dict) -> dict:
    """Module-level assignments that evaluate, in order (last one wins)."""
    env = dict(env)
    for stmt in tree.body:
        target, value = None, None
        if isinstance(stmt, ast.Assign) and len(stmt.targets) == 1:
            target, value = stmt.targets[0], stmt.value
        elif isinstance(stmt, ast.AnnAssign) and stmt.value is not None:
            target, value = stmt.target, stmt.value
        if isinstance(target, ast.Name):
            try:
                env[target.id] = evaluate(value, env)
            except Unknown:
                env.pop(target.id, None)
    return env


def walk_branches(body: list, env: dict) -> None:
    """Runs `self.X = …` assignments of a method body, taking the `if`
    branches whose test evaluates; an untestable `if` is skipped whole."""
    for stmt in body:
        if isinstance(stmt, (ast.Assign, ast.AnnAssign)):
            target = stmt.targets[0] if isinstance(stmt, ast.Assign) else stmt.target
            if isinstance(stmt, ast.Assign) and len(stmt.targets) != 1 or stmt.value is None:
                continue
            if isinstance(target, ast.Attribute) and isinstance(target.value, ast.Name):
                key = f"{target.value.id}.{target.attr}"
            elif isinstance(target, ast.Name):
                key = target.id
            else:
                continue
            try:
                env[key] = evaluate(stmt.value, env)
            except Unknown:
                env.pop(key, None)
        elif isinstance(stmt, ast.If):
            try:
                taken = evaluate(stmt.test, env)
            except Unknown:
                continue
            walk_branches(stmt.body if taken else stmt.orelse, env)


def find_class(tree: ast.Module, name: str) -> ast.ClassDef:
    for node in tree.body:
        if isinstance(node, ast.ClassDef) and node.name == name:
            return node
    raise Missing(f"class {name}")


def find_function(body: list, name: str) -> ast.FunctionDef:
    for node in body:
        if isinstance(node, ast.FunctionDef) and node.name == name:
            return node
    raise Missing(f"function {name}")


def one_assignment(node: ast.AST, name: str) -> ast.expr:
    """The value of the single `name = …` anywhere under node."""
    found = [
        n.value
        for n in ast.walk(node)
        if isinstance(n, ast.Assign)
        and len(n.targets) == 1
        and isinstance(n.targets[0], ast.Name)
        and n.targets[0].id == name
    ]
    if len(found) != 1:
        raise Missing(f"{name} ({len(found)} assignments)")
    return found[0]


def need(env: dict, key: str, where: str):
    if key not in env:
        raise Missing(f"{where}: {key}")
    return env[key]


def extract(upstream: Path) -> dict:
    trees = {}
    for name in FILES:
        path = upstream / name
        try:
            trees[name] = ast.parse(path.read_text(encoding="utf-8"), filename=name)
        except (OSError, SyntaxError, ValueError) as e:
            raise Missing(f"{name}: {type(e).__name__}") from None

    const = module_env(trees["const.py"], {})
    hyundai = need(const, "BRAND_HYUNDAI", "const.py")
    brands = need(const, "BRANDS", "const.py")
    brand_ids = [k for k, v in brands.items() if v == hyundai]
    if len(brand_ids) != 1:
        raise Missing("const.py: BRANDS entry for BRAND_HYUNDAI")
    brand = brand_ids[0]

    impl1 = module_env(trees["ApiImplType1.py"], {})
    eu_tree = trees["KiaUvoApiEU.py"]
    eu = module_env(eu_tree, const)
    eu_class = find_class(eu_tree, "KiaUvoApiEU")
    init = find_function(eu_class.body, "__init__")
    obj = dict(eu, brand=brand, region=1, language="en")
    walk_branches(init.body, obj)
    login = find_function(eu_class.body, "_login_with_password_cci")
    try:
        idp_user_agent = evaluate(one_assignment(login, "mobile_ua"), eu)
    except Unknown as e:
        raise Missing(f"KiaUvoApiEU.py: mobile_ua ({e})") from None

    fp_tree = trees["cci_fingerprint.py"]
    fp = module_env(fp_tree, const)
    keys = need(fp, "_FINGERPRINT_KEYS", "cci_fingerprint.py")
    key = keys.get(hyundai) if isinstance(keys, dict) else None
    if not (isinstance(key, tuple) and len(key) == 3):
        raise Missing("cci_fingerprint.py: _FINGERPRINT_KEYS[BRAND_HYUNDAI]")
    name_fn = find_function(fp_tree.body, "client_name_for_brand")
    returns = [n.value for n in ast.walk(name_fn) if isinstance(n, ast.Return)]
    try:
        if len(returns) != 1:
            raise Unknown("returns")
        client_name = evaluate(returns[0], dict(fp, brand=brand))
    except Unknown as e:
        raise Missing(f"cci_fingerprint.py: client_name_for_brand ({e})") from None

    where = "KiaUvoApiEU.__init__"
    cfb = need(obj, "self.CFB", where)
    if not isinstance(cfb, B64):
        raise Missing(f"{where}: self.CFB is not base64.b64decode(<literal>)")
    return {
        "idp_host": need(obj, "self.LOGIN_FORM_HOST", where),
        "cci_host": need(obj, "self.CCI_API_URL", where),
        "spa_host": "https://" + need(obj, "self.BASE_URL", where),
        "oneapp_client_id": need(obj, "self.ONEAPP_CLIENT_ID", where),
        "oneapp_redirect_uri": need(obj, "self.ONEAPP_REDIRECT_URI", where),
        "ccsp_service_id": need(obj, "self.CCSP_SERVICE_ID", where),
        "app_id": need(obj, "self.APP_ID", where),
        "cfb_base64": cfb.text,
        "push_type": need(obj, "self.PUSH_TYPE", where),
        "idp_user_agent": idp_user_agent,
        "spa_user_agent": need(impl1, "USER_AGENT_OK_HTTP", "ApiImplType1.py"),
        "client": {
            "package_id": need(obj, "self.CCI_PACKAGE_ID", where),
            "client_name": client_name,
            "version": need(fp, "CLIENT_VERSION", "cci_fingerprint.py"),
            "os_code": need(fp, "CLIENT_OS_CODE", "cci_fingerprint.py"),
            "os_version": need(fp, "CLIENT_OS_VERSION", "cci_fingerprint.py"),
            "device_model": need(fp, "CLIENT_DEVICE_MODEL", "cci_fingerprint.py"),
            "notification_provider": need(fp, "CLIENT_NOTIFICATION_PROVIDER", "cci_fingerprint.py"),
            "user_agent": need(fp, "USER_AGENT", "cci_fingerprint.py"),
            "brand_label": hyundai,
            "region": need(fp, "APP_REGION", "cci_fingerprint.py"),
        },
        "signing_key": {"secret": key[0], "piece": key[1], "suffix": key[2]},
    }


def validate(c: dict) -> None:
    """Every value is a short printable-ASCII string; hosts, ids and the CFB
    have their expected shape. Errors name the field, never the value."""

    def strings(d: dict, prefix: str = ""):
        for k, v in d.items():
            if isinstance(v, dict):
                yield from strings(v, f"{prefix}{k}.")
            else:
                yield f"{prefix}{k}", v

    for name, value in strings(c):
        if not (isinstance(value, str) and 0 < len(value) <= MAX_VALUE_LEN and all(0x20 <= ord(ch) <= 0x7E for ch in value)):
            raise Missing(f"{name} is not a short printable string")
    for name in ("idp_host", "cci_host", "spa_host", "oneapp_redirect_uri"):
        if not c[name].startswith("https://"):
            raise Missing(f"{name} is not https")
    for name in ("oneapp_client_id", "ccsp_service_id", "app_id"):
        if not UUID.match(c[name]):
            raise Missing(f"{name} is not a uuid")
    try:
        cfb = base64.b64decode(c["cfb_base64"], validate=True)
    except (binascii.Error, ValueError):
        cfb = b""
    if not cfb:
        raise Missing("cfb_base64 does not decode")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("upstream", type=Path)
    parser.add_argument("--upstream-ref", default="unknown", help="repo@commit, recorded in the output")
    parser.add_argument("--out", type=Path)
    parser.add_argument("--check", type=Path, help="compare with this JSON instead of writing")
    args = parser.parse_args()
    if (args.out is None) == (args.check is None):
        parser.error("give exactly one of --out or --check")

    try:
        constants = extract(args.upstream)
        validate(constants)
    except Missing as e:
        print(f"app-constants: upstream changed shape, not found: {e}", file=sys.stderr)
        return 1

    if args.check:
        expected = json.loads(args.check.read_text(encoding="utf-8"))
        expected.pop("about", None)
        differ = sorted(
            k for k in set(expected) | set(constants) if expected.get(k) != constants.get(k)
        )
        if differ:
            print(f"app-constants: differs from {args.check.name}: {', '.join(differ)}", file=sys.stderr)
            return 1
        print(f"app-constants: matches {args.check.name}")
        return 0

    out = {"upstream": args.upstream_ref, **constants}
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(out, indent=2) + "\n", encoding="utf-8")
    print(f"app-constants: wrote {args.out} from {args.upstream_ref}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
