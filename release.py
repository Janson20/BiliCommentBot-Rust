#!/usr/bin/env python3
"""
BiliCommentBot-RS 发版脚本

用法: python release.py x.x.x

功能:
  1. 校验版本号 (semver)
  2. 预检查: 工作区干净、tag 不存在、remote 已配置
  3. 同步更新 package.json / tauri.conf.json / Cargo.toml 版本号
  4. git commit + tag + push --follow-tags
  5. 触发 GitHub Action 自动构建并发布 Release
"""

import argparse
import json
import os
import re
import subprocess
import sys
from pathlib import Path

# --- 常量 ---
ROOT = Path(__file__).resolve().parent
VERSION_FILES = [
    ROOT / "package.json",
    ROOT / "src-tauri" / "tauri.conf.json",
    ROOT / "src-tauri" / "Cargo.toml",
]

SEMVER_RE = re.compile(r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$")


# ─── 工具函数 ──────────────────────────────────────────────

def run(cmd: list[str], *, cwd: Path = None, check: bool = True) -> subprocess.CompletedProcess:
    """运行命令，捕获输出；失败时打印 stderr 并退出"""
    try:
        result = subprocess.run(
            cmd, cwd=cwd or ROOT,
            capture_output=True, text=True, encoding="utf-8",
        )
        if check and result.returncode != 0:
            print(f"[ERROR] 命令失败: {' '.join(cmd)}")
            print(f"  stderr: {result.stderr.strip()}")
            sys.exit(1)
        return result
    except FileNotFoundError:
        print(f"[ERROR] 未找到命令: {cmd[0]}，请确保已安装并加入 PATH")
        sys.exit(1)


def run_shell(cmd: str, *, cwd: Path = None) -> subprocess.CompletedProcess:
    """使用 shell 执行命令（用于 git push --follow-tags 等复杂参数）"""
    result = subprocess.run(
        cmd, cwd=cwd or ROOT, shell=True,
        capture_output=True, text=True, encoding="utf-8",
    )
    if result.returncode != 0:
        print(f"[ERROR] 命令失败: {cmd}")
        print(f"  stderr: {result.stderr.strip()}")
        sys.exit(1)
    return result


# ─── 校验 ───────────────────────────────────────────────────

def validate_version(version: str) -> str:
    """校验 semver 格式，返回去掉前缀 v 的版本号"""
    v = version.strip().lstrip("v")
    if not SEMVER_RE.match(v):
        print(f"[ERROR] 版本号 '{version}' 格式非法，必须为 x.y.z（如 1.0.0）")
        sys.exit(1)
    return v


def pre_checks(tag: str):
    """发版前置检查"""
    print("[INFO] 执行前置检查...")

    # 1. 确保在 git 仓库中
    if not (ROOT / ".git").exists():
        print("[ERROR] 当前目录不是 git 仓库")
        sys.exit(1)

    # 2. 工作区干净
    status = run(["git", "status", "--porcelain"])
    if status.stdout.strip():
        print("[ERROR] 工作区不干净，请先提交或暂存所有变更:")
        print(status.stdout)
        sys.exit(1)

    # 3. tag 不存在
    tags = run(["git", "tag", "--list", tag])
    if tags.stdout.strip():
        print(f"[ERROR] Tag '{tag}' 已存在。请手动删除后重试: git tag -d {tag} && git push origin :refs/tags/{tag}")
        sys.exit(1)

    # 4. 有 remote 配置
    remote = run(["git", "remote", "get-url", "origin"])
    if not remote.stdout.strip():
        print("[ERROR] 未配置 git remote 'origin'")
        sys.exit(1)

    print(f"  remote origin: {remote.stdout.strip()}")
    print("[OK] 前置检查通过")


# ─── 版本号同步 ────────────────────────────────────────────

def update_package_json(version: str):
    """更新 package.json 中的 version 字段"""
    path = ROOT / "package.json"
    data = json.loads(path.read_text("utf-8"))
    old = data["version"]
    data["version"] = version
    path.write_text(json.dumps(data, indent=2, ensure_ascii=False) + "\n", "utf-8")
    print(f"  package.json: {old} -> {version}")


def update_tauri_conf(version: str):
    """更新 tauri.conf.json 中的 package.version 字段"""
    path = ROOT / "src-tauri" / "tauri.conf.json"
    data = json.loads(path.read_text("utf-8"))
    old = data["package"]["version"]
    data["package"]["version"] = version
    path.write_text(json.dumps(data, indent=2, ensure_ascii=False) + "\n", "utf-8")
    print(f"  tauri.conf.json: {old} -> {version}")


def update_cargo_toml(version: str):
    """更新 Cargo.toml 中 [package] 下的 version 字段"""
    path = ROOT / "src-tauri" / "Cargo.toml"
    lines = path.read_text("utf-8").splitlines(keepends=True)

    in_package = False
    found = False
    for i, line in enumerate(lines):
        if line.strip() == "[package]":
            in_package = True
            continue
        if in_package:
            if line.strip().startswith("["):
                # 退出 [package] section
                break
            m = re.match(r'^version\s*=\s*"([^"]*)"', line)
            if m:
                old = m.group(1)
                lines[i] = f'version = "{version}"\n'
                print(f"  Cargo.toml: {old} -> {version}")
                found = True
                break

    if not found:
        print("[ERROR] 未在 Cargo.toml 的 [package] 节中找到 version 字段")
        sys.exit(1)

    path.write_text("".join(lines), "utf-8")


def sync_versions(version: str):
    """同步所有版本文件"""
    print("[INFO] 同步版本号...")
    update_package_json(version)
    update_tauri_conf(version)
    update_cargo_toml(version)
    print("[OK] 版本号同步完成")


# ─── Git 操作 ───────────────────────────────────────────────

def git_commit_and_tag(version: str, tag: str):
    """提交版本变更文件并打 tag"""
    print("[INFO] 提交版本变更...")

    # git add 版本文件
    files = [str(f.relative_to(ROOT)) for f in VERSION_FILES]
    run(["git", "add"] + files)

    # git commit
    commit_msg = f"chore: bump version to {version}"
    run(["git", "commit", "-m", commit_msg])

    # git tag
    run(["git", "tag", tag])

    print(f"[OK] 已提交并打 tag: {tag}")


def git_push():
    """推送 commit 和 tag"""
    print("[INFO] 推送到 origin...")

    # 先推 commit
    branch = run(["git", "rev-parse", "--abbrev-ref", "HEAD"]).stdout.strip()
    run(["git", "push", "origin", branch])

    # 推送 tag (--follow-tags 需要 shell)
    run(["git", "push", "origin", "--tags"])

    print("[OK] 推送完成，GitHub Action 将自动构建并发布 Release")


# ─── 主流程 ─────────────────────────────────────────────────

def main():
    parser = argparse.ArgumentParser(
        description="BiliCommentBot-RS 发版脚本 - 同步版本号、提交、打 tag 并推送触发 CI",
    )
    parser.add_argument(
        "version",
        help="版本号，格式 x.y.z（如 1.2.3），自动添加 v 前缀作为 tag",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="仅打印操作不实际执行",
    )
    args = parser.parse_args()

    # 校验
    version = validate_version(args.version)
    tag = f"v{version}"

    print(f"{'='*60}")
    print(f"  BiliCommentBot-RS 发版脚本")
    print(f"  版本: {version}  |  Tag: {tag}")
    print(f"{'='*60}")
    print()

    if args.dry_run:
        print("[DRY-RUN] 仅预览模式，不会执行任何操作")
        print(f"[DRY-RUN] 将更新以下文件: {[f.name for f in VERSION_FILES]}")
        print(f"[DRY-RUN] git commit -m 'chore: bump version to {version}'")
        print(f"[DRY-RUN] git tag {tag}")
        print(f"[DRY-RUN] git push origin --tags")
        return

    # 前置检查
    pre_checks(tag)

    # 同步版本号
    sync_versions(version)

    # 提交 + 打 tag
    git_commit_and_tag(version, tag)

    # 推送
    git_push()

    print()
    print(f"{'='*60}")
    print(f"  发版流程完成！")
    print(f"  GitHub Action 监听 {tag} 将自动构建并发布 Release")
    print(f"{'='*60}")


if __name__ == "__main__":
    main()
