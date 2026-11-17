"""Execute a pinned official CORE diagnostic profile, without claiming conformance."""

import argparse
from collections import Counter
import csv
from datetime import datetime, timezone
from email.parser import Parser
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import platform
import re
import shutil
import signal
import subprocess
import sys
import tarfile
import tempfile


STATUSES = {"SUCCESS", "ISSUE REPORTED", "SKIPPED", "EXECUTION ERROR"}
ENV_FLAGS = {"PRODUCT": "-s", "VERSION": "-v", "SUBSTANDARD": "-ss",
             "USE_CASE": "-uc", "CT": "-ct", "DEFINE_XML": "-dx"}
REPORT_TEMPLATES = ("resources/templates/report-template.xlsx",
                    "resources/templates/usdm-report-template.xlsx")


def write_json(path: Path, value: dict) -> None:
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


class DigestReader:
    def __init__(self, stream):
        self.stream = stream
        self.digest = hashlib.sha256()

    def read(self, size: int) -> bytes:
        block = self.stream.read(size)
        self.digest.update(block)
        return block

    def finish(self) -> str:
        while self.read(1024 * 1024):
            pass
        return self.digest.hexdigest()


def sha256(path: Path) -> str:
    with path.open("rb") as stream:
        return DigestReader(stream).finish()


def load_profile(path: Path) -> dict:
    profile = json.loads(path.read_text(encoding="utf-8"))
    if profile.get("schema_version") != 1 or profile.get("scope") != "Published":
        raise ValueError("expected schema_version=1 and scope=Published")
    if profile.get("conformance_claim") is not False:
        raise ValueError("profile must explicitly disclaim conformance")
    for section, key, length in [("rules", "sha", 40), ("rules", "engine_submodule_sha", 40),
                                 ("engine", "sha", 40), ("engine", "asset_sha256", 64),
                                 ("engine", "binary_sha256", 64), ("engine", "cache_tree_sha256", 64)]:
        if not re.fullmatch(f"[0-9a-f]{{{length}}}", profile[section][key]):
            raise ValueError(f"invalid pin: {section}.{key}")
    if not profile["engine"].get("version") or not profile.get("cases"):
        raise ValueError("version and nonempty cases are required")
    timeout = profile.get("timeout_seconds", 300)
    if type(timeout) is not int or not 1 <= timeout <= 3600:
        raise ValueError("timeout_seconds must be between 1 and 3600")
    labels = set()
    for case in profile["cases"]:
        label = case["label"]
        if not re.fullmatch(r"[a-z0-9][a-z0-9-]*", label) or label in labels:
            raise ValueError(f"invalid or duplicate label: {label}")
        labels.add(label)
        if not re.fullmatch(r"CORE-\d{6}", case["rule_id"]):
            raise ValueError("invalid rule_id")
        if not re.fullmatch(r"CORE-\d{6}/(?:positive|negative)/\d+", case["fixture"]):
            raise ValueError("invalid fixture path")
        for key in ("environment", "overrides"):
            values = case.get(key, {})
            if not isinstance(values, dict) or set(values) - ENV_FLAGS.keys():
                raise ValueError(f"invalid {key} for {label}")
            if any(not isinstance(v, str) or not v or "\n" in v for v in values.values()):
                raise ValueError(f"invalid environment value for {label}")
        if not {"PRODUCT", "VERSION"}.issubset(case["environment"]):
            raise ValueError(f"missing standard/version for {label}")
        if case.get("expected_status") is not None and case["expected_status"] not in STATUSES:
            raise ValueError(f"invalid expected status for {label}")
    return profile


def isolated_environment(out: Path) -> dict:
    return {"PATH": os.defpath, "HOME": str(out / "home"), "TMPDIR": str(out / "tmp"),
            "LANG": "C.UTF-8", "LC_ALL": "C.UTF-8", "CACHE_TYPE": "in_memory"}


def git(root: Path, *args: str) -> bytes:
    env = {"PATH": os.defpath, "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": os.devnull,
           "GIT_TERMINAL_PROMPT": "0", "LC_ALL": "C"}
    result = subprocess.run(["git", "-C", str(root), *args], capture_output=True, env=env,
                            timeout=60, check=False)
    if result.returncode:
        raise ValueError(f"git {' '.join(args)} failed: {result.stderr.decode(errors='replace').strip()}")
    return result.stdout


