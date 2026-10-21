#!/usr/bin/env python3
"""Link a bounded entropy regression against the application's actual AWS-LC archive."""

import argparse
import pathlib
import re
import subprocess


def output(*args: str) -> str:
    return subprocess.check_output(args, text=True, timeout=60)


def check(archive: pathlib.Path, binary: pathlib.Path, compiler: str,
          headers: str, build_only: bool) -> None:
    symbols = output("nm", "-g", "--defined-only", str(archive))
    names = re.findall(r"\bT\s+(\w*CRYPTO_sysrand)$", symbols, re.MULTILINE)
    if len(names) != 1:
        raise RuntimeError(f"expected one AWS-LC entropy entry point in {archive}: {names}")
    undefined = output("nm", "-u", str(archive))
    if re.search(r"\bU\s+getentropy\b", undefined):
        raise RuntimeError(f"{archive} uses getentropy without the Linux ENOSYS fallback")
    source = pathlib.Path(__file__).with_name("linux-entropy-probe.c")
    subprocess.run(
        [compiler, "-O2", "-static", "-pthread", "-Wall", "-Wextra", "-Werror",
         "-idirafter", headers,
         f"-DENTROPY_FUNCTION={names[0]}", str(source), str(archive), "-o", str(binary)],
        check=True, timeout=60,
    )
    if "(NEEDED)" in output("readelf", "-d", str(binary)):
        raise RuntimeError("entropy probe must be statically linked")
    if re.search(r"\bINTERP\b", output("readelf", "-l", str(binary))):
        raise RuntimeError("entropy probe must not require an ELF interpreter")
    if not build_only:
        subprocess.run([str(binary.resolve())], check=True, timeout=40)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("build_dir", type=pathlib.Path)
    parser.add_argument("--compiler", default="musl-gcc")
    parser.add_argument("--headers", required=True)
    parser.add_argument("--output-dir", required=True, type=pathlib.Path)
    parser.add_argument("--build-only", action="store_true",
                        help="build for execution on separate hardware; does not validate runtime")
    args = parser.parse_args()
    archives = sorted(args.build_dir.glob("aws-lc-sys-*/out/**/libaws_lc*.a"))
    if not archives:
        raise RuntimeError(f"no built AWS-LC archives under {args.build_dir}")
    args.output_dir.mkdir(parents=True, exist_ok=True)
    # Validate every candidate: never silently select an unrelated cached build.
    for index, archive in enumerate(archives):
        print(f"Validating entropy from {archive}", flush=True)
        check(archive, args.output_dir / f"entropy-probe-{index}",
              args.compiler, args.headers, args.build_only)


if __name__ == "__main__":
    main()