def check_rules(root: Path, pins: dict) -> dict:
    if Path(os.fsdecode(git(root, "rev-parse", "--show-toplevel")).strip()).resolve() != root:
        raise ValueError("rules-root must be the checkout root")
    head = git(root, "rev-parse", "HEAD").decode().strip()
    if head != pins["sha"]:
        raise ValueError(f"rules SHA mismatch: {head}")
    git(root, "diff", "--no-ext-diff", "--no-textconv", "--ignore-submodules=all", "--quiet", "HEAD", "--")
    entry = git(root, "ls-tree", "HEAD", "--", "engine").decode().strip()
    if entry != f"160000 commit {pins['engine_submodule_sha']}\tengine":
        raise ValueError("rules engine submodule pin mismatch")
    return {"head": head, "tracked_clean": True, "engine_gitlink": pins["engine_submodule_sha"],
            "untracked_files": "not enumerated or used"}


def cache_manifest(root: Path) -> dict:
    if root.is_symlink() or not root.is_dir():
        raise ValueError("cache must be a real directory")
    files = {}
    for path in sorted(root.rglob("*")):
        if path.is_symlink():
            raise ValueError(f"cache symlink rejected: {path.name}")
        if path.is_file():
            files[path.relative_to(root).as_posix()] = sha256(path)
    tree_hash = hashlib.sha256(json.dumps(files, sort_keys=True, separators=(",", ":")).encode()).hexdigest()
    return {"files": files, "file_count": len(files), "tree_sha256": tree_hash}


def check_cache(manifest: dict, pins: dict) -> None:
    if (manifest["tree_sha256"] != pins["cache_tree_sha256"]
            or manifest["file_count"] != pins["cache_file_count"]):
        raise ValueError("cache tree pin mismatch")
    for name, expected in pins.get("cache_files", {}).items():
        if manifest["files"].get(name) != expected:
            raise ValueError(f"cache file pin mismatch: {name}")


def unpack_engine(bundle: tarfile.TarFile, destination: Path) -> None:
    seen = set()
    for member in bundle:
        name = PurePosixPath(member.name)
        if name in seen:
            raise ValueError(f"duplicate engine archive path: {member.name}")
        seen.add(name)
        if name == PurePosixPath(".") and member.isdir():
            continue
        if name.is_absolute() or ".." in name.parts or name.parts[:1] != ("core",):
            raise ValueError(f"invalid engine archive path: {member.name}")
        target = destination.joinpath(*name.parts)
        if member.isdir():
            target.mkdir(mode=0o700, parents=True, exist_ok=True)
        elif member.type in {tarfile.REGTYPE, tarfile.AREGTYPE} and not member.issparse():
            target.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
            with bundle.extractfile(member) as source, target.open("xb") as output:
                shutil.copyfileobj(source, output)
            target.chmod(0o700 if member.mode & 0o111 else 0o600)
        else:
            raise ValueError(f"unsupported engine archive entry: {member.name}")


def install_engine(archive: Path, destination: Path, pins: dict) -> dict:
    with archive.open("rb") as stream:
        actual = DigestReader(stream).finish()
        if actual != pins["asset_sha256"]:
            raise ValueError(f"engine archive SHA-256 mismatch: {actual}")
        stream.seek(0)
        # Verify the actual extraction stream too, in case the open archive is changed in place.
        reader = DigestReader(stream)
        with tarfile.open(fileobj=reader, mode="r|gz") as bundle:
            unpack_engine(bundle, destination)
        if reader.finish() != actual:
            raise ValueError("engine archive changed during extraction")

    engine = destination / "core"
    binary_sha256 = sha256(engine / "core")
    if binary_sha256 != pins["binary_sha256"]:
        raise ValueError(f"engine binary SHA-256 mismatch: {binary_sha256}")
    templates = {name: sha256(engine / name) for name in REPORT_TEMPLATES}
    # The pinned release stores core/core with mode 0644.
    (engine / "core").chmod(0o700)
    return {"archive_verification": {"status": "verified", "sha256": actual,
                                     "binding": "fresh private extraction"},
            "binary_sha256": binary_sha256, "template_sha256": templates}


def check_platform(out: Path, rules: Path) -> None:
    if platform.system() != "Linux" or platform.machine().lower() not in {"x86_64", "amd64"}:
        raise ValueError("official Ubuntu bundle requires Linux x86-64")
    with tempfile.TemporaryDirectory(dir=out) as temporary:
        probe = Path(temporary) / "CaseSensitive"
        probe.touch()
        if probe.with_name("casesensitive").exists():
            raise ValueError("output filesystem is not case-sensitive")
    witness = rules / "README.md"
    if not witness.is_file():
        raise ValueError(f"missing filesystem witness: {witness}")
    alternate = witness.with_name(witness.name.swapcase())
    if alternate.exists() and alternate.samefile(witness):
        raise ValueError(f"filesystem is not case-sensitive: {witness.parent}")


def reject_ambient_dotenv(root: Path) -> None:
    # python-dotenv searches parent directories from the frozen executable's cwd.
    for parent in (root, *root.parents):
        if os.path.lexists(parent / ".env"):
            raise ValueError("audit ancestry contains .env; choose an isolated output path")


def tracked_files(root: Path, relative: str) -> dict:
    entries = git(root, "ls-tree", "-rz", "HEAD", "--", relative)
    files = {}
    for entry in entries.split(b"\0"):
        if not entry:
            continue
        metadata, raw_name = entry.split(b"\t", 1)
        mode, kind, blob = metadata.decode().split()
        name = os.fsdecode(raw_name)
        if kind != "blob" or mode not in {"100644", "100755"}:
            raise ValueError(f"non-regular tracked input: {name}")
        path = root / name
        if path.is_symlink() or root not in path.resolve().parents:
            raise ValueError(f"input escapes checkout: {name}")
        content = path.read_bytes()
        actual = hashlib.sha1(b"blob " + str(len(content)).encode() + b"\0" + content).hexdigest()
        if actual != blob:
            raise ValueError(f"tracked input content differs from HEAD: {name}")
        files[name] = {"git_blob": blob, "sha256": hashlib.sha256(content).hexdigest(),
                       "bytes": len(content)}
    if not files:
        raise ValueError(f"no tracked inputs: {relative}")
    return files


def read_fixture_environment(path: Path) -> dict:
    values = {}
    for line in path.read_text(encoding="utf-8-sig").splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        key, separator, value = line.partition("=")
        key, value = key.strip(), value.strip()
        if not separator or key not in ENV_FLAGS or key in values or not value:
            raise ValueError("fixture .env must contain unique supported settings only")
        values[key] = value
    return values


def prepare_case(root: Path, case: dict, destination: Path) -> dict:
    data_relative = f"Published/{case['fixture']}/data"
    rule_relative = f"Published/{case['rule_id']}/rule.yml"
    files = {**tracked_files(root, data_relative), **tracked_files(root, rule_relative)}
    for name, metadata in files.items():
        source = root / name
        target = destination / "input" / name
        target.parent.mkdir(parents=True, exist_ok=True)
        content = source.read_bytes()
        if hashlib.sha256(content).hexdigest() != metadata["sha256"]:
            raise ValueError(f"input changed during snapshot: {name}")
        target.write_bytes(content)
    data = destination / "input" / data_relative
    environment = read_fixture_environment(data / ".env")
    if environment != case["environment"]:
        raise ValueError(f"fixture environment differs from profile: {case['label']}")
    write_json(destination / "inputs.json", files)
    return {"files": files, "data": str(data), "rule": str(destination / "input" / rule_relative),
            "fixture_environment": environment, "effective_environment": {**environment, **case.get("overrides", {})}}


def execute(argv: list, cwd: Path, env: dict, prefix: Path, timeout: int) -> dict:
    result = {"argv": argv, "cwd": str(cwd), "environment": env, "returncode": None,
              "stdout": str(prefix.with_suffix(".stdout.txt")),
              "stderr": str(prefix.with_suffix(".stderr.txt")), "timed_out": False}
    with Path(result["stdout"]).open("wb") as stdout, Path(result["stderr"]).open("wb") as stderr:
        try:
            with subprocess.Popen(argv, cwd=cwd, env=env, stdout=stdout, stderr=stderr,
                                  start_new_session=True) as process:
                try:
                    result["returncode"] = process.wait(timeout=timeout)
                except subprocess.TimeoutExpired:
                    result["timed_out"] = True
                    os.killpg(process.pid, signal.SIGKILL)
                    result["returncode"] = process.wait()
        except OSError as error:
            result["launch_error"] = str(error)
    return result


def inspect_report(path: Path, rule_id: str) -> dict:
    report = json.loads(path.read_text(encoding="utf-8"))
    rows = report.get("Rules_Report") if isinstance(report, dict) else None
    if not isinstance(rows, list) or len(rows) != 1 or not isinstance(rows[0], dict):
        raise ValueError("Rules_Report must contain exactly one execution record")
    row = rows[0]
    if row.get("core_id") != rule_id:
        raise ValueError("Rules_Report identifies the wrong rule")
    if row.get("status") not in STATUSES:
        raise ValueError("Rules_Report has an unknown execution status")
    if not isinstance(report.get("Issue_Details"), list):
        raise ValueError("Issue_Details must be a list")
    if row["status"] == "SUCCESS" and report["Issue_Details"]:
        raise ValueError("SUCCESS report contradicts nonempty Issue_Details")
    if row["status"] == "ISSUE REPORTED" and not report["Issue_Details"]:
        raise ValueError("ISSUE REPORTED report has no Issue_Details")
    return {"status": row["status"], "rules_report": rows,
            "conformance_details": report.get("Conformance_Details"),
            "issue_details_count": len(report["Issue_Details"]), "sha256": sha256(path)}


def verify_csv_projection(path: Path, report_path: Path, status: str) -> int:
    with path.open(encoding="utf-8-sig", newline="") as stream:
        reader = csv.reader(stream)
        header = next(reader, None)
        tabular = header == ["Dataset", "Record", "Variable", "Value"]
        if not tabular and header != ["path", "attribute", "value"]:
            raise ValueError("unrecognized official CSV header")
        count = 0
        actual = Counter()
        for row in reader:
            if len(row) != len(header):
                raise ValueError("malformed official CSV row")
            count += 1
            actual[tuple(row[:-1])] += 1
    if status in {"SUCCESS", "SKIPPED"} and count:
        raise ValueError(f"{status} report contains CSV findings")
    if status == "ISSUE REPORTED":
        expected = Counter()
        details = json.loads(report_path.read_text(encoding="utf-8"))["Issue_Details"]
        for detail in details:
            locators = ("dataset", "row") if tabular else ("path",)
            prefix = tuple("" if detail[key] is None else str(detail[key]) for key in locators)
            names = detail["variables" if tabular else "attributes"]
            if not isinstance(names, list) or any(not isinstance(name, str) for name in names):
                raise ValueError("invalid JSON finding projection")
            for name in names or [""]:
                expected[(*prefix, name)] += 1
        if expected != actual:
            raise ValueError("CSV finding locators do not match the JSON report")
    return count


def run_case(root: Path, engine: Path, out: Path, case: dict, env: dict, timeout: int) -> dict:
    destination = out / "cases" / case["label"]
    destination.mkdir(parents=True)
    observation = {"label": case["label"], "rule_id": case["rule_id"], "fixture": case["fixture"],
                   "expected_status": case.get("expected_status"), "status": None,
                   "outcome": "not_executed", "errors": []}
    try:
        inputs = prepare_case(root, case, destination)
        observation["inputs"] = inputs
        argv = [str(engine / "core"), "validate", "-ca", str(engine / "resources/cache"), "-ps", "1",
                "-lr", inputs["rule"], "-d", inputs["data"], "-dep", str(Path(inputs["data"]) / ".env"),
                "-of", "JSON", "-of", "CSV", "-o", str(destination / "report"), "-p", "disabled", "-l", "info"]
        for key, value in case.get("overrides", {}).items():
            argv.extend([ENV_FLAGS[key], value])
        process = observation["process"] = execute(argv, engine, env, destination / "engine", timeout)
        if process["returncode"] != 0 or process["timed_out"]:
            observation["outcome"] = "process_error"
            observation["errors"].append("engine process failed or timed out")
        else:
            observation["outcome"] = "invalid_report"
            observation["report"] = inspect_report(destination / "report.json", case["rule_id"])
            observation["status"] = observation["report"]["status"]
            observation["csv_data_rows"] = verify_csv_projection(
                destination / "report.csv", destination / "report.json", observation["status"]
            )
            observation["csv_sha256"] = sha256(destination / "report.csv")
            observation["outcome"] = observation["status"].lower().replace(" ", "_")
            expected = case.get("expected_status")
            acceptable = {expected} if expected else {"SUCCESS", "ISSUE REPORTED"}
            if observation["status"] not in acceptable:
                observation["errors"].append("unexpected execution status")
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError) as error:
        observation["errors"].append(str(error))
    write_json(destination / "observation.json", observation)
    return observation


def run_audit(profile_path: Path, rules_root: Path, engine_archive: Path, out: Path) -> int:
    if os.path.lexists(out):
        raise ValueError("output already exists; choose a fresh directory")
    out.mkdir(parents=True, exist_ok=False)
    out, rules_root = out.resolve(), rules_root.resolve()
    summary = {"schema_version": 1, "conformance_claim": False, "audit_complete": False,
               "started_at": datetime.now(timezone.utc).isoformat(), "cases": [], "errors": [],
               "archive_verification": {"status": "not_verified"},
               "host": {"system": platform.system(), "machine": platform.machine(),
                        "platform": platform.platform(), "runner_python": sys.version}}
    exit_code = 2
    try:
        profile = load_profile(profile_path)
        summary["profile"] = profile
        summary["profile_sha256"] = sha256(profile_path)
        write_json(out / "profile.json", profile)
        check_platform(out, rules_root)
        reject_ambient_dotenv(out)
        summary["rules"] = check_rules(rules_root, profile["rules"])
        pins = profile["engine"]
        # CORE's multiprocessing manager binds a Unix socket under TMPDIR.
        with tempfile.TemporaryDirectory(prefix="core-audit-", dir="/tmp") as temporary:
            runtime = Path(temporary)
            engine_root = runtime / "core"
            summary.update(install_engine(engine_archive, runtime, pins))
            reject_ambient_dotenv(engine_root)
            cache = engine_root / "resources/cache"
            manifest = cache_manifest(cache)
            write_json(out / "cache-manifest.json", manifest)
            check_cache(manifest, pins)
            summary["cache_tree_sha256"] = manifest["tree_sha256"]
            summary["bundled_dependency_versions"] = {}
            for metadata_file in sorted(engine_root.glob("*.dist-info/METADATA")):
                metadata = Parser().parsestr(metadata_file.read_text(encoding="utf-8"))
                summary["bundled_dependency_versions"][metadata["Name"]] = metadata["Version"]
            env = isolated_environment(runtime)
            (runtime / "home").mkdir()
            (runtime / "tmp").mkdir()
            version_process = summary["version_process"] = execute(
                [str(engine_root / "core"), "version"], engine_root, env, out / "version", 60)
            version = Path(version_process["stdout"]).read_text().strip()
            summary["engine_version"] = version
            if version_process["returncode"] != 0 or version_process["timed_out"] or version != pins["version"]:
                raise ValueError("engine runtime version failed or differs from profile")
            for case in profile["cases"]:
                observation = run_case(rules_root, engine_root, out, case, env, profile.get("timeout_seconds", 300))
                summary["cases"].append(observation)
                write_json(out / "summary.json", summary)
            # Check pins again so a changed checkout/cache cannot silently pass a long run.
            check_rules(rules_root, profile["rules"])
            check_cache(cache_manifest(cache), pins)
        summary["audit_complete"] = not any(case["errors"] for case in summary["cases"])
        exit_code = 0 if summary["audit_complete"] else 1
    except (OSError, ValueError, KeyError, TypeError, EOFError, tarfile.TarError, subprocess.SubprocessError) as error:
        summary["errors"].append(str(error))
    finally:
        summary["finished_at"] = datetime.now(timezone.utc).isoformat()
        summary["exit_code"] = exit_code
        summary["outcome_counts"] = dict(Counter(case["outcome"] for case in summary["cases"]))
        write_json(out / "summary.json", summary)
    return exit_code


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    for flag in ("profile", "rules-root", "engine-archive", "out"):
        parser.add_argument(f"--{flag}", type=Path, required=True)
    args = parser.parse_args()
    try:
        return run_audit(args.profile, args.rules_root, args.engine_archive, args.out)
    except (OSError, ValueError) as error:
        print(str(error), file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
